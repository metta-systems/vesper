use {
    crate::{CapError, Key, RawKey, Rights, decode_syscall_result},
    core::fmt,
};

#[cfg(not(test))]
use libsyscall::protected_call6;
#[cfg(test)]
use tests::protected_call6;

#[cfg(test)]
#[path = "../tests/support/frame.rs"]
mod tests;

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// The memory attribute of a Frame mapping (`Frame.Map`'s `x5`), and the code
/// of a device Frame's ceiling (`Untyped.Retype`'s `x3` bits 15:8).
///
/// The four hardware attributes are ordered by how much freedom the hardware
/// gets, strictest first: `DeviceStrict` < `DevicePosted` < `WriteCombining`
/// < `Cacheable`. [`Self::Default`] stands for the Frame's own default and is
/// not an attribute of its own.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameAttribute {
    /// The Frame's default: `Cacheable` for RAM, `DeviceStrict` for device
    /// memory. As a ceiling code: inherit the source's.
    Default = 0,
    /// Normal memory, write-back cacheable (MAIR index 0)
    Cacheable = 1,
    /// Normal memory, non-cacheable: writes may be merged and buffered
    /// (MAIR index 1)
    WriteCombining = 2,
    /// Device-nGnRE: no gathering or reordering, early write acknowledgement
    /// (MAIR index 2)
    DevicePosted = 3,
    /// Device-nGnRnE: no gathering, reordering or early acknowledgement
    /// (MAIR index 3)
    DeviceStrict = 4,
}

impl FrameAttribute {
    /// The attribute for a wire `code`, if defined.
    pub const fn from_code(code: u64) -> Option<Self> {
        match code {
            0 => Some(Self::Default),
            1 => Some(Self::Cacheable),
            2 => Some(Self::WriteCombining),
            3 => Some(Self::DevicePosted),
            4 => Some(Self::DeviceStrict),
            _ => None,
        }
    }

    /// The wire code.
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// How much freedom the hardware gets under the attribute: 0 for
    /// `DeviceStrict` up to 3 for `Cacheable`; none for [`Self::Default`].
    pub const fn freedom(self) -> Option<u8> {
        match self {
            Self::Default => None,
            Self::DeviceStrict => Some(0),
            Self::DevicePosted => Some(1),
            Self::WriteCombining => Some(2),
            Self::Cacheable => Some(3),
        }
    }

    /// The index into `MAIR_EL1` the attribute's mappings use; none for
    /// [`Self::Default`].
    pub const fn mair_index(self) -> Option<u8> {
        match self {
            Self::Default => None,
            Self::Cacheable => Some(0),
            Self::WriteCombining => Some(1),
            Self::DevicePosted => Some(2),
            Self::DeviceStrict => Some(3),
        }
    }

    /// Whether a device Frame with `ceiling` may be mapped with `self`: not
    /// looser than the ceiling. `Cacheable` is never allowed for a device
    /// Frame, and [`Self::Default`] is not an attribute to compare.
    pub const fn within_device_ceiling(self, ceiling: Self) -> bool {
        match (self.freedom(), ceiling.freedom()) {
            (Some(requested), Some(allowed)) => {
                requested <= allowed && !matches!(self, Self::Cacheable)
            }
            _ => false,
        }
    }
}

/// Userspace handle to a Frame capability.
///
/// Nucleus dispatch supports `Map`, `Unmap`, `GetAddress` and the four cache
/// maintenance operations; the wrapper
/// preserves kernel errors. Operation ID `7` is unassigned.
pub struct FrameKey {
    key: Key<FrameType>,
}

enum FrameType {}

/// Existing operation vocabulary; decoding an ID does not imply kernel support.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameOp {
    /// Map the frame into a target Domain's translation context at a virtual
    /// address. Bootstrap-era schema: the explicit target Domain capability
    /// exists so an authorized builder can populate a Domain's address space
    /// before it runs; self-context mapping is the intended ordinary path once
    /// syscall caller identity exists.
    Map = 0,
    /// Unmap the frame through its recorded mapping identity.
    Unmap = 1,
    /// Query the physical extent (requires `GRANT`).
    GetAddress = 2,
    /// Write dirty cache lines of a byte range back to the Point of Coherency
    /// (requires `READ`).
    CleanData = 3,
    /// Discard the cache lines of a byte range (requires `WRITE`).
    InvalidateData = 4,
    /// Clean, then discard, the cache lines of a byte range (requires `READ`).
    CleanInvalidateData = 5,
    /// Clean a byte range to the Point of Unification and invalidate the
    /// instruction caches (requires `READ`).
    UnifyInstruction = 6,
}

impl TryFrom<u64> for FrameOp {
    type Error = CapError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Map),
            1 => Ok(Self::Unmap),
            2 => Ok(Self::GetAddress),
            3 => Ok(Self::CleanData),
            4 => Ok(Self::InvalidateData),
            5 => Ok(Self::CleanInvalidateData),
            6 => Ok(Self::UnifyInstruction),

            _ => Err(CapError::InvalidOperation),
        }
    }
}

impl TryFrom<u32> for FrameOp {
    type Error = CapError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::try_from(u64::from(value))
    }
}

impl FrameKey {
    /// Construct a non-owning handle without installing or validating authority.
    pub const fn from_key(key: RawKey) -> Self {
        Self { key: Key::new(key) }
    }

    /// The wire encoding of the underlying key.
    pub const fn to_wire(&self) -> u64 {
        self.key.to_wire()
    }

    /// The underlying key, for passing to `Invocation.CallWithKey`.
    pub const fn raw(&self) -> RawKey {
        self.key.raw()
    }

    /// Map this frame into the target Domain's translation context at `vaddr`.
    ///
    /// Wire schema (selected 2026-09-15): `x2` target `Domain` key, `x3` virtual
    /// address, `x4` requested rights, `x5` the memory [`FrameAttribute`],
    /// `x6..x7` zero. The requested rights must be a subset of the frame
    /// capability's rights, and the attribute within the Frame's ceiling. The
    /// walk requires every intermediate page table to be present.
    pub fn map(
        &self,
        domain: RawKey,
        vaddr: u64,
        rights: Rights,
        attribute: FrameAttribute,
    ) -> Result<(), CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result = unsafe {
            protected_call6(
                self.key.to_wire(),
                FrameOp::Map as u64,
                domain.to_wire(),
                vaddr,
                u64::from(rights.bits()),
                u64::from(attribute.code()),
                0,
                0,
            )
        };
        decode_syscall_result(result).map(|_| ())
    }

    /// Unmap this frame through its recorded mapping identity.
    ///
    /// Wire schema: no arguments (`x2..x7` zero).
    pub fn unmap(&self) -> Result<(), CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result =
            unsafe { protected_call6(self.key.to_wire(), FrameOp::Unmap as u64, 0, 0, 0, 0, 0, 0) };
        decode_syscall_result(result).map(|_| ())
    }

    /// Query the frame's physical extent (requires `GRANT`).
    ///
    /// Returns `(physical base address, size in bytes)`.
    pub fn get_extent(&self) -> Result<(u64, u64), CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result = unsafe {
            protected_call6(
                self.key.to_wire(),
                FrameOp::GetAddress as u64,
                0,
                0,
                0,
                0,
                0,
                0,
            )
        };
        decode_syscall_result(result)
    }

    /// Write the dirty cache lines of bytes `start..end` of the frame back to
    /// the Point of Coherency (requires `READ`).
    ///
    /// Wire schema: `x2` start offset, `x3` end offset (exclusive), `x4..x7`
    /// zero; `start < end <= size`.
    pub fn clean_data(&self, start: u64, end: u64) -> Result<(), CapError> {
        self.maintain_cache(FrameOp::CleanData, start, end)
    }

    /// Discard the cache lines of bytes `start..end` (requires `WRITE`); a
    /// partial line at either end is cleaned first, so bytes outside the
    /// range keep their values.
    pub fn invalidate_data(&self, start: u64, end: u64) -> Result<(), CapError> {
        self.maintain_cache(FrameOp::InvalidateData, start, end)
    }

    /// Clean, then discard, the cache lines of bytes `start..end` (requires
    /// `READ`).
    pub fn clean_invalidate_data(&self, start: u64, end: u64) -> Result<(), CapError> {
        self.maintain_cache(FrameOp::CleanInvalidateData, start, end)
    }

    /// Make code written to bytes `start..end` through a data mapping visible
    /// to instruction fetch (requires `READ`).
    pub fn unify_instruction(&self, start: u64, end: u64) -> Result<(), CapError> {
        self.maintain_cache(FrameOp::UnifyInstruction, start, end)
    }

    fn maintain_cache(&self, op: FrameOp, start: u64, end: u64) -> Result<(), CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result =
            unsafe { protected_call6(self.key.to_wire(), op as u64, start, end, 0, 0, 0, 0) };
        decode_syscall_result(result).map(|_| ())
    }
}

impl fmt::Debug for FrameKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FrameKey").field("key", &self.key).finish()
    }
}

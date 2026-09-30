use {
    crate::{CapError, Key, RawKey, decode_syscall_result},
    core::fmt,
};

#[cfg(not(test))]
use libsyscall::protected_call6;
#[cfg(test)]
use tests::protected_call6;

#[cfg(test)]
#[path = "../tests/support/page_table.rs"]
mod tests;

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Userspace handle to a `PageTable` capability.
///
/// Nucleus dispatch supports `Map` (installation into a parent) and `Unmap`.
pub struct PageTableKey {
    key: Key<PageTableType>,
}

enum PageTableType {}

/// Existing operation vocabulary; decoding an ID does not imply kernel support.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PageTableOp {
    /// Install this table into a parent named by capability type: a `Domain`
    /// capability installs the translation root (vaddr must be zero), a
    /// `PageTable` capability installs one intermediate level (vaddr selects the
    /// parent slot).
    Map = 0,
    /// Unmap this table from its parent; the table must be empty.
    Unmap = 1,
}

impl TryFrom<u64> for PageTableOp {
    type Error = CapError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Map),
            1 => Ok(Self::Unmap),
            _ => Err(CapError::InvalidOperation),
        }
    }
}

impl TryFrom<u32> for PageTableOp {
    type Error = CapError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::try_from(u64::from(value))
    }
}

impl PageTableKey {
    /// Construct a non-owning handle without installing or validating authority.
    pub const fn from_key(key: RawKey) -> Self {
        Self { key: Key::new(key) }
    }

    /// The wire encoding of the underlying key.
    pub const fn to_wire(&self) -> u64 {
        self.key.to_wire()
    }

    /// Install this table into `parent` (a `Domain` or `PageTable` capability).
    ///
    /// Wire schema (selected 2026-09-15): `x2` parent key, `x3` virtual
    /// address, `x4..x7` zero. For a `Domain` parent the virtual address must be
    /// zero (root installation); for a `PageTable` parent the virtual address
    /// selects the parent slot and the parent must already be installed.
    pub fn map(&self, parent: RawKey, vaddr: u64) -> Result<(), CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result = unsafe {
            protected_call6(
                self.key.to_wire(),
                PageTableOp::Map as u64,
                parent.to_wire(),
                vaddr,
                0,
                0,
                0,
                0,
            )
        };
        decode_syscall_result(result).map(|_| ())
    }

    /// Unmap this table from its parent; the table must be empty.
    ///
    /// Wire schema: no arguments (`x2..x7` zero).
    pub fn unmap(&self) -> Result<(), CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result = unsafe {
            protected_call6(
                self.key.to_wire(),
                PageTableOp::Unmap as u64,
                0,
                0,
                0,
                0,
                0,
                0,
            )
        };
        decode_syscall_result(result).map(|_| ())
    }
}

impl fmt::Debug for PageTableKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PageTableKey")
            .field("key", &self.key)
            .finish()
    }
}

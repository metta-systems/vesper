use {
    crate::{board_console, objects::NucleusObject},
    libobject::{CapError, ObjectType, debug_console::WRITE_LIMIT},
};

// ====================
// == Nucleus object ==
// ====================

pub struct DebugConsole;

/// The exception level a Write's caller runs at, which its bytes must be
/// readable at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reader {
    /// A Thread at EL0
    El0,
    /// The boot code at EL1
    El1,
}

/// Bytes of one translation granule: the step [`readable`] checks in.
const PAGE: u64 = 4096;

impl DebugConsole {
    /// Print the first [`WRITE_LIMIT`] of the `length` bytes at `address` in
    /// the caller's `AddressSpace` (the active one), as they are: on
    /// semihosting under `qemu` (up to the first NUL), on the board console
    /// otherwise.
    ///
    /// The bytes are copied out before they are printed, so a caller that
    /// changes them meanwhile changes nothing.
    ///
    /// # Errors
    ///
    /// `InvalidPointer` if `reader` may not read some page of the range, or
    /// the range wraps; nothing is printed then.
    pub fn handle_write(address: u64, length: u64, reader: Reader) -> Result<(), CapError> {
        let length = usize::try_from(length).map_or(WRITE_LIMIT, |length| length.min(WRITE_LIMIT));
        if !readable(address, length, reader) {
            return Err(CapError::InvalidPointer);
        }
        // One more byte for the NUL semihosting needs.
        let mut copy = [0_u8; WRITE_LIMIT + 1];
        for (offset, byte) in (0..).zip(&mut copy[..length]) {
            // SAFETY: `readable` found every page of the range readable at
            // the caller's level in the active `AddressSpace`, which stays
            // active through the call; PAN is off, so EL1 reads what EL0 may.
            *byte = unsafe { ((address + offset) as *const u8).read_volatile() };
        }
        #[cfg(feature = "qemu")]
        if let Ok(text) = core::ffi::CStr::from_bytes_until_nul(&copy) {
            libqemu::semihosting::sys_write0_call(text);
        }
        board_console::bytes(&copy[..length]);
        Ok(())
    }
}

/// Whether `reader` may read each of the `length` bytes at `address` in the
/// active `AddressSpace`: every page of the range translates for its read.
fn readable(address: u64, length: usize, reader: Reader) -> bool {
    let Some(last) = length
        .checked_sub(1)
        .and_then(|last| address.checked_add(last as u64))
    else {
        // Empty (readable) or wrapping (not).
        return length == 0;
    };
    let mut page = address & !(PAGE - 1);
    loop {
        if !translates(page, reader) {
            return false;
        }
        match page.checked_add(PAGE) {
            Some(next) if next <= last => page = next,
            _ => return true,
        }
    }
}

/// Whether a read of `address` at `reader`'s level translates in the active
/// `AddressSpace` (`AT S1E0R` or `AT S1E1R`).
fn translates(address: u64, reader: Reader) -> bool {
    use aarch64_cpu::registers::{PAR_EL1, Read};
    // SAFETY: `AT` only writes PAR_EL1, which nothing else holds across this
    // call: the nucleus runs with interrupts masked.
    unsafe {
        match reader {
            Reader::El0 => core::arch::asm!(
                "at s1e0r, {address}",
                "isb",
                address = in(reg) address,
                options(nostack, preserves_flags)
            ),
            Reader::El1 => core::arch::asm!(
                "at s1e1r, {address}",
                "isb",
                address = in(reg) address,
                options(nostack, preserves_flags)
            ),
        }
    }
    !PAR_EL1.is_set(PAR_EL1::F)
}

impl NucleusObject for DebugConsole {
    const TYPE: ObjectType = ObjectType::DEBUG_CONSOLE;
}

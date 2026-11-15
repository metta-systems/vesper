use {
    crate::{
        board_console,
        objects::NucleusObject,
        user_memory::{Reader, read_byte, readable},
    },
    libobject::{CapError, ObjectType, debug_console::WRITE_LIMIT},
};

// ====================
// == Nucleus object ==
// ====================

pub struct DebugConsole;

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
            // active through the call.
            *byte = unsafe { read_byte(address + offset, reader) };
        }
        #[cfg(feature = "qemu")]
        if let Ok(text) = core::ffi::CStr::from_bytes_until_nul(&copy) {
            libqemu::semihosting::sys_write0_call(text);
        }
        board_console::bytes(&copy[..length]);
        Ok(())
    }
}

impl NucleusObject for DebugConsole {
    const TYPE: ObjectType = ObjectType::DEBUG_CONSOLE;
}

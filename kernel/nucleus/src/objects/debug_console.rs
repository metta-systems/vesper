use {
    crate::objects::NucleusObject,
    core::slice,
    libaddress::PhysAddr,
    libobject::{CapError, ObjectType},
};

// ====================
// == Nucleus object ==
// ====================

pub struct DebugConsole;

impl DebugConsole {
    pub fn handle_write(ptr: PhysAddr, len: u64) -> Result<(), CapError> {
        // semi::println!(
        //     "DebugConsole::handle_write(user ptr {ptr:?}, kernel ptr {:?}, size {})",
        //     ptr.user_to_kernel(),
        //     len
        // );
        let len = usize::try_from(len).unwrap();
        // SAFETY: Unsafe, need to check user pointers.
        let slice = unsafe { slice::from_raw_parts(ptr.user_to_kernel().as_ptr(), len) };
        let mut buf = [0_u8; 4096];
        // semi::println!(
        //     "DebugConsole::copy from user to {:#08x}",
        //     &buf as *const _ as u64
        // );
        buf[..len].copy_from_slice(slice);
        buf[slice.len()] = 0;
        let cstr =
            // SAFETY: Need to validate user pointer is valid, need to copy via kernel physmem mapping.
            unsafe { core::ffi::CStr::from_bytes_with_nul(&buf[..=slice.len()]) }.map_err(|e| {
                // semi::println!("{e}");
                CapError::Unknown
            })?;
        #[cfg(feature = "qemu")]
        libqemu::semihosting::sys_write0_call(cstr);
        Ok(())
    }
}

impl NucleusObject for DebugConsole {
    const TYPE: ObjectType = ObjectType::DEBUG_CONSOLE;
}

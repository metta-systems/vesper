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
        // libqemu::semi_println!(
        //     "DebugConsole::handle_write(user ptr {ptr:?}, kernel ptr {:?}, size {})",
        //     ptr.user_to_kernel(),
        //     len
        // );
        let slice = unsafe { slice::from_raw_parts(ptr.user_to_kernel().as_ptr(), len as usize) };
        let mut buf = [0u8; 4096];
        // libqemu::semi_println!(
        //     "DebugConsole::copy from user to {:#08x}",
        //     &buf as *const _ as u64
        // );
        // SAFETY: Need to validate user pointer is valid, need to copy via kernel physmem mapping.
        buf[..len as usize].copy_from_slice(slice);
        buf[slice.len()] = 0;
        let cstr =
            unsafe { core::ffi::CStr::from_bytes_with_nul(&buf[..=slice.len()]) }.map_err(|e| {
                // libqemu::semi_println!("{e}");
                CapError::Unknown
            })?;
        #[cfg(qemu)]
        libqemu::semihosting::sys_write0_call(cstr);
        Ok(())
    }
}

impl NucleusObject for DebugConsole {
    const TYPE: ObjectType = ObjectType::DEBUG_CONSOLE;
}

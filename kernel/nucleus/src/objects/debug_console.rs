use {core::slice, libobject::ObjectType};

// ====================
// == Nucleus object ==
// ====================

struct DebugConsole;

impl DebugConsole {
    fn handle_write(ptr: u64, len: u64) -> Result<(), SyscallError> {
        let slice = unsafe { slice::from_raw_parts(ptr as *const u8, len as usize) };
        let mut buf = [0u8; 4096];
        // SAFETY: Need to validate user pointer is valid, need to copy via kernel physmem mapping.
        buf.copy_from_slice(slice);
        buf[slice.len()] = 0;
        let cstr = unsafe { core::ffi::CStr::from_bytes_with_nul(&buf[..=slice.len() + 1]) }
            .map_err(SyscallError::Unknown)?;
        libqemu::semihosting::sys_write0_call(cstr);
        Ok(())
    }
}

impl NucleusObject for DebugConsole {
    const TYPE: ObjectType = ObjectType::DEBUG_CONSOLE;
}

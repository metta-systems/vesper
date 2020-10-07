pub mod semihosting {
    pub fn exit_success() {
        use qemu_exit::QEMUExit;

        #[cfg(target_arch = "aarch64")]
        let qemu_exit_handle = qemu_exit::AArch64::new();

        qemu_exit_handle.exit_success();
    }

    #[cfg(test)]
    pub fn sys_write0_call(text: &str) {
        // SAFETY: text must be \0-terminated!
        unsafe {
            asm!(
            "mov w0, #0x04
             hlt #0xF000"
            , in("x1") text.as_ptr() as u64
            );
        }
    }
}

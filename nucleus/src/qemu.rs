pub mod semihosting {
    #[cfg(test)]
    pub fn exit_success() {
        use qemu_exit::QEMUExit;

        #[cfg(target_arch = "aarch64")]
        let qemu_exit_handle = qemu_exit::AArch64::new();

        qemu_exit_handle.exit_success();
    }

    #[cfg(test)]
    pub fn sys_write0_call(text: &str) {
        // SAFETY: text must be \0-terminated!
        let cmd = 0x04;
        unsafe {
            asm!(
                "hlt #0xF000"
                , in("w0") cmd
                , in("x1") text.as_ptr() as u64
            );
        }
    }
}

/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */
pub mod semihosting {
    pub fn exit_success() -> ! {
        use qemu_exit::QEMUExit;

        #[cfg(target_arch = "aarch64")]
        let qemu_exit_handle = qemu_exit::AArch64::new();

        qemu_exit_handle.exit_success()
    }

    #[cfg(test)]
    pub fn exit_failure() -> ! {
        use qemu_exit::QEMUExit;

        #[cfg(target_arch = "aarch64")]
        let qemu_exit_handle = qemu_exit::AArch64::new();

        qemu_exit_handle.exit_failure()
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

    #[allow(non_upper_case_globals)]
    const ADP_Stopped_BreakPoint: u64 = 0x20020;

    #[repr(C)]
    struct qemu_parameter_block {
        arg0: u64,
        arg1: u64,
    }

    fn sys_exit_call(block: &qemu_parameter_block) {
        let cmd = 0x18;
        unsafe {
            asm!(
                "hlt #0xF000"
                 , in("w0") cmd
                 , in("x1") block as *const _ as u64
            );
        }
    }

    fn sys_breakpoint_call() {
        let block = qemu_parameter_block {
            arg0: ADP_Stopped_BreakPoint,
            arg1: 0,
        };

        sys_exit_call(&block)
    }
}

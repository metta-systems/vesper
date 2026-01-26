/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

#![no_std]
#![no_main]
#![feature(format_args_nl)]
#![feature(custom_test_frameworks)]
#![test_runner(libtest::test_runner)]
#![reexport_test_harness_main = "test_main"]

pub mod semihosting {
    pub fn exit_success() -> ! {
        use qemu_exit::QEMUExit;

        #[cfg(target_arch = "aarch64")]
        let qemu_exit_handle = qemu_exit::AArch64::new();

        qemu_exit_handle.exit_success()
    }

    pub fn exit_failure() -> ! {
        use qemu_exit::QEMUExit;

        #[cfg(target_arch = "aarch64")]
        let qemu_exit_handle = qemu_exit::AArch64::new();

        qemu_exit_handle.exit_failure()
    }

    pub fn sys_write0_call(text: &core::ffi::CStr) {
        let cmd = 0x04;
        // SAFETY: text must be \0-terminated, which CStr above shall ensure.
        unsafe {
            core::arch::asm!(
                "hlt #0xF000"
                , in("w0") cmd
                , in("x1") text.as_ptr() as u64
            );
        }
    }

    #[macro_export]
    macro_rules! semi_print {
        // early_print!("a {} event", "log")
        ($($arg:tt)+) => {
            let mut buf = [0_u8; 4096]; // Increase this buffer size to allow dumping larger panic texts.
            libqemu::semihosting::sys_write0_call(
                libprint::format_cstr(&mut buf, core::format_args!($($arg)+)).unwrap(),
            );
        };
    }

    #[macro_export]
    macro_rules! semi_println {
        // semi_println!()
        () => {
            let mut buf = [0_u8; 4096]; // Increase this buffer size to allow dumping larger panic texts.
            libqemu::semihosting::sys_write0_call(
                libprint::format_cstr(&mut buf, core::format_args_nl!("")).unwrap(),
            );
        };
        // semi_println!("a {} event", "log")
        ($($arg:tt)+) => {
            let mut buf = [0_u8; 4096]; // Increase this buffer size to allow dumping larger panic texts.
            libqemu::semihosting::sys_write0_call(
                libprint::format_cstr(&mut buf, core::format_args_nl!($($arg)+)).unwrap(),
            );
        };
    }
}

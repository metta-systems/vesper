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
    #[repr(C)]
    struct ExitParameters {
        arg0: u64,
        arg1: u64,
    }

    #[expect(non_upper_case_globals)]
    const ADP_Stopped_ApplicationExit: u64 = 0x20026;

    pub fn exit_success() -> ! {
        let params = ExitParameters {
            arg0: ADP_Stopped_ApplicationExit,
            arg1: 0,
        };
        sys_exit_call(&params)
    }

    pub fn exit_failure() -> ! {
        let params = ExitParameters {
            arg0: ADP_Stopped_ApplicationExit,
            arg1: 1,
        };
        sys_exit_call(&params)
    }

    fn sys_exit_call(params: &ExitParameters) -> ! {
        // SAFETY: safe enough!
        unsafe {
            core::arch::asm!(
                "hlt #0xF000",
                in("x0") 0x18, // Sys_Exit
                in("x1") core::ptr::from_ref(params) as u64,
                options(nostack)
            );
            loop {
                core::arch::asm!("wfe", options(nomem, nostack));
            }
        }
    }

    pub fn sys_write0_call(text: &core::ffi::CStr) {
        // SAFETY: text must be \0-terminated, which CStr above shall ensure.
        unsafe {
            core::arch::asm!(
                "hlt #0xF000"
                , in("x0") 0x04 // Sys_Write0
                , in("x1") text.as_ptr() as u64,
                options(nostack)
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

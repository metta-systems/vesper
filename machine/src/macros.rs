/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

/// Macro similar to [std](https://doc.rust-lang.org/src/std/macros.rs.html)
/// but for writing into kernel-specific output (UART or QEMU console).
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::macros::_print(format_args!($($arg)*)));
}

/// Macro similar to [std](https://doc.rust-lang.org/src/std/macros.rs.html)
/// but for writing into kernel-specific output (UART or QEMU console).
#[macro_export]
macro_rules! println {
    () => (print!("\n"));
    ($($arg:tt)*) => ({
        $crate::macros::_print(format_args_nl!($($arg)*));
    })
}

#[doc(hidden)]
#[cfg(not(any(test, qemu)))]
pub fn _print(args: core::fmt::Arguments) {
    use {crate::console::console, core::fmt::Write};
    console().write_fmt(args).unwrap();
}

/// qemu-based tests use semihosting write0 syscall.
#[doc(hidden)]
#[cfg(any(test, qemu))] // qemu feature not enabled here?? we pass --features=qemu to cargo test
pub fn _print(args: core::fmt::Arguments) {
    use crate::{qemu, write_to};

    let mut buf = [0u8; 2048]; // Increase this buffer size to allow dumping larger panic texts.
    qemu::semihosting::sys_write0_call(write_to::c_show(&mut buf, args).unwrap());
}

//--------------------------------------------------------------------------------------------------
//--------------------------------------------------------------------------------------------------

/// Prints info text, with a newline.
#[macro_export]
macro_rules! info {
    ($string:expr) => ({
        let timestamp = $crate::time::time_manager().uptime();

        $crate::macros::_print(format_args_nl!(
            concat!("[  {:>3}.{:06}] ", $string),
            timestamp.as_secs(),
            timestamp.subsec_micros(),
        ));
    });
    ($format_string:expr, $($arg:tt)*) => ({
        let timestamp = $crate::time::time_manager().uptime();

        $crate::macros::_print(format_args_nl!(
            concat!("[  {:>3}.{:06}] ", $format_string),
            timestamp.as_secs(),
            timestamp.subsec_micros(),
            $($arg)*
        ));
    })
}

/// Prints warning text, with a newline.
#[macro_export]
macro_rules! warn {
    ($string:expr) => ({
        let timestamp = $crate::time::time_manager().uptime();

        $crate::macros::_print(format_args_nl!(
            concat!("[W {:>3}.{:06}] ", $string),
            timestamp.as_secs(),
            timestamp.subsec_micros(),
        ));
    });
    ($format_string:expr, $($arg:tt)*) => ({
        let timestamp = $crate::time::time_manager().uptime();

        $crate::macros::_print(format_args_nl!(
            concat!("[W {:>3}.{:06}] ", $format_string),
            timestamp.as_secs(),
            timestamp.subsec_micros(),
            $($arg)*
        ));
    })
}

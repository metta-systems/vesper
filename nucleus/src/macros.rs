/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

// https://doc.rust-lang.org/src/std/macros.rs.html
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::macros::_print(format_args!($($arg)*)));
}

// https://doc.rust-lang.org/src/std/macros.rs.html
#[macro_export]
macro_rules! println {
    () => (print!("\n"));
    ($($arg:tt)*) => ({
        $crate::macros::_print(format_args_nl!($($arg)*));
    })
}

#[doc(hidden)]
#[cfg(not(any(test, qemu)))]
pub fn _print(_args: core::fmt::Arguments) {
    // @todo real system implementation
}

#[doc(hidden)]
#[cfg(any(test, qemu))] // qemu feature not enabled here?? we pass --features=qemu to cargo test
pub fn _print(args: core::fmt::Arguments) {
    use crate::{qemu, write_to};
    let mut buf = [0u8; 512];
    qemu::semihosting::sys_write0_call(write_to::c_show(&mut buf, args).unwrap());
}

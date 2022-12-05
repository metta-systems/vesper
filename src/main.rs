#![no_std]
#![no_main]

#[cfg(not(target_arch = "aarch64"))]
use architecture_not_supported_sorry;

pub mod boot;

/// Loop forever in sleep mode.
#[inline]
pub fn endless_sleep() -> ! {
    loop {
        aarch64_cpu::asm::wfe();
    }
}

use armv8a_panic_semihosting as _;

#[export_name = "main"]
#[inline(always)]
pub unsafe fn __main() -> ! {
    kernel_main();
}

/// Kernel entry point.
pub fn kernel_main() -> ! {
    armv8a_semihosting::hprintln!("Letsgo!").ok();

    armv8a_semihosting::hprintln!("Lets {}!", "go").ok(); // culprit

    armv8a_semihosting::hprintln!("Lets go 2!").ok();
    panic!("Off you go!");
}

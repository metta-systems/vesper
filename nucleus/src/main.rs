#![no_std]
#![no_main]

#[cfg(not(target_arch = "aarch64"))]
use architecture_not_supported_sorry;

#[macro_use]
pub mod arch;
pub use arch::*;

// Kernel entry point
// arch crate is responsible for calling this
pub fn kmain() -> ! {
    endless_sleep()
}

#[panic_handler]
fn panicked(_info: &core::panic::PanicInfo) -> ! {
    endless_sleep()
}

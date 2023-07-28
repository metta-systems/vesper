#![no_std]
#![no_main]
#![allow(stable_features)]
#![feature(decl_macro)]
#![feature(ptr_internals)]
#![feature(allocator_api)]
#![feature(format_args_nl)]
#![feature(core_intrinsics)]
#![feature(strict_provenance)]
#![feature(stmt_expr_attributes)]
#![feature(slice_ptr_get)]
#![feature(panic_info_message)]
#![feature(nonnull_slice_from_raw_parts)] // stabilised in 1.71 nightly
#![feature(custom_test_frameworks)]
#![test_runner(crate::tests::test_runner)]
#![reexport_test_harness_main = "test_main"]
#![allow(clippy::upper_case_acronyms)]
#![allow(clippy::enum_variant_names)]
#![allow(clippy::nonstandard_macro_braces)] // https://github.com/shepmaster/snafu/issues/296
#![allow(missing_docs)] // Temp: switch to deny
#![deny(warnings)]
#![allow(unused)]

#[cfg(not(target_arch = "aarch64"))]
use architecture_not_supported_sorry;

/// Architecture-specific code.
#[macro_use]
pub mod arch;

pub use arch::*;

pub mod console;
pub mod devices;
pub mod drivers;
pub mod macros;
pub mod memory;
mod mm;
pub mod mmio_deref_wrapper;
pub mod panic;
pub mod platform;
pub mod qemu;
mod sync;
pub mod tests;
pub mod write_to;

// The global allocator for DMA-able memory. That is, memory which is tagged
// non-cacheable in the page tables.
// #[allow(dead_code)]
// static DMA_ALLOCATOR: sync::NullLock<Lazy<BuddyAlloc>> =
//     sync::NullLock::new(Lazy::new(|| unsafe {
//         BuddyAlloc::new(BuddyAllocParam::new(
//             // @todo Init this after we loaded boot memory map
//             DMA_HEAP_START as *const u8,
//             DMA_HEAP_END - DMA_HEAP_START,
//             64,
//         ))
//     }));
// Try the following arguments instead to see all mailbox operations
// fail. It will cause the allocator to use memory that is marked
// cacheable and therefore not DMA-safe. The answer from the VideoCore
// won't be received by the CPU because it reads an old cached value
// that resembles an error case instead.

// 0x00600000 as usize,
// 0x007FFFFF as usize,

#[cfg(test)]
mod lib_tests {
    #[panic_handler]
    fn panicked(info: &core::panic::PanicInfo) -> ! {
        crate::panic::handler_for_tests(info)
    }

    /// Main for running tests.
    #[no_mangle]
    pub unsafe fn main() -> ! {
        crate::test_main();
        crate::qemu::semihosting::exit_success()
    }
}

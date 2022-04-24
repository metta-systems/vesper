#![no_std]
#![no_main]
#![feature(decl_macro)]
#![feature(allocator_api)]
#![feature(format_args_nl)]
#![feature(stmt_expr_attributes)]
#![feature(nonnull_slice_from_raw_parts)]
#![feature(custom_test_frameworks)]
#![test_runner(crate::tests::test_runner)]
#![reexport_test_harness_main = "test_main"]
#![allow(clippy::upper_case_acronyms)]
#![allow(clippy::enum_variant_names)]
#![allow(clippy::nonstandard_macro_braces)] // https://github.com/shepmaster/snafu/issues/296
#![allow(missing_docs)] // Temp: switch to deny
#![deny(warnings)]

#[cfg(not(target_arch = "aarch64"))]
use architecture_not_supported_sorry;

/// Architecture-specific code.
#[macro_use]
pub mod arch;

pub use arch::*;

pub mod devices;
pub mod macros;
mod mm;
pub mod panic;
pub mod platform;
pub mod qemu;
mod sync;
pub mod tests;
pub mod write_to;

/// The global console. Output of the kernel print! and println! macros goes here.
pub static CONSOLE: sync::NullLock<devices::Console> = sync::NullLock::new(devices::Console::new());

/// The global allocator for DMA-able memory. That is, memory which is tagged
/// non-cacheable in the page tables.
#[allow(dead_code)]
static DMA_ALLOCATOR: sync::NullLock<mm::BumpAllocator> =
    sync::NullLock::new(mm::BumpAllocator::new(
        // @todo Init this after we loaded boot memory map
        memory::map::virt::DMA_HEAP_START as usize,
        memory::map::virt::DMA_HEAP_END as usize,
        "Global DMA Allocator",
        // Try the following arguments instead to see all mailbox operations
        // fail. It will cause the allocator to use memory that are marked
        // cacheable and therefore not DMA-safe. The answer from the VideoCore
        // won't be received by the CPU because it reads an old cached value
        // that resembles an error case instead.

        // 0x00600000 as usize,
        // 0x007FFFFF as usize,
        // "Global Non-DMA Allocator",
    ));

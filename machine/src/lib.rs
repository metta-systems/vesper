#![no_std]
#![no_main]
#![feature(try_find)] // For DeviceTree iterators
#![feature(decl_macro)]
#![feature(allocator_api)]
#![feature(format_args_nl)]
#![feature(core_intrinsics)]
#![feature(stmt_expr_attributes)]
#![feature(slice_ptr_get)]
#![feature(nonnull_slice_from_raw_parts)]
#![feature(custom_test_frameworks)]
#![feature(default_free_fn)]
#![test_runner(crate::tests::test_runner)]
#![reexport_test_harness_main = "test_main"]
#![allow(clippy::upper_case_acronyms)]
#![allow(clippy::enum_variant_names)]
#![allow(clippy::nonstandard_macro_braces)] // https://github.com/shepmaster/snafu/issues/296
#![allow(missing_docs)] // Temp: switch to deny
#![deny(warnings)]

#[cfg(not(target_arch = "aarch64"))]
use architecture_not_supported_sorry;
use core::{
    alloc::{AllocError, Allocator, Layout},
    ptr::NonNull,
};

use {
    buddy_alloc::{BuddyAlloc, BuddyAllocParam},
    once_cell::unsync::Lazy,
    platform::memory::map::virt::{DMA_HEAP_END, DMA_HEAP_START},
};

/// Architecture-specific code.
#[macro_use]
pub mod arch;

pub use arch::*;

pub mod boot_info;
pub mod device_tree;
pub mod devices;
pub mod macros;
pub mod memory;
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
static DMA_ALLOCATOR: sync::NullLock<Lazy<BuddyAlloc>> =
    sync::NullLock::new(Lazy::new(|| unsafe {
        BuddyAlloc::new(BuddyAllocParam::new(
            // @todo Init this after we loaded boot memory map
            DMA_HEAP_START as *const u8,
            DMA_HEAP_END - DMA_HEAP_START,
            64,
        ))
    }));
// Try the following arguments instead to see all mailbox operations
// fail. It will cause the allocator to use memory that are marked
// cacheable and therefore not DMA-safe. The answer from the VideoCore
// won't be received by the CPU because it reads an old cached value
// that resembles an error case instead.

// 0x00600000 as usize,
// 0x007FFFFF as usize,

pub fn dma_allocate(layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
    DMA_ALLOCATOR.lock(|a| a.allocate(layout))
}

pub fn dma_deallocate(ptr: NonNull<u8>, layout: Layout) {
    DMA_ALLOCATOR.lock(|a| unsafe { a.deallocate(ptr, layout) })
}

// Temporarily allocate out of DMA region until we have proper alloc arena
pub fn allocate_zeroed(layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
    DMA_ALLOCATOR.lock(|dma| dma.allocate_zeroed(layout))
}

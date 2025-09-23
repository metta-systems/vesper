#![no_std]
#![no_main]
#![allow(incomplete_features)]
#![feature(generic_const_exprs)] // incomplete_features
#![feature(format_args_nl)]
#![feature(int_roundings)]
#![feature(linkage)]
#![feature(step_trait)]
#![feature(trait_alias)]
#![feature(decl_macro)]
#![feature(allocator_api)]
#![feature(stmt_expr_attributes)]
#![feature(slice_ptr_get)]
#![feature(custom_test_frameworks)]
#![test_runner(libtest::test_runner)]
#![reexport_test_harness_main = "test_main"]
#![allow(clippy::upper_case_acronyms)]
#![allow(clippy::enum_variant_names)]
#![allow(clippy::nonstandard_macro_braces)] // https://github.com/shepmaster/snafu/issues/296
#![allow(missing_docs)] // Temp: switch to deny
#![deny(warnings)]
#![allow(linker_messages)]
#![allow(unused)]
#![allow(internal_features)]
#![feature(core_intrinsics)]
#![feature(ptr_internals)]

#[cfg(not(target_arch = "aarch64"))]
compile_error!("Architecture not supported, sorry!");

pub mod debug;
pub mod panic;

/// Version string.
pub fn version() -> &'static str {
    concat!(
        env!("CARGO_PKG_NAME"),
        " version ",
        env!("CARGO_PKG_VERSION")
    )
}

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
    use super::*;

    #[panic_handler]
    fn panicked(info: &core::panic::PanicInfo) -> ! {
        libtest::panic::handler_for_tests(info)
    }

    /// Main for running tests.
    #[unsafe(no_mangle)]
    pub unsafe fn main() -> ! {
        libexception::exception::handling_init();

        let phys_kernel_tables_base_addr = match unsafe { memory::mmu::kernel_map_binary() } {
            Err(string) => panic!("Error mapping kernel binary: {}", string),
            Ok(addr) => addr,
        };

        if let Err(e) = unsafe { memory::mmu::enable_mmu_and_caching(phys_kernel_tables_base_addr) }
        {
            panic!("Enabling MMU failed: {}", e);
        }

        memory::mmu::post_enable_init();
        platform::drivers::qemu_bring_up_console();

        test_main();

        libqemu::semihosting::exit_success()
    }
}

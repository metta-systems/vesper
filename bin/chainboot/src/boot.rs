use core::cell::UnsafeCell;
// Assembly counterpart to this file.
#[cfg(feature = "asm")]
core::arch::global_asm!(include_str!("boot.s"));

// This is quite impossible - the linker constants are resolved to fully constant offsets in asm
// version, but are image-relative symbols in rust, and I see no way to force it otherwise.
#[no_mangle]
#[link_section = ".text._start"]
#[cfg(not(feature = "asm"))]
#[optimize(size)]
pub unsafe extern "C" fn _start() -> ! {
    use {
        cortex_a::registers::{MPIDR_EL1, SP},
        machine::endless_sleep,
        tock_registers::interfaces::{Readable, Writeable},
    };
    extern "Rust" {
        // Stack top
        static __boot_core_stack_end_exclusive: UnsafeCell<()>;
    }

    // Set stack pointer first.
    SP.set(__boot_core_stack_end_exclusive.get() as u64);
    // const STACK_START: u64 = 0x0008_0000; // Keep in sync with linker script

    const CORE_0: u64 = 0;
    const CORE_MASK: u64 = 0x3;

    if CORE_0 == MPIDR_EL1.get() & CORE_MASK {
        prepare_and_relocate();
    }

    // if not core0, infinitely wait for events
    endless_sleep()
}

#[inline]
#[cfg(not(feature = "asm"))]
unsafe fn prepare_and_relocate() -> ! {
    // These are a problem, because they are not interpreted as constants here.
    // Subsequently, this code tries to read values from not-yet-existing data locations.
    extern "Rust" {
        // Boundaries of the .bss section, provided by the linker script
        static __bss_start: UnsafeCell<()>;
        static __bss_end_exclusive: UnsafeCell<()>;
        // Load address of the kernel binary
        static __binary_nonzero_lma: UnsafeCell<()>;
        // Address to relocate to and image size
        static __binary_nonzero_vma: UnsafeCell<()>;
        static __binary_nonzero_vma_end_exclusive: UnsafeCell<()>;
    }

    // Zeroes the .bss section
    r0::zero_bss(
        __bss_start.get() as *mut u64,
        __bss_end_exclusive.get() as *mut u64,
    );

    // Relocate the code
    core::ptr::copy_nonoverlapping(
        __binary_nonzero_lma.get() as *const u64,
        __binary_nonzero_vma.get() as *mut u64,
        __binary_nonzero_vma_end_exclusive.get() as usize - __binary_nonzero_vma.get() as usize,
    );

    let max_kernel_size = __binary_nonzero_vma.get() as u64 - __binary_nonzero_lma.get() as u64;
    _start_rust(max_kernel_size);
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

/// The Rust entry of the `kernel` binary.
///
/// The function is called from the assembly `_start` function, keep it to support "asm" feature.
#[no_mangle]
#[inline]
pub unsafe fn _start_rust(max_kernel_size: u64) -> ! {
    crate::kernel_init(max_kernel_size)
}

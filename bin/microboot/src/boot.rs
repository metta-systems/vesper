// Assembly counterpart to this file.
#[cfg(feature = "asm")]
core::arch::global_asm!(include_str!("boot.s"));

// This is quite impossible - the linker constants are resolved to fully constant offsets in asm
// version, but are image-relative symbols in rust, and I see no way to force it otherwise.
#[no_mangle]
#[link_section = ".text._start"]
#[cfg(not(feature = "asm"))]
pub unsafe extern "C" fn _start() -> ! {
    use {
        cortex_a::registers::{MPIDR_EL1, SP},
        machine::endless_sleep,
        tock_registers::interfaces::{Readable, Writeable},
    };

    const CORE_0: u64 = 0;
    const CORE_MASK: u64 = 0x3;

    if CORE_0 == MPIDR_EL1.get() & CORE_MASK {
        // if not core0, infinitely wait for events
        endless_sleep()
    }

    // These are a problem, because they are not interpreted as constants here.
    // Subsequently, this code tries to read values from not-yet-existing data locations.
    extern "C" {
        // Boundaries of the .bss section, provided by the linker script
        static mut __bss_start: u64;
        static mut __bss_end_exclusive: u64;
        // Load address of the kernel binary
        static mut __binary_nonzero_lma: u64;
        // Address to relocate to and image size
        static mut __binary_nonzero_vma: u64;
        static mut __binary_nonzero_vma_end_exclusive: u64;
        // Stack top
        static mut __boot_core_stack_end_exclusive: u64;
    }

    // Set stack pointer.
    SP.set(&mut __boot_core_stack_end_exclusive as *mut u64 as u64);

    // Zeroes the .bss section
    r0::zero_bss(&mut __bss_start, &mut __bss_end_exclusive);

    // Relocate the code
    core::ptr::copy_nonoverlapping(
        &mut __binary_nonzero_lma as *const u64,
        &mut __binary_nonzero_vma as *mut u64,
        (&mut __binary_nonzero_vma_end_exclusive as *mut u64 as u64
            - &mut __binary_nonzero_vma as *mut u64 as u64) as usize,
    );

    _start_rust();
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

/// The Rust entry of the `kernel` binary.
///
/// The function is called from the assembly `_start` function, keep it to support "asm" feature.
#[no_mangle]
#[inline(always)]
pub unsafe fn _start_rust(max_kernel_size: u64) -> ! {
    crate::kernel_init(max_kernel_size)
}

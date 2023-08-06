// Make first function small enough so that compiler doesn't try
// to crate a huge stack frame before we have a chance to set SP.
#[no_mangle]
#[link_section = ".text.chainboot.entry"]
pub unsafe extern "C" fn _start() -> ! {
    use {
        aarch64_cpu::registers::{MPIDR_EL1, SP},
        core::cell::UnsafeCell,
        machine::cpu::endless_sleep,
        tock_registers::interfaces::{Readable, Writeable},
    };

    const CORE_0: u64 = 0;
    const CORE_MASK: u64 = 0x3;

    if CORE_0 != MPIDR_EL1.get() & CORE_MASK {
        // if not core0, infinitely wait for events
        endless_sleep()
    }

    extern "Rust" {
        // Stack top
        static __boot_core_stack_end_exclusive: UnsafeCell<()>;
    }
    // Set stack pointer.
    SP.set(__boot_core_stack_end_exclusive.get() as u64);

    reset();
}

#[no_mangle]
#[link_section = ".text.chainboot"]
pub unsafe extern "C" fn reset() -> ! {
    use core::{
        cell::UnsafeCell,
        sync::{atomic, atomic::Ordering},
    };

    // These are a problem, because they are not interpreted as constants here.
    // Subsequently, this code tries to read values from not-yet-existing data locations.
    extern "Rust" {
        // Boundaries of the .bss section, provided by the linker script
        static __BSS_START: UnsafeCell<()>;
        static __BSS_SIZE_U64S: UnsafeCell<()>;
        // Load address of the kernel binary
        static __binary_nonzero_lma: UnsafeCell<()>;
        // Address to relocate to and image size
        static __binary_nonzero_vma: UnsafeCell<()>;
        static __binary_nonzero_vma_end_exclusive: UnsafeCell<()>;
        // Stack top
        static __boot_core_stack_end_exclusive: UnsafeCell<()>;
    }

    // This tries to call memcpy() at a wrong linked address - the function is in relocated area!

    // Relocate the code.
    // Emulate
    // core::ptr::copy_nonoverlapping(
    //     __binary_nonzero_lma.get() as *const u64,
    //     __binary_nonzero_vma.get() as *mut u64,
    //     __binary_nonzero_vma_end_exclusive.get() as usize - __binary_nonzero_vma.get() as usize,
    // );
    let binary_size =
        __binary_nonzero_vma_end_exclusive.get() as usize - __binary_nonzero_vma.get() as usize;
    local_memcpy(
        __binary_nonzero_vma.get() as *mut u8,
        __binary_nonzero_lma.get() as *const u8,
        binary_size,
    );

    // This tries to call memset() at a wrong linked address - the function is in relocated area!

    // Zeroes the .bss section
    // Emulate
    // crate::stdmem::local_memset(__bss_start.get() as *mut u8, 0u8, __bss_size.get() as usize);
    let bss = core::slice::from_raw_parts_mut(
        __BSS_START.get() as *mut u64,
        __BSS_SIZE_U64S.get() as usize,
    );
    for i in bss {
        *i = 0;
    }

    // Don't cross this line with loads and stores. The initializations
    // done above could be "invisible" to the compiler, because we write to the
    // same memory location that is used by statics after this point.
    // Additionally, we assume that no statics are accessed before this point.
    atomic::compiler_fence(Ordering::SeqCst);

    let max_kernel_size =
        __binary_nonzero_vma.get() as u64 - __boot_core_stack_end_exclusive.get() as u64;
    crate::kernel_init(max_kernel_size)
}

#[inline(always)]
#[link_section = ".text.chainboot"]
unsafe fn local_memcpy(mut dest: *mut u8, mut src: *const u8, n: usize) {
    let dest_end = dest.add(n);
    while dest < dest_end {
        *dest = *src;
        dest = dest.add(1);
        src = src.add(1);
    }
}

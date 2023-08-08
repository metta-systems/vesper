//! Platform memory Management.
//!
//! The physical memory layout.
//!
//! The Raspberry's firmware copies the kernel binary to 0x8_0000. The preceding region will be used
//! as the boot core's stack.
//!
//! +---------------------------------------+
//! |                                       | boot_core_stack_start @ 0x0
//! |                                       |                                ^
//! | Boot-core Stack                       |                                | stack
//! |                                       |                                | growth
//! |                                       |                                | direction
//! +---------------------------------------+
//! |                                       | code_start @ 0x8_0000 == boot_core_stack_end_exclusive
//! | .text                                 |
//! | .rodata                               |
//! | .got                                  |
//! |                                       |
//! +---------------------------------------+
//! |                                       | data_start == code_end_exclusive
//! | .data                                 |
//! | .bss                                  |
//! |                                       |
//! +---------------------------------------+
//! |                                       | data_end_exclusive
//! |                                       |
//!
//!
//!
//!
//!
//! The virtual memory layout is as follows:
//!
//! +---------------------------------------+
//! |                                       | boot_core_stack_start @ 0x0
//! |                                       |                                ^
//! | Boot-core Stack                       |                                | stack
//! |                                       |                                | growth
//! |                                       |                                | direction
//! +---------------------------------------+
//! |                                       | code_start @ 0x8_0000 == boot_core_stack_end_exclusive
//! | .text                                 |
//! | .rodata                               |
//! | .got                                  |
//! |                                       |
//! +---------------------------------------+
//! |                                       | data_start == code_end_exclusive
//! | .data                                 |
//! | .bss                                  |
//! |                                       |
//! +---------------------------------------+
//! |                                       |  mmio_remap_start == data_end_exclusive
//! | VA region for MMIO remapping          |
//! |                                       |
//! +---------------------------------------+
//! |                                       |  mmio_remap_end_exclusive
//! |                                       |
pub mod mmu;

//--------------------------------------------------------------------------------------------------
// Private Definitions
//--------------------------------------------------------------------------------------------------

use {
    crate::memory::{mmu::PageAddress, Address, Physical, Virtual},
    core::cell::UnsafeCell,
};

// Symbols from the linker script.
extern "Rust" {
    // Boot code.
    //
    // Using the linker script, we ensure that the boot area is consecutive and 4
    // KiB aligned, and we export the boundaries via symbols:
    //
    // [__BOOT_START, __BOOT_END)
    //
    // The inclusive start of the boot area, aka the address of the
    // first byte of the area.
    static __BOOT_START: UnsafeCell<()>;

    // The exclusive end of the boot area, aka the address of
    // the first byte _after_ the RO area.
    static __BOOT_END: UnsafeCell<()>;

    // Kernel code and RO data.
    //
    // Using the linker script, we ensure that the RO area is consecutive and 4
    // KiB aligned, and we export the boundaries via symbols:
    //
    // [__RO_START, __RO_END)
    //
    // The inclusive start of the read-only area, aka the address of the
    // first byte of the area.
    static __RO_START: UnsafeCell<()>;
    // The exclusive end of the read-only area, aka the address of
    // the first byte _after_ the RO area.
    static __RO_END: UnsafeCell<()>;
}

// Symbols from the linker script.
// extern "Rust" {
//     static __code_start: UnsafeCell<()>; // __RO_START
//     static __code_end_exclusive: UnsafeCell<()>; // __RO_END
//
//     static __data_start: UnsafeCell<()>;
//     static __data_end_exclusive: UnsafeCell<()>;
//
//     static __mmio_remap_start: UnsafeCell<()>;
//     static __mmio_remap_end_exclusive: UnsafeCell<()>;
//
//     static __boot_core_stack_start: UnsafeCell<()>;
//     static __boot_core_stack_end_exclusive: UnsafeCell<()>;
// }

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// The board's physical memory map.
/// This is a fixed memory map for Raspberry Pi,
/// @todo we need to infer the memory map from the provided DTB instead.
#[rustfmt::skip]
pub(super) mod map {
    use super::*;

    /// Beginning of memory.
    pub const START:                   usize =             0x0000_0000;
    /// End of memory - 8Gb RPi4
    pub const END_INCLUSIVE:           usize =             0x1_FFFF_FFFF;

    /// Physical RAM addresses.
    pub mod phys {
        /// Base address of video (VC) memory.
        pub const VIDEOMEM_BASE:       usize =             0x3e00_0000;
    }

    pub const VIDEOCORE_MBOX_OFFSET: usize = 0x0000_B880;
    pub const GPIO_OFFSET:           usize = 0x0020_0000;
    pub const UART_OFFSET:           usize = 0x0020_1000;
    pub const MINIUART_OFFSET:       usize = 0x0021_5000;

    /// Physical devices.
    #[cfg(feature = "rpi3")]
    pub mod mmio {
        use super::*;

        /// Base address of MMIO register range.
        pub const MMIO_BASE:           usize =             0x3F00_0000;

        /// Interrupt controller
        pub const PERIPHERAL_IC_BASE:  Address<Physical> = Address::new(MMIO_BASE + 0x0000_B200);
        pub const PERIPHERAL_IC_SIZE:  usize             =              0x24;

        /// Base address of ARM<->VC mailbox area.
        pub const VIDEOCORE_MBOX_BASE: Address<Physical> = Address::new(MMIO_BASE + VIDEOCORE_MBOX_OFFSET);

        /// Base address of GPIO registers.
        pub const GPIO_BASE:           Address<Physical> = Address::new(MMIO_BASE + GPIO_OFFSET);
        pub const GPIO_SIZE:           usize             =              0xA0;

        pub const PL011_UART_BASE:     Address<Physical> = Address::new(MMIO_BASE + UART_OFFSET);
        pub const PL011_UART_SIZE:     usize             =              0x48;

        /// Base address of MiniUART.
        pub const MINI_UART_BASE:      Address<Physical> = Address::new(MMIO_BASE + MINIUART_OFFSET);

        /// End of MMIO memory region.
        pub const END:                 Address<Physical> = Address::new(0x4001_0000);
    }

    /// Physical devices.
    #[cfg(feature = "rpi4")]
    pub mod mmio {
        use super::*;

        /// Base address of MMIO register range.
        pub const MMIO_BASE:        usize =             0xFE00_0000;

        /// Base address of GPIO registers.
        pub const GPIO_BASE:        Address<Physical> = Address::new(MMIO_BASE + GPIO_OFFSET);
        pub const GPIO_SIZE:        usize             =              0xA0;

        /// Base address of regular UART.
        pub const PL011_UART_BASE:  Address<Physical> = Address::new(MMIO_BASE + UART_OFFSET);
        pub const PL011_UART_SIZE:  usize             =              0x48;

        /// Base address of MiniUART.
        pub const MINI_UART_BASE:   Address<Physical> = Address::new(MMIO_BASE + MINIUART_OFFSET);

        /// Interrupt controller
        pub const GICD_BASE:        Address<Physical> = Address::new(0xFF84_1000);
        pub const GICD_SIZE:        usize             =              0x824;

        pub const GICC_BASE:        Address<Physical> = Address::new(0xFF84_2000);
        pub const GICC_SIZE:        usize             =              0x14;

        /// Base address of ARM<->VC mailbox area.
        pub const VIDEOCORE_MBOX_BASE: usize = MMIO_BASE + VIDEOCORE_MBOX_OFFSET;

        /// End of MMIO memory region.
        pub const END:              Address<Physical> = Address::new(0xFF85_0000);
    }

    ///  End address of mapped memory.
    pub const END: Address<Physical> = mmio::END;

    //----
    // Unused?
    //----

    /// Virtual (mapped) addresses.
    pub mod virt {
        /// Start (top) of kernel stack.
        pub const KERN_STACK_START:    usize =             super::START;
        /// End (bottom) of kernel stack. SP starts at KERN_STACK_END + 1.
        pub const KERN_STACK_END:      usize =             0x0007_FFFF;

        /// Location of DMA-able memory region (in the second 2 MiB block).
        pub const DMA_HEAP_START:      usize =             0x0020_0000;
        /// End of DMA-able memory region.
        pub const DMA_HEAP_END:        usize =             0x005F_FFFF;
    }
}

//--------------------------------------------------------------------------------------------------
// Private Code
//--------------------------------------------------------------------------------------------------

/// Start page address of the boot segment.
///
/// # Safety
///
/// - Value is provided by the linker script and must be trusted as-is.
#[inline(always)]
fn boot_start() -> usize {
    unsafe { __BOOT_START.get() as usize }
}

/// Exclusive end page address of the boot segment.
/// # Safety
///
/// - Value is provided by the linker script and must be trusted as-is.
#[inline(always)]
fn boot_end_exclusive() -> usize {
    unsafe { __BOOT_END.get() as usize }
}

/// Start page address of the code segment.
///
/// # Safety
///
/// - Value is provided by the linker script and must be trusted as-is.
#[inline(always)]
fn code_start() -> usize {
    unsafe { __RO_START.get() as usize }
}

/// Start page address of the code segment.
///
/// # Safety
///
/// - Value is provided by the linker script and must be trusted as-is.
#[inline(always)]
fn virt_code_start() -> PageAddress<Virtual> {
    PageAddress::from(unsafe { __RO_START.get() as usize })
}

/// Size of the code segment.
///
/// # Safety
///
/// - Value is provided by the linker script and must be trusted as-is.
#[inline(always)]
fn code_size() -> usize {
    unsafe { (__RO_END.get() as usize) - (__RO_START.get() as usize) }
}

/// Exclusive end page address of the code segment.
/// # Safety
///
/// - Value is provided by the linker script and must be trusted as-is.
// #[inline(always)]
// fn code_end_exclusive() -> usize {
//     unsafe { __RO_END.get() as usize }
// }

/// Start page address of the data segment.
#[inline(always)]
fn virt_data_start() -> PageAddress<Virtual> {
    PageAddress::from(unsafe { __data_start.get() as usize })
}

/// Size of the data segment.
///
/// # Safety
///
/// - Value is provided by the linker script and must be trusted as-is.
#[inline(always)]
fn data_size() -> usize {
    unsafe { (__data_end_exclusive.get() as usize) - (__data_start.get() as usize) }
}

/// Start page address of the MMIO remap reservation.
///
/// # Safety
///
/// - Value is provided by the linker script and must be trusted as-is.
#[inline(always)]
fn virt_mmio_remap_start() -> PageAddress<Virtual> {
    PageAddress::from(unsafe { __mmio_remap_start.get() as usize })
}

/// Size of the MMIO remap reservation.
///
/// # Safety
///
/// - Value is provided by the linker script and must be trusted as-is.
#[inline(always)]
fn mmio_remap_size() -> usize {
    unsafe { (__mmio_remap_end_exclusive.get() as usize) - (__mmio_remap_start.get() as usize) }
}

/// Start page address of the boot core's stack.
#[inline(always)]
fn virt_boot_core_stack_start() -> PageAddress<Virtual> {
    PageAddress::from(unsafe { __boot_core_stack_start.get() as usize })
}

/// Size of the boot core's stack.
#[inline(always)]
fn boot_core_stack_size() -> usize {
    unsafe {
        (__boot_core_stack_end_exclusive.get() as usize) - (__boot_core_stack_start.get() as usize)
    }
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

/// Exclusive end address of the physical address space.
#[inline(always)]
pub fn phys_addr_space_end_exclusive_addr() -> PageAddress<Physical> {
    PageAddress::from(map::END)
}

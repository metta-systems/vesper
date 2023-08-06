use core::cell::UnsafeCell;

pub mod mmu;

//--------------------------------------------------------------------------------------------------
// Private Definitions
//--------------------------------------------------------------------------------------------------

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

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// System memory map.
/// This is a fixed memory map for Raspberry Pi,
/// @todo we need to infer the memory map from the provided DTB.
#[rustfmt::skip]
pub mod map { // @todo only pub(super) for proper isolation!
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

    /// Memory-mapped devices.
    #[cfg(feature = "rpi3")]
    pub mod mmio {
        use super::*;

        /// Base address of MMIO register range.
        pub const MMIO_BASE:           usize =             0x3F00_0000;
        /// Base address of ARM<->VC mailbox area.
        pub const VIDEOCORE_MBOX_BASE: usize = MMIO_BASE + VIDEOCORE_MBOX_OFFSET;
        /// Base address of GPIO registers.
        pub const GPIO_BASE:           usize = MMIO_BASE + GPIO_OFFSET;
        /// Base address of regular UART.
        pub const PL011_UART_BASE:     usize = MMIO_BASE + UART_OFFSET;
        /// Base address of MiniUART.
        pub const MINI_UART_BASE:      usize = MMIO_BASE + MINIUART_OFFSET;
        /// Interrupt controller
        pub const PERIPHERAL_IC_START: usize = MMIO_BASE + 0x0000_B200;
        /// End of MMIO memory.
        pub const MMIO_END:            usize =             super::END_INCLUSIVE;
    }

    /// Memory-mapped devices.
    #[cfg(feature = "rpi4")]
    pub mod mmio {
        use super::*;

        /// Base address of MMIO register range.
        pub const MMIO_BASE:           usize =             0xFE00_0000;
        /// Base address of ARM<->VC mailbox area.
        pub const VIDEOCORE_MBOX_BASE: usize = MMIO_BASE + VIDEOCORE_MBOX_OFFSET;
        /// Base address of GPIO registers.
        pub const GPIO_BASE:           usize = MMIO_BASE + GPIO_OFFSET;
        /// Base address of regular UART.
        pub const PL011_UART_BASE:     usize = MMIO_BASE + UART_OFFSET;
        /// Base address of MiniUART.
        pub const MINI_UART_BASE:      usize = MMIO_BASE + MINIUART_OFFSET;
        /// Interrupt controller
        pub const GICD_START:          usize =             0xFF84_1000;
        pub const GICC_START:          usize =             0xFF84_2000;
        /// End of MMIO memory.
        pub const MMIO_END:            usize =             super::END_INCLUSIVE;
    }

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

/// Exclusive end page address of the code segment.
/// # Safety
///
/// - Value is provided by the linker script and must be trusted as-is.
#[inline(always)]
fn code_end_exclusive() -> usize {
    unsafe { __RO_END.get() as usize }
}

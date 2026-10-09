//! Platform memory Management.
//!
//! The physical memory layout.
//!
//! The Raspberry's firmware copies the kernel binary to `0x8_0000`. The preceding region will be used
//! as the boot core's stack.
//!

use {
    libaddress::PhysAddr,
    // libmapping::{MemoryRegion, PageAddress},
    // libmemory::{AddressSpace, AssociatedTranslationTable, TranslationGranule},
};

//--------------------------------------------------------------------------------------------------
// Private Definitions
//--------------------------------------------------------------------------------------------------

// type KernelTranslationTable =
//     <KernelVirtAddrSpace as AssociatedTranslationTable>::TableStartFromBottom;

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

// The translation granule chosen by this platform. This will be used everywhere else
// in the kernel to derive respective data structures and their sizes.
// For example, the `crate::memory::mmu::Page`.
// pub type KernelGranule = TranslationGranule<{ 64 * 1024 }>;

// The kernel's virtual address space defined by this platform.
// pub type KernelVirtAddrSpace = AddressSpace<{ 1024 * 1024 * 1024 }>;

/// The board's physical memory map.
/// This is a fixed memory map for Raspberry Pi,
/// @todo we need to infer the memory map from the provided DTB instead.
#[rustfmt::skip]
pub mod map {
    use super::*;

    /// Beginning of memory.
    pub const START:                   u64 =             0x0000_0000;
    /// End of memory - 8Gb `RPi4`
    pub const END_INCLUSIVE:           u64 =             0x1_FFFF_FFFF;

    /// Physical RAM addresses.
    pub mod phys {
        /// Base address of video (VC) memory.
        pub const VIDEOMEM_BASE:       u64 =             0x3e00_0000;
    }

    pub const VIDEOCORE_MBOX_OFFSET: u64 = 0x0000_B880;
    pub const POWER_OFFSET:          u64 = 0x0010_0000;
    pub const GPIO_OFFSET:           u64 = 0x0020_0000;
    pub const UART_OFFSET:           u64 = 0x0020_1000;
    pub const MINIUART_OFFSET:       u64 = 0x0021_5000;

    /// Physical devices.
    #[cfg(board_rpi3)]
    pub mod mmio {
        use super::*;

        /// Base address of MMIO register range.
        pub const MMIO_BASE:           u64 =             0x3F00_0000;

        /// Interrupt controller
        pub const PERIPHERAL_IC_BASE:  PhysAddr = PhysAddr::new(MMIO_BASE + 0x0000_B200);
        pub const PERIPHERAL_IC_SIZE:  usize             =              0x24;

        /// Base address of ARM<->VC mailbox area.
        pub const VIDEOCORE_MBOX_BASE: PhysAddr = PhysAddr::new(MMIO_BASE + VIDEOCORE_MBOX_OFFSET);

        /// Board power control.
        pub const POWER_BASE:          PhysAddr = PhysAddr::new(MMIO_BASE + POWER_OFFSET);

        /// Base address of GPIO registers.
        pub const GPIO_BASE:           PhysAddr = PhysAddr::new(MMIO_BASE + GPIO_OFFSET);
        pub const GPIO_SIZE:           usize             =              0xA0;

        pub const PL011_UART_BASE:     PhysAddr = PhysAddr::new(MMIO_BASE + UART_OFFSET);
        pub const PL011_UART_SIZE:     usize             =              0x48;

        /// Base address of `MiniUART`.
        pub const MINI_UART_BASE:      PhysAddr = PhysAddr::new(MMIO_BASE + MINIUART_OFFSET);

        /// End of MMIO memory region.
        pub const END:                 PhysAddr = PhysAddr::new(0x4001_0000);
    }

    /// Physical devices.
    #[cfg(board_rpi4)]
    pub mod mmio {
        use super::*;

        /// Base address of MMIO register range.
        pub const MMIO_BASE:        u64 =             0xFE00_0000;

        /// Base address of GPIO registers.
        pub const GPIO_BASE:        PhysAddr = PhysAddr::new(MMIO_BASE + GPIO_OFFSET);
        pub const GPIO_SIZE:        usize             =              0xA0;

        /// Base address of regular UART.
        pub const PL011_UART_BASE:  PhysAddr = PhysAddr::new(MMIO_BASE + UART_OFFSET);
        pub const PL011_UART_SIZE:  usize             =              0x48;

        /// Base address of `MiniUART`.
        pub const MINI_UART_BASE:   PhysAddr = PhysAddr::new(MMIO_BASE + MINIUART_OFFSET);

        /// Interrupt controller
        pub const GICD_BASE:        PhysAddr = PhysAddr::new(0xFF84_1000);
        pub const GICD_SIZE:        usize             =              0x824;

        pub const GICC_BASE:        PhysAddr = PhysAddr::new(0xFF84_2000);
        pub const GICC_SIZE:        usize             =              0x14;

        /// Base address of ARM<->VC mailbox area.
        pub const VIDEOCORE_MBOX_BASE: u64 = MMIO_BASE + VIDEOCORE_MBOX_OFFSET;

        /// End of MMIO memory region.
        pub const END:              PhysAddr = PhysAddr::new(0xFF85_0000);
    }

    #[cfg(not(any(board_rpi3, board_rpi4)))]
    compile_error!("No platform selected - specify TARGET_BOARD in configuration");

    ///  End address of mapped memory.
    pub const END: PhysAddr = mmio::END;

    //----
    // Unused?
    //----

    /// Virtual (mapped) addresses.
    pub mod virt {
        /// Start (top) of kernel stack.
        pub const KERN_STACK_START:    u64 =             super::START;
        /// End (bottom) of kernel stack. SP starts at `KERN_STACK_END` + 1.
        pub const KERN_STACK_END:      u64 =             0x0007_FFFF;

        /// Location of DMA-able memory region (in the second 2 MiB block).
        pub const DMA_HEAP_START:      u64 =             0x0020_0000;
        /// End of DMA-able memory region.
        pub const DMA_HEAP_END:        u64 =             0x005F_FFFF;
    }
}

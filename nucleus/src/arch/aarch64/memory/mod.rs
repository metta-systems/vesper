/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Memory management functions for aarch64.

use {
    crate::println,
    core::{fmt, ops::RangeInclusive},
};

mod addr;
// pub mod mmu;
mod features;
mod page_size;
mod phys_frame;
mod virt_page;

pub mod mmu2;
pub use mmu2::*;

// mod area_frame_allocator;
// pub use self::area_frame_allocator::AreaFrameAllocator;
// mod boot_allocator; // Hands out physical memory obtained from devtree
// use self::paging::PAGE_SIZE;

pub use addr::PhysAddr;
pub use addr::VirtAddr;
pub use page_size::PageSize;
pub use phys_frame::PhysFrame;

use mmu_experimental::PhysFrame;

// @todo ??
pub trait FrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame>; // @todo Result<>
    fn deallocate_frame(&mut self, frame: PhysFrame);
}

// Identity-map things for now.
//
// aarch64 granules and page sizes howto:
// https://stackoverflow.com/questions/34269185/simultaneous-existence-of-different-sized-pages-on-aarch64

/// Default page size used by the kernel.
pub const PAGE_SIZE: usize = 4096;

/// System memory map.
/// This is a fixed memory map for RasPi3,
/// @todo we need to infer the memory map from the provided DTB.
#[rustfmt::skip]
pub mod map {
    /// Beginning of memory.
    pub const START:                   usize =             0x0000_0000;
    /// End of memory.
    pub const END:                     usize =             0x3FFF_FFFF;

    /// Physical RAM addresses.
    pub mod phys {
        /// Base address of video (VC) memory.
        pub const VIDEOMEM_BASE:       usize =             0x3e00_0000;
        /// Base address of MMIO register range.
        pub const MMIO_BASE:           usize =             0x3F00_0000;
        /// Base address of ARM<->VC mailbox area.
        pub const VIDEOCORE_MBOX_BASE: usize = MMIO_BASE + 0x0000_B880;
        /// Base address of GPIO registers.
        pub const GPIO_BASE:           usize = MMIO_BASE + 0x0020_0000;
        /// Base address of regular UART.
        pub const PL011_UART_BASE:     usize = MMIO_BASE + 0x0020_1000;
        /// Base address of MiniUART.
        pub const MINI_UART_BASE:      usize = MMIO_BASE + 0x0021_5000;
        /// End of MMIO memory.
        pub const MMIO_END:            usize =             super::END;
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

/// Types used for compiling the virtual memory layout of the kernel using address ranges.
pub mod kernel_mem_range {
    use core::ops::RangeInclusive;

    /// Memory region attributes.
    #[derive(Copy, Clone)]
    pub enum MemAttributes {
        /// Regular memory
        CacheableDRAM,
        /// Memory without caching
        NonCacheableDRAM,
        /// Device memory
        Device,
    }

    /// Memory region access permissions.
    #[derive(Copy, Clone)]
    pub enum AccessPermissions {
        /// Read-only access
        ReadOnly,
        /// Read-write access
        ReadWrite,
    }

    /// Memory region translation.
    #[allow(dead_code)]
    #[derive(Copy, Clone)]
    pub enum Translation {
        /// One-to-one address mapping
        Identity,
        /// Mapping with a specified offset
        Offset(usize),
    }

    /// Summary structure of memory region properties.
    #[derive(Copy, Clone)]
    pub struct AttributeFields {
        /// Attributes
        pub mem_attributes: MemAttributes,
        /// Permissions
        pub acc_perms: AccessPermissions,
        /// Disable executable code in this region
        pub execute_never: bool,
    }

    impl Default for AttributeFields {
        fn default() -> AttributeFields {
            AttributeFields {
                mem_attributes: MemAttributes::CacheableDRAM,
                acc_perms: AccessPermissions::ReadWrite,
                execute_never: true,
            }
        }
    }

    /// Memory region descriptor.
    ///
    /// Used to construct iterable kernel memory ranges.
    pub struct Descriptor {
        /// Name of the region
        pub name: &'static str,
        /// Virtual memory range
        pub virtual_range: fn() -> RangeInclusive<usize>,
        /// Mapping translation
        pub translation: Translation,
        /// Attributes
        pub attribute_fields: AttributeFields,
    }
}

pub use kernel_mem_range::*;

/// A virtual memory layout that is agnostic of the paging granularity that the
/// hardware MMU will use.
///
/// Contains only special ranges, aka anything that is _not_ normal cacheable
/// DRAM.
static KERNEL_VIRTUAL_LAYOUT: [Descriptor; 6] = [
    Descriptor {
        name: "Kernel stack",
        virtual_range: || {
            RangeInclusive::new(map::virt::KERN_STACK_START, map::virt::KERN_STACK_END)
        },
        translation: Translation::Identity,
        attribute_fields: AttributeFields {
            mem_attributes: MemAttributes::CacheableDRAM,
            acc_perms: AccessPermissions::ReadWrite,
            execute_never: true,
        },
    },
    Descriptor {
        name: "Boot code and data",
        virtual_range: || {
            // Using the linker script, we ensure that the boot area is consecutive and 4
            // KiB aligned, and we export the boundaries via symbols:
            //
            // [__BOOT_START, __BOOT_END)
            extern "C" {
                // The inclusive start of the boot area, aka the address of the
                // first byte of the area.
                static __BOOT_START: u64;

                // The exclusive end of the boot area, aka the address of
                // the first byte _after_ the RO area.
                static __BOOT_END: u64;
            }

            unsafe {
                // Notice the subtraction to turn the exclusive end into an
                // inclusive end
                RangeInclusive::new(
                    &__BOOT_START as *const _ as usize,
                    &__BOOT_END as *const _ as usize - 1,
                )
            }
        },
        translation: Translation::Identity,
        attribute_fields: AttributeFields {
            mem_attributes: MemAttributes::CacheableDRAM,
            acc_perms: AccessPermissions::ReadOnly,
            execute_never: false,
        },
    },
    Descriptor {
        name: "Kernel code and RO data",
        virtual_range: || {
            // Using the linker script, we ensure that the RO area is consecutive and 4
            // KiB aligned, and we export the boundaries via symbols:
            //
            // [__RO_START, __RO_END)
            extern "C" {
                // The inclusive start of the read-only area, aka the address of the
                // first byte of the area.
                static __RO_START: u64;

                // The exclusive end of the read-only area, aka the address of
                // the first byte _after_ the RO area.
                static __RO_END: u64;
            }

            unsafe {
                // Notice the subtraction to turn the exclusive end into an
                // inclusive end
                RangeInclusive::new(
                    &__RO_START as *const _ as usize,
                    &__RO_END as *const _ as usize - 1,
                )
            }
        },
        translation: Translation::Identity,
        attribute_fields: AttributeFields {
            mem_attributes: MemAttributes::CacheableDRAM,
            acc_perms: AccessPermissions::ReadOnly,
            execute_never: false,
        },
    },
    Descriptor {
        name: "Kernel data and BSS",
        virtual_range: || {
            extern "C" {
                static __DATA_START: u64;
                static __BSS_END: u64;
            }

            unsafe {
                RangeInclusive::new(
                    &__DATA_START as *const _ as usize,
                    &__BSS_END as *const _ as usize - 1,
                )
            }
        },
        translation: Translation::Identity,
        attribute_fields: AttributeFields {
            mem_attributes: MemAttributes::CacheableDRAM,
            acc_perms: AccessPermissions::ReadWrite,
            execute_never: true,
        },
    },
    // @todo these should come from DTB and mem-map?
    Descriptor {
        name: "DMA heap pool",
        virtual_range: || RangeInclusive::new(map::virt::DMA_HEAP_START, map::virt::DMA_HEAP_END),
        translation: Translation::Identity,
        attribute_fields: AttributeFields {
            mem_attributes: MemAttributes::NonCacheableDRAM,
            acc_perms: AccessPermissions::ReadWrite,
            execute_never: true,
        },
    },
    // @todo these should come from DTB and mem-map?
    Descriptor {
        name: "Device MMIO",
        virtual_range: || RangeInclusive::new(map::phys::VIDEOMEM_BASE, map::phys::MMIO_END),
        translation: Translation::Identity,
        attribute_fields: AttributeFields {
            mem_attributes: MemAttributes::Device,
            acc_perms: AccessPermissions::ReadWrite,
            execute_never: true,
        },
    },
];

/// For a given virtual address, find and return the output address and
/// according attributes.
///
/// If the address is not covered in VIRTUAL_LAYOUT, return a default for normal
/// cacheable DRAM.
pub fn get_virt_addr_properties(
    virt_addr: usize,
) -> Result<(usize, AttributeFields), &'static str> {
    if virt_addr > map::END {
        return Err("Address out of range.");
    }

    for i in KERNEL_VIRTUAL_LAYOUT.iter() {
        if (i.virtual_range)().contains(&virt_addr) {
            let output_addr = match i.translation {
                Translation::Identity => virt_addr,
                Translation::Offset(a) => a + (virt_addr - (i.virtual_range)().start()),
            };

            return Ok((output_addr, i.attribute_fields));
        }
    }

    Ok((virt_addr, AttributeFields::default()))
}

/// Human-readable output of a Descriptor.
impl fmt::Display for Descriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Call the function to which self.range points, and dereference the
        // result, which causes Rust to copy the value.
        let start = *(self.virtual_range)().start();
        let end = *(self.virtual_range)().end();
        let size = end - start + 1;

        // log2(1024)
        const KIB_RSHIFT: u32 = 10;

        // log2(1024 * 1024)
        const MIB_RSHIFT: u32 = 20;

        let (size, unit) = if (size >> MIB_RSHIFT) > 0 {
            (size >> MIB_RSHIFT, "MiB")
        } else if (size >> KIB_RSHIFT) > 0 {
            (size >> KIB_RSHIFT, "KiB")
        } else {
            (size, "Byte")
        };

        let attr = match self.attribute_fields.mem_attributes {
            MemAttributes::CacheableDRAM => "C",
            MemAttributes::NonCacheableDRAM => "NC",
            MemAttributes::Device => "Dev",
        };

        let acc_p = match self.attribute_fields.acc_perms {
            AccessPermissions::ReadOnly => "RO",
            AccessPermissions::ReadWrite => "RW",
        };

        let xn = if self.attribute_fields.execute_never {
            "PXN"
        } else {
            "PX"
        };

        write!(
            f,
            "      {:#010X} - {:#010X} | {: >3} {} | {: <3} {} {: <3} | {}",
            start, end, size, unit, attr, acc_p, xn, self.name
        )
    }
}

/// Print the kernel memory layout.
pub fn print_layout() {
    println!("[i] Kernel memory layout:");

    for i in KERNEL_VIRTUAL_LAYOUT.iter() {
        println!("{}", i);
    }
}

// Boot allocator, Section mapping, memory permissions

use {
    crate::{boot_info::BOOT_INFO, loader::LoadableSection},
    libaddress::{PhysAddr, VirtAddr},
    liblocking::interface::Mutex,
    libmapping::AttributeFields,
};

// Memory region translation.
// #[allow(dead_code)]
// #[derive(Copy, Clone)]
// pub enum Translation {
//     /// One-to-one address mapping
//     Identity,
//     /// Mapping with a specified offset
//     Offset(usize),
// }

pub struct BootAllocator {
    current: PhysAddr,
    end: PhysAddr,
}

#[derive(PartialEq, Copy, Clone)]
pub enum Alloc {
    /// This allocation should be entered into mappings and stay around after `init_thread` finishes
    Persistent,
    /// This allocation will perish and be added to Untypeds after `init_thread` finishes
    Droppable,
}

impl BootAllocator {
    pub fn new(start: PhysAddr, size: usize) -> Self {
        Self {
            current: start,
            end: PhysAddr::new(start.as_u64() + size as u64),
        }
    }

    pub fn alloc_pages(&mut self, count: usize, usage: (&'static str, Alloc)) -> Option<PhysAddr> {
        self.alloc_aligned(count * 4096, 4096, usage)
    }

    pub fn alloc_aligned(
        &mut self,
        size: usize,
        align: usize,
        usage: (&'static str, Alloc),
    ) -> Option<PhysAddr> {
        let aligned = self.current.aligned_up(align as u64);
        let new_current = PhysAddr::new(aligned.as_u64() + size as u64);

        #[cfg(qemu)]
        libqemu::semi_println!(
            "alloc_aligned {:#016x} => {:#016x} (wrt {:#016x})",
            aligned.as_u64(),
            new_current.as_u64(),
            self.end.as_u64()
        );

        if !usage.0.is_empty() {
            BOOT_INFO.lock(|bi| {
                bi.insert_used_region(
                    aligned,
                    new_current,
                    AttributeFields {
                        droppable: usage.1 == Alloc::Droppable,
                        ..Default::default() // TODO: better RWX flags
                    },
                    usage.0,
                );
            });
        }

        if new_current > self.end {
            return None;
        }
        self.current = new_current;
        Some(aligned)
    }

    pub fn current(&self) -> PhysAddr {
        self.current
    }
    pub fn end(&self) -> PhysAddr {
        self.end
    }
    pub fn remaining(&self) -> usize {
        self.end - self.current
    }
}

/// Memory protection flags
#[derive(Debug, Clone, Copy)]
pub struct MemoryPermissions {
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
}

impl core::fmt::Display for MemoryPermissions {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{}{}{}",
            if self.readable { "R" } else { "-" },
            if self.writable { "W" } else { "-" },
            if self.executable { "X" } else { "-" }
        )
    }
}

impl MemoryPermissions {
    /// Convert to `AArch64` page table flags
    pub const fn as_pte_flags(self) -> u64 {
        let mut flags = 0_u64;

        // Access flag (must be set)
        flags |= 1 << 10; // AF

        // Shareability (inner shareable for normal memory)
        flags |= 0b11 << 8; // SH

        // Access permissions
        if !self.writable {
            flags |= 0b10 << 6; // AP[2:1] = read-only
        }

        // Execute never flags
        if !self.executable {
            flags |= 1 << 53; // PXN
            flags |= 1 << 54; // UXN
        }

        flags
    }
}

/// Layout of the loaded kernel in physical memory
#[derive(Debug)]
pub struct KernelLayout {
    /// Physical address where kernel is loaded
    pub phys_base: PhysAddr,
    /// Virtual base address (higher-half)
    pub virt_base: VirtAddr,
    /// Total size of kernel in physical memory
    pub total_size: usize,
    /// Section metadata (for page table setup)
    pub sections: &'static [LoadableSection],
    /// BSS physical address (for zeroing)
    pub bss_phys: PhysAddr,
    /// BSS virtual address (for kernel mapping)
    pub bss_virt: VirtAddr,
    /// BSS size
    pub bss_size: usize,
    /// Stack virtual address (for kernel mapping)
    pub stack_virt_bottom: VirtAddr,
    /// Exception vector table virtual address (for `VBAR_EL1`)
    pub vectors_virt: VirtAddr,
}

impl KernelLayout {
    pub fn virt_to_phys(&self, virt: VirtAddr) -> PhysAddr {
        let offset = virt.as_u64() - self.virt_base.as_u64();
        PhysAddr::new(self.phys_base.as_u64() + offset)
    }

    /// Get the `VBAR_EL1` value (virtual address for use after MMU enable)
    ///
    /// This is what the kernel would set `VBAR_EL1` to after switching to
    /// higher-half addresses.
    pub fn vbar_el1_virt(&self) -> u64 {
        assert!(
            self.vectors_virt.as_u64().trailing_zeros() >= 11,
            "VBAR_EL1 address 0x{:016X} must be 2KB aligned",
            self.vectors_virt.as_u64()
        );
        self.vectors_virt.as_u64()
    }

    pub fn iter_sections(&self) -> impl Iterator<Item = SectionMapping> + '_ {
        self.sections.iter().map(move |section| {
            let offset = section.meta.offset_from_base(self.virt_base.as_u64());
            SectionMapping {
                name: section.meta.name,
                phys_start: PhysAddr::new(self.phys_base.as_u64() + offset),
                virt_start: VirtAddr::new(section.meta.virt_addr),
                size: section.meta.size,
                permissions: section.meta.permissions,
            }
        })
    }

    pub fn bss_mapping(&self) -> Option<SectionMapping> {
        (self.bss_size > 0).then_some(SectionMapping {
            name: ".bss",
            phys_start: self.bss_phys,
            virt_start: self.bss_virt,
            size: self.bss_size,
            permissions: MemoryPermissions {
                readable: true,
                writable: true,
                executable: false,
            },
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SectionMapping {
    pub name: &'static str,
    pub phys_start: PhysAddr,
    pub virt_start: VirtAddr,
    pub size: usize,
    pub permissions: MemoryPermissions,
}

impl SectionMapping {
    pub fn page_count(&self) -> usize {
        self.size.div_ceil(0x1000)
    }

    pub fn pages(&self) -> impl Iterator<Item = (PhysAddr, VirtAddr)> {
        let phys_start = self.phys_start.as_u64();
        let virt_start = self.virt_start.as_u64();
        let count = self.page_count();
        (0..count).map(move |i| {
            let offset = (i * 0x1000) as u64;
            (
                PhysAddr::new(phys_start + offset),
                VirtAddr::new(virt_start + offset),
            )
        })
    }
}

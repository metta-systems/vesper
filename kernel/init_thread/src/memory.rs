// Boot allocator, Section mapping, memory permissions

use crate::loader::LoadableSection;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct PhysAddr(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct VirtAddr(pub u64);

impl PhysAddr {
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }
    pub const fn as_u64(self) -> u64 {
        self.0
    }
    pub const fn as_ptr<T>(self) -> *const T {
        self.0 as *const T
    }
    pub fn as_mut_ptr<T>(self) -> *mut T {
        self.0 as *mut T
    }
    pub const fn add(self, offset: u64) -> Self {
        Self(self.0 + offset)
    }
    pub const fn align_up(self, align: u64) -> Self {
        Self((self.0 + align - 1) & !(align - 1))
    }
    pub const fn align_down(self, align: u64) -> Self {
        Self(self.0 & !(align - 1))
    }
    pub const fn is_aligned(self, align: u64) -> bool {
        self.0 & (align - 1) == 0
    }
}

impl VirtAddr {
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }
    pub const fn as_u64(self) -> u64 {
        self.0
    }
    pub const fn add(self, offset: u64) -> Self {
        Self(self.0 + offset)
    }
    pub const fn sub(self, other: VirtAddr) -> u64 {
        self.0 - other.0
    }
    pub const fn is_higher_half(self) -> bool {
        self.0 >= 0xFFFF_0000_0000_0000
    }
    pub const fn is_aligned(self, align: u64) -> bool {
        self.0 & (align - 1) == 0
    }
}

pub struct BootAllocator {
    current: PhysAddr,
    end: PhysAddr,
}

impl BootAllocator {
    pub const fn new(start: PhysAddr, size: usize) -> Self {
        Self {
            current: start,
            end: PhysAddr(start.0 + size as u64),
        }
    }

    pub fn alloc_pages(&mut self, count: usize) -> Option<PhysAddr> {
        self.alloc_aligned(count * 4096, 4096)
    }

    pub fn alloc_aligned(&mut self, size: usize, align: usize) -> Option<PhysAddr> {
        let aligned = self.current.align_up(align as u64);
        let new_current = PhysAddr(aligned.0 + size as u64);

        libqemu::semi_println!(
            "alloc_aligned {:#016x} => {:#016x} (wrt {:#016x})",
            aligned.0,
            new_current.0,
            self.end.0
        );

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
        (self.end.0 - self.current.0) as usize
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
    /// Convert to AArch64 page table flags
    pub const fn as_pte_flags(&self) -> u64 {
        let mut flags = 0u64;

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
    /// BSS physical address
    pub bss_phys: PhysAddr,
    /// BSS virtual address
    pub bss_virt: VirtAddr,
    /// BSS size
    pub bss_size: usize,
    /// Exception vector table physical address (for VBAR_EL1)
    pub vectors_phys: PhysAddr,
    /// Exception vector table virtual address
    pub vectors_virt: VirtAddr,
}

impl KernelLayout {
    pub fn virt_to_phys(&self, virt: VirtAddr) -> PhysAddr {
        let offset = virt.as_u64() - self.virt_base.as_u64();
        PhysAddr(self.phys_base.as_u64() + offset)
    }

    /// Get the VBAR_EL1 value (virtual address for use after MMU enable)
    ///
    /// This is what the kernel would set VBAR_EL1 to after switching to
    /// higher-half addresses.
    pub fn vbar_el1_virt(&self) -> u64 {
        assert!(
            self.vectors_virt.as_u64() & 0x7FF == 0,
            "VBAR_EL1 address 0x{:016X} must be 2KB aligned",
            self.vectors_virt.0
        );
        self.vectors_virt.0
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
        if self.bss_size > 0 {
            Some(SectionMapping {
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
        } else {
            None
        }
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

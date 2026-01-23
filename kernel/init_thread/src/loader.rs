// init_thread/src/loader.rs

use {
    crate::{
        embed::KERNEL,
        memory::{BootAllocator, KernelLayout, MemoryPermissions, PhysAddr, VirtAddr},
    },
    core::ptr,
};

/// Metadata for a kernel section
#[derive(Debug, Clone, Copy)]
pub struct KernelSectionMeta {
    /// Section name (for debugging)
    pub name: &'static str,
    /// Virtual address in kernel's higher-half address space
    pub virt_addr: u64,
    /// Size of section in bytes
    pub size: usize,
    /// Required alignment in bytes
    pub alignment: u64,
    /// Memory protection permissions
    pub permissions: MemoryPermissions,
}

impl KernelSectionMeta {
    /// Calculate offset from kernel virtual base
    pub const fn offset_from_base(&self, virt_base: u64) -> u64 {
        self.virt_addr - virt_base
    }

    /// Calculate physical address given kernel physical base
    pub const fn phys_addr(&self, kernel_phys_base: u64, kernel_virt_base: u64) -> u64 {
        kernel_phys_base + self.offset_from_base(kernel_virt_base)
    }

    /// Number of 4KB pages needed
    pub const fn page_count(&self) -> usize {
        self.size.div_ceil(0x1000)
    }
}

/// Exception vector table metadata
///
/// The vector table must be 2KB aligned for VBAR_EL1.
/// It contains 16 entries × 128 bytes = 2048 bytes total.
#[derive(Debug, Clone, Copy)]
pub struct VectorTableMeta {
    /// Virtual address of the vector table (higher-half)
    pub virt_addr: u64,
    /// Size of the vector table (typically 0x800 = 2048 bytes)
    pub size: usize,
    /// Alignment requirement (must be at least 2048 for VBAR)
    pub alignment: u64,
}

impl VectorTableMeta {
    /// Calculate offset from kernel virtual base
    pub const fn offset_from_base(&self, virt_base: u64) -> u64 {
        self.virt_addr - virt_base
    }

    /// Calculate physical address given kernel physical base
    ///
    /// This is the value to write to VBAR_EL1 before enabling MMU,
    /// since at that point we're still using physical addresses.
    pub const fn phys_addr(&self, kernel_phys_base: u64, kernel_virt_base: u64) -> u64 {
        kernel_phys_base + self.offset_from_base(kernel_virt_base) // FIXME: dupe of the above same fns
    }

    /// Verify the address meets VBAR alignment requirements
    pub const fn is_properly_aligned(&self, addr: u64) -> bool {
        addr & 0x7FF == 0 // Must be 2KB aligned
    }
}

/// Complete kernel image information
#[derive(Debug)]
pub struct KernelImageInfo {
    /// Virtual base address (higher-half) -- FIXME: don't need this necessarily
    pub virt_base: u64,
    /// Loadable sections with their binary data
    pub sections: &'static [LoadableSection],
    /// BSS section metadata (no binary data - must be zeroed)
    pub bss: Option<KernelSectionMeta>,
    /// Exception vector table metadata
    pub vectors: Option<VectorTableMeta>,
}

impl KernelImageInfo {
    /// Total size needed for kernel in physical memory (all sections + BSS)
    pub fn total_size(&self) -> usize {
        let mut max_end: u64 = 0;

        for section in self.sections {
            let end = section.meta.virt_addr + section.meta.size as u64;
            max_end = max_end.max(end);
        }

        if let Some(bss) = &self.bss {
            let bss_end = bss.virt_addr + bss.size as u64;
            max_end = max_end.max(bss_end);
        }

        let size = (max_end - self.virt_base) as usize;
        (size + 0xFFF) & !0xFFF // FIXME: aligned to a page size
    }
}

/// A loadable section with its binary content
#[derive(Debug)]
pub struct LoadableSection {
    pub meta: KernelSectionMeta,
    pub data: &'static [u8], // or Option<&'static [u8]>?
}

pub fn load_kernel(allocator: &mut BootAllocator) -> Result<KernelLayout, &'static str> {
    let total_size = KERNEL.total_size();
    let total_pages = total_size.div_ceil(0x1000);

    // Allocate 2MB-aligned for potential huge page mapping -- FIXME: with this we can abandon the whole loaded imade and do ASLR easy
    let phys_base = allocator
        .alloc_aligned(total_pages * 0x1000, 2 * 1024 * 1024)
        .ok_or("Failed to allocate memory for kernel")?;

    // Load each section
    for section in KERNEL.sections {
        load_section(section, phys_base)?;
    }

    // Zero BSS section
    if let Some(bss) = &KERNEL.bss {
        zero_bss(bss, phys_base)?;
    }

    memory_barrier();

    // Build layout information
    let bss_info = KERNEL.bss.map(|bss| {
        let phys = PhysAddr::new(phys_base.as_u64() + bss.offset_from_base(KERNEL.virt_base));
        (phys, VirtAddr::new(bss.virt_addr), bss.size)
    });

    // Calculate vector table addresses
    let vectors_info = KERNEL.vectors.map(|v| {
        let phys = PhysAddr::new(phys_base.as_u64() + v.offset_from_base(KERNEL.virt_base));
        let virt = VirtAddr::new(v.virt_addr);

        // Verify alignment
        if !virt.is_aligned(2048) {
            panic!(
                "Vector table virtual address 0x{:016X} is not 2KB aligned!",
                virt.0
            );
        }

        (phys, virt)
    });

    Ok(KernelLayout {
        phys_base,
        virt_base: VirtAddr::new(KERNEL.virt_base),
        total_size,
        sections: KERNEL.sections,
        bss_phys: bss_info.map(|(p, _, _)| p).unwrap_or(PhysAddr::new(0)),
        bss_virt: bss_info.map(|(_, v, _)| v).unwrap_or(VirtAddr::new(0)),
        bss_size: bss_info.map(|(_, _, s)| s).unwrap_or(0),
        vectors_phys: vectors_info.map(|(p, _)| p),
        vectors_virt: vectors_info.map(|(_, v)| v),
    })
}

fn load_section(section: &LoadableSection, kernel_phys_base: PhysAddr) -> Result<(), &'static str> {
    let offset = section.meta.offset_from_base(KERNEL.virt_base);
    let dest_phys = PhysAddr::new(kernel_phys_base.as_u64() + offset);

    if !dest_phys.as_u64().is_multiple_of(section.meta.alignment) {
        return Err("Section alignment violated");
    }

    unsafe {
        ptr::copy_nonoverlapping(
            section.data.as_ptr(),
            dest_phys.as_mut_ptr::<u8>(),
            section.data.len(),
        );
    }

    if section.meta.size > section.data.len() {
        let zero_start = PhysAddr::new(dest_phys.as_u64() + section.data.len() as u64);
        let zero_size = section.meta.size - section.data.len();
        unsafe {
            ptr::write_bytes(zero_start.as_mut_ptr::<u8>(), 0, zero_size);
        }
    }

    Ok(())
}

fn zero_bss(bss: &KernelSectionMeta, kernel_phys_base: PhysAddr) -> Result<(), &'static str> {
    let offset = bss.offset_from_base(KERNEL.virt_base);
    let dest_phys = PhysAddr::new(kernel_phys_base.as_u64() + offset);

    if !dest_phys.as_u64().is_multiple_of(bss.alignment) {
        return Err("BSS alignment violated");
    }

    unsafe {
        ptr::write_bytes(dest_phys.as_mut_ptr::<u8>(), 0, bss.size);
    }
    Ok(())
}

#[inline(always)]
pub fn memory_barrier() {
    unsafe {
        core::arch::asm!("dsb sy", "isb", options(nostack, preserves_flags));
    }
}

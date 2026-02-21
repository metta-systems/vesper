// init_thread/src/loader.rs

#[cfg(qemu)]
use libqemu::semi_println;
use {
    crate::{
        embed::KERNEL,
        memory::{Alloc, BootAllocator, KernelLayout, MemoryPermissions},
    },
    core::ptr,
    libaddress::{PhysAddr, VirtAddr},
};

/// Metadata for a kernel section
#[derive(Debug, Clone, Copy)]
pub struct SectionMeta {
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

impl SectionMeta {
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

/// Complete kernel image information
#[derive(Debug)]
pub struct ImageInfo {
    /// Virtual base address (higher-half) -- FIXME: don't need this necessarily
    pub virt_base: u64,
    /// Loadable sections with their binary data
    pub sections: &'static [LoadableSection],
    /// BSS section metadata (no binary data - must be zeroed)
    pub bss: SectionMeta,
    /// BSS section metadata (no binary data - must be zeroed)
    pub stack_virt_bottom: u64,
    /// Exception vector table metadata (to set up VBAR)
    pub vectors: SectionMeta,
}

impl ImageInfo {
    /// Total size needed for kernel in physical memory (all sections + BSS)
    pub fn total_size(&self) -> usize {
        let mut max_end: u64 = 0;

        for section in self.sections {
            let end = section.meta.virt_addr + section.meta.size as u64;
            max_end = max_end.max(end);
        }

        let bss_end = self.bss.virt_addr + self.bss.size as u64;
        max_end = max_end.max(bss_end);

        let size = (max_end - self.virt_base) as usize;
        (size + 0xFFF) & !0xFFF // FIXME: aligned to a page size
    }
}

/// A loadable section with its binary content
#[derive(Debug)]
pub struct LoadableSection {
    pub meta: SectionMeta,
    pub data: &'static [u8], // or Option<&'static [u8]>?
}

pub fn load_kernel(allocator: &mut BootAllocator) -> Result<KernelLayout, &'static str> {
    let total_size = KERNEL.total_size();
    let total_pages = total_size.div_ceil(0x1000);

    // Allocate 2MB-aligned for potential huge page mapping -- FIXME: with this we can abandon the whole loaded image and do ASLR easy
    let phys_base = allocator
        .alloc_aligned(
            total_pages * 0x1000,
            2 * 1024 * 1024,
            ("", Alloc::Persistent),
        )
        .ok_or("Failed to allocate memory for kernel")?;

    #[cfg(qemu)]
    semi_println!(
        "Nucleus is {total_pages} * 4K pages @ {:#016X}",
        phys_base.as_u64()
    );

    // Load each section
    for section in KERNEL.sections {
        load_section(section, phys_base)?;
    }

    // Zero BSS section
    zero_bss(&KERNEL.bss, phys_base)?;

    memory_barrier();

    // Build layout information
    let bss_info = {
        let phys =
            PhysAddr::new(phys_base.as_u64() + KERNEL.bss.offset_from_base(KERNEL.virt_base));
        (phys, VirtAddr::new(KERNEL.bss.virt_addr), KERNEL.bss.size)
    };

    // Calculate vector table addresses
    let vectors_virt = {
        let virt = VirtAddr::new(KERNEL.vectors.virt_addr);

        // Verify alignment
        if !virt.is_aligned(2048u64) {
            panic!(
                "Vector table virtual address 0x{:016X} is not 2KB aligned!",
                virt.as_u64()
            );
        }

        virt
    };

    Ok(KernelLayout {
        phys_base,
        virt_base: VirtAddr::new(KERNEL.virt_base),
        total_size,
        sections: KERNEL.sections,
        bss_phys: bss_info.0,
        bss_virt: bss_info.1,
        bss_size: bss_info.2,
        stack_virt_bottom: VirtAddr::new(KERNEL.stack_virt_bottom),
        vectors_virt: vectors_virt,
    })
}

fn load_section(section: &LoadableSection, kernel_phys_base: PhysAddr) -> Result<(), &'static str> {
    let offset = section.meta.offset_from_base(KERNEL.virt_base);
    let dest_phys = PhysAddr::new(kernel_phys_base.as_u64() + offset);

    #[cfg(qemu)]
    semi_println!(
        "> section {}, copy {} bytes of {} bytes total to {:#016X}",
        section.meta.name,
        section.data.len(),
        section.meta.size,
        dest_phys.as_u64()
    );

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

fn zero_bss(bss: &SectionMeta, kernel_phys_base: PhysAddr) -> Result<(), &'static str> {
    let offset = bss.offset_from_base(KERNEL.virt_base);
    let dest_phys = PhysAddr::new(kernel_phys_base.as_u64() + offset);

    #[cfg(qemu)]
    semi_println!(
        "> section {}, zero {} bytes at {:#016X}",
        bss.name,
        bss.size,
        dest_phys.as_u64()
    );

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

// kickstart/src/loader.rs
// TODO: replace semi::prints with logger output

pub use libimage::{LoadableSection, SectionMeta};
use {
    crate::{
        embed::KERNEL,
        memory::{Alloc, BootAllocator, KernelLayout},
    },
    core::ptr,
    libaddress::{PhysAddr, VirtAddr},
    libqemu::semihosting as semi,
};

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

    semi::println!(
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
        assert!(
            virt.is_aligned(2048_u64),
            "Vector table virtual address 0x{:016X} is not 2KB aligned!",
            virt.as_u64()
        );

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
        vectors_virt,
    })
}

fn load_section(section: &LoadableSection, kernel_phys_base: PhysAddr) -> Result<(), &'static str> {
    let offset = section.meta.offset_from_base(KERNEL.virt_base);
    let dest_phys = PhysAddr::new(kernel_phys_base.as_u64() + offset);

    semi::println!(
        "> section {}, copy {} bytes of {} bytes total to {:#016X}",
        section.meta.name,
        section.data.len(),
        section.meta.size,
        dest_phys.as_u64()
    );

    if !dest_phys.as_u64().is_multiple_of(section.meta.alignment) {
        return Err("Section alignment violated");
    }

    // SAFETY: Unsafe
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
        // SAFETY: Unsafe
        unsafe {
            ptr::write_bytes(zero_start.as_mut_ptr::<u8>(), 0, zero_size);
        }
    }

    Ok(())
}

fn zero_bss(bss: &SectionMeta, kernel_phys_base: PhysAddr) -> Result<(), &'static str> {
    let offset = bss.offset_from_base(KERNEL.virt_base);
    let dest_phys = PhysAddr::new(kernel_phys_base.as_u64() + offset);

    semi::println!(
        "> section {}, zero {} bytes at {:#016X}",
        bss.name,
        bss.size,
        dest_phys.as_u64()
    );

    if !dest_phys.as_u64().is_multiple_of(bss.alignment) {
        return Err("BSS alignment violated");
    }

    // SAFETY: Unsafe
    unsafe {
        ptr::write_bytes(dest_phys.as_mut_ptr::<u8>(), 0, bss.size);
    }
    Ok(())
}

#[inline(always)]
pub fn memory_barrier() {
    // SAFETY: Unsafe
    unsafe {
        core::arch::asm!("dsb sy", "isb", options(nostack, preserves_flags));
    }
}

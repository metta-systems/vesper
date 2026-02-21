// init_thread/src/paging.rs - Page table management with section-aware mapping

/// Kernel virtual address space layout (TTBR1 region: 0xFFFF_xxxx_xxxx_xxxx)
///
/// ```
/// 0xFFFF_FFFF_FFFF_FFFF  ┌─────────────────────┐
///                        │  Kernel stacks      │  Per-CPU kernel stacks
/// 0xFFFF_FFFF_8000_0000  ├─────────────────────┤
///                        │  Kernel heap        │  (if any - we try to avoid)
/// 0xFFFF_FFFF_0000_0000  ├─────────────────────┤
///                        │  DCB shared pages   │  Read-only mapped to user too
/// 0xFFFF_FF00_0000_0000  ├─────────────────────┤
///                        │  Device MMIO        │  1:1 mapped device regions
/// 0xFFFF_0080_0000_0000  ├─────────────────────┤
///                        │  Physical memory    │  Linear map of all RAM
///                        │  (kernel direct)    │  Kernel can access any phys through this offset
/// 0xFFFF_0000_0000_0000  └─────────────────────┘
/// ```
#[cfg(qemu)]
use libqemu::semi_println;
use {
    crate::memory::{Alloc, BootAllocator, KernelLayout, MemoryPermissions, SectionMapping},
    core::ptr,
    libaddress::{PhysAddr, VirtAddr},
};

const KERNEL_BASE: u64 = 0xFFFF_8000_0000_0000;
const KERNEL_PHYS_MAP: u64 = 0xFFFF_8000_0000_0000; // Linear map base
const KERNEL_DEVICE_BASE: u64 = 0xFFFF_8080_0000_0000;
const KERNEL_DCB_BASE: u64 = 0xFFFF_FF00_0000_0000;
const KERNEL_HEAP_BASE: u64 = 0xFFFF_FFFF_0000_0000;
const KERNEL_STACK_BASE: u64 = 0xFFFF_FFFF_8000_0000;

/// Page table entry flags for `AArch64` Stage 1
pub mod flags {
    pub const VALID: u64 = 1 << 0;
    pub const TABLE: u64 = 1 << 1;
    pub const PAGE: u64 = 1 << 1;

    // Access permissions (AP[2:1])
    pub const AP_RW_EL1: u64 = 0b00 << 6;
    pub const AP_RO_EL1: u64 = 0b10 << 6;

    // Shareability
    pub const SH_INNER: u64 = 0b11 << 8;

    // Access flag
    pub const AF: u64 = 1 << 10;

    // Memory attributes index
    pub const ATTR_NORMAL: u64 = 0 << 2;
    pub const ATTR_DEVICE: u64 = 1 << 2;

    // Execute never
    pub const PXN: u64 = 1 << 53;
    pub const UXN: u64 = 1 << 54;

    // Block descriptor
    pub const BLOCK: u64 = 0;
}

/// Page table (512 entries × 8 bytes = 4KB)
#[repr(C, align(4096))]
pub struct PageTable {
    entries: [u64; 512],
}

impl PageTable {
    pub const fn new() -> Self {
        Self { entries: [0; 512] }
    }
}

/// Which TTBR to use
#[derive(Debug, Clone, Copy)]
pub enum Ttbr {
    Ttbr0,
    Ttbr1,
}

/// MMU configuration builder
pub struct MmuSetup<'a> {
    allocator: &'a mut BootAllocator,
    ttbr0_l0: PhysAddr,
    ttbr1_l0: PhysAddr,
}

impl<'a> MmuSetup<'a> {
    pub fn new(allocator: &'a mut BootAllocator) -> Result<Self, &'static str> {
        let ttbr0_l0 = allocator
            .alloc_pages(1, ("user L0", Alloc::Droppable))
            .ok_or("Failed to allocate TTBR0 L0 table")?;

        let ttbr1_l0 = allocator
            .alloc_pages(1, ("kernel L0", Alloc::Persistent))
            .ok_or("Failed to allocate TTBR1 L0 table")?;

        // SAFETY: Unsafe
        unsafe {
            ptr::write_bytes(ttbr0_l0.as_mut_ptr::<u8>(), 0, 4096);
            ptr::write_bytes(ttbr1_l0.as_mut_ptr::<u8>(), 0, 4096);
        }

        Ok(Self {
            allocator,
            ttbr0_l0,
            ttbr1_l0,
        })
    }

    pub fn memory_top(&self) -> PhysAddr {
        self.allocator.current()
    }

    /// Map a 4KB page with specific permissions
    pub fn map_page(
        &mut self,
        ttbr: Ttbr,
        virt: VirtAddr,
        phys: PhysAddr,
        perms: MemoryPermissions,
        usage: (&'static str, Alloc),
    ) -> Result<(), &'static str> {
        let pte_flags = perms.as_pte_flags() | flags::ATTR_NORMAL;
        self.map_page_with_flags(ttbr, virt, phys, pte_flags, perms, usage)
    }

    /// Map a 4KB page with raw PTE flags
    fn map_page_with_flags(
        &mut self,
        ttbr: Ttbr,
        virt: VirtAddr,
        phys: PhysAddr,
        pte_flags: u64,
        perms: MemoryPermissions, // Only for Display
        usage: (&'static str, Alloc),
    ) -> Result<(), &'static str> {
        let l0_phys = match ttbr {
            Ttbr::Ttbr0 => self.ttbr0_l0,
            Ttbr::Ttbr1 => self.ttbr1_l0,
        };

        let va = virt.as_u64();
        let l0_idx = ((va >> 39) & 0x1FF) as usize;
        let l1_idx = ((va >> 30) & 0x1FF) as usize;
        let l2_idx = ((va >> 21) & 0x1FF) as usize;
        let l3_idx = ((va >> 12) & 0x1FF) as usize;

        let l1_phys = self.ensure_table(l0_phys, l0_idx, usage)?;
        let l2_phys = self.ensure_table(l1_phys, l1_idx, usage)?;
        let l3_phys = self.ensure_table(l2_phys, l2_idx, usage)?;

        // SAFETY: Unsafe
        let l3_table = unsafe { &mut *(l3_phys.as_mut_ptr::<PageTable>()) };
        l3_table.entries[l3_idx] = phys.as_u64() | flags::VALID | flags::PAGE | pte_flags;

        #[cfg(qemu)]
        semi_println!(
            "Mapped 4K page {} frame {} in {} with {}",
            virt,
            phys,
            match ttbr {
                Ttbr::Ttbr0 => "TTBR0(user)",
                Ttbr::Ttbr1 => "TTBR1(kernel)",
            },
            perms
        );

        Ok(())
    }

    /// Map a 2MB block with specific permissions
    pub fn map_block_2mb(
        &mut self,
        ttbr: Ttbr,
        virt: VirtAddr,
        phys: PhysAddr,
        perms: MemoryPermissions,
        usage: (&'static str, Alloc),
    ) -> Result<(), &'static str> {
        if virt.as_u64() & 0x1F_FFFF != 0 || phys.as_u64() & 0x1F_FFFF != 0 {
            return Err("2MB block mapping requires 2MB alignment");
        }

        let pte_flags = perms.as_pte_flags() | flags::ATTR_NORMAL;

        let l0_phys = match ttbr {
            Ttbr::Ttbr0 => self.ttbr0_l0,
            Ttbr::Ttbr1 => self.ttbr1_l0,
        };

        let va = virt.as_u64();
        let l0_idx = ((va >> 39) & 0x1FF) as usize;
        let l1_idx = ((va >> 30) & 0x1FF) as usize;
        let l2_idx = ((va >> 21) & 0x1FF) as usize;

        let l1_phys = self.ensure_table(l0_phys, l0_idx, usage)?;
        let l2_phys = self.ensure_table(l1_phys, l1_idx, usage)?;

        // SAFETY: Unsafe
        let l2_table = unsafe { &mut *(l2_phys.as_mut_ptr::<PageTable>()) };
        l2_table.entries[l2_idx] = phys.as_u64() | flags::VALID | flags::BLOCK | pte_flags;

        #[cfg(qemu)]
        semi_println!(
            "Mapped 2M page {} frame {} in {} with {}",
            virt,
            phys,
            match ttbr {
                Ttbr::Ttbr0 => "TTBR0(user)",
                Ttbr::Ttbr1 => "TTBR1(kernel)",
            },
            perms
        );

        Ok(())
    }

    fn ensure_table(
        &mut self,
        table_phys: PhysAddr,
        index: usize,
        usage: (&'static str, Alloc),
    ) -> Result<PhysAddr, &'static str> {
        // SAFETY: Unsafe
        let table = unsafe { &mut *(table_phys.as_mut_ptr::<PageTable>()) };
        let entry = table.entries[index];

        if entry & flags::VALID != 0 {
            Ok(PhysAddr::new(entry & 0x0000_FFFF_FFFF_F000))
        } else {
            let new_table = self
                .allocator
                .alloc_pages(1, usage)
                .ok_or("Failed to allocate page table")?;

            // SAFETY: Unsafe
            unsafe {
                ptr::write_bytes(new_table.as_mut_ptr::<u8>(), 0, 4096);
            }

            table.entries[index] = new_table.as_u64() | flags::VALID | flags::TABLE;
            Ok(new_table)
        }
    }

    pub fn ttbr0(&self) -> u64 {
        self.ttbr0_l0.as_u64()
    }

    pub fn ttbr1(&self) -> u64 {
        self.ttbr1_l0.as_u64()
    }
}

/// Create identity mapping for `init_thread`
pub fn create_identity_mapping(
    setup: &mut MmuSetup,
    start: PhysAddr,
    end: PhysAddr,
) -> Result<(), &'static str> {
    let start_aligned = start.aligned_down(2_u64 * 1024 * 1024);
    let end_aligned = end.aligned_up(2_u64 * 1024 * 1024);

    let perms = MemoryPermissions {
        readable: true,
        writable: true,
        executable: true,
    }; // for init code -- FIXME: not necessarily!

    let mut addr = start_aligned;
    while addr.as_u64() < end_aligned.as_u64() {
        setup.map_block_2mb(
            Ttbr::Ttbr0,
            VirtAddr::new(addr.as_u64()),
            addr,
            perms,
            ("Init_Thread identity mapping", Alloc::Droppable),
        )?;
        addr = PhysAddr::new(addr.as_u64() + 2 * 1024 * 1024);
    }

    Ok(())
}

/// Create higher-half mapping for kernel with proper per-section permissions
pub fn create_kernel_mapping(
    setup: &mut MmuSetup,
    layout: &KernelLayout,
    max_ram_bytes: u64,
    el1_stack: u64,
    el1_stack_size: usize,
) -> Result<(u64,), &'static str> {
    // Map each section with its specific permissions
    for section in layout.iter_sections() {
        map_section(setup, &section)?;
    }

    // Map BSS section
    if let Some(bss) = layout.bss_mapping() {
        map_section(setup, &bss)?;
    }

    // ─────────────────────────────────────────────────────────────────
    // Setup linear physical map (all RAM accessible to nucleus)
    // TODO: exclude physical memory that covers the kernel image itself!
    // ─────────────────────────────────────────────────────────────────

    let perms = MemoryPermissions {
        readable: true,
        writable: true,
        executable: false,
    };

    // Map all physical memory using 2MB blocks with a specific offset
    // Kernel mapping for phys memory starts at
    for i in 0..max_ram_bytes.div_ceil(2 * 1024 * 1024) {
        setup.map_block_2mb(
            Ttbr::Ttbr1,
            VirtAddr::new(libaddress::PHYSICAL_KERNEL_WINDOW + i * 2 * 1024 * 1024),
            PhysAddr::new(i * 2 * 1024 * 1024),
            perms,
            ("Nucleus phys memory mapping", Alloc::Persistent),
        );
    }

    // Map kernel stack
    let stack_bottom = layout.stack_virt_bottom;

    for i in 0..el1_stack_size.div_ceil(4 * 1024) as u64 {
        setup.map_page(
            Ttbr::Ttbr1,
            VirtAddr::new(stack_bottom.as_u64() + i * 4 * 1024),
            PhysAddr::new(el1_stack + i * 4 * 1024),
            perms,
            ("Nucleus stack mapping", Alloc::Persistent),
        );
    }

    let stack_virt_top = stack_bottom + el1_stack_size;

    // ─────────────────────────────────────────────────────────────────
    // Setup device MMIO mappings
    // ─────────────────────────────────────────────────────────────────
    // RPi4 peripherals at 0xFE00_0000 - 0xFF00_0000
    // let device_base_phys = 0xFE00_0000_u64;
    // Map as device memory (non-cacheable, no speculation)
    // create_device_mapping(setup, device_base_phys, VIRT_MMIO, 0x100_0000); // 16MiB

    Ok((stack_virt_top.into(),))
}

/// Map a single section with proper permissions
fn map_section(setup: &mut MmuSetup, section: &SectionMapping) -> Result<(), &'static str> {
    if !section.phys_start.is_aligned(4096_u64) {
        #[cfg(qemu)]
        semi_println!("!! Section {} not aligned to 4K boundary!", section.name);
        return Err("Section not aligned");
    }

    // Check if we can use 2MB blocks (section must be 2MB aligned and sized)
    let can_use_2mb = section.phys_start.is_aligned(2_u64 * 1024 * 1024)
        && section.virt_start.as_u64().is_multiple_of(2 * 1024 * 1024)
        && section.size >= 2 * 1024 * 1024;

    if can_use_2mb {
        // Use 2MB blocks for large aligned sections
        let mut phys = section.phys_start;
        let mut virt = section.virt_start;
        let mut remaining = section.size;

        while remaining >= 2 * 1024 * 1024 {
            setup.map_block_2mb(
                Ttbr::Ttbr1,
                virt,
                phys,
                section.permissions,
                ("Nucleus section mapping", Alloc::Persistent),
            )?;
            phys = PhysAddr::new(phys.as_u64() + 2 * 1024 * 1024);
            virt = VirtAddr::new(virt.as_u64() + 2 * 1024 * 1024);
            remaining -= 2 * 1024 * 1024;
        }

        // Map remaining pages
        while remaining > 0 {
            setup.map_page(
                Ttbr::Ttbr1,
                virt,
                phys,
                section.permissions,
                ("Nucleus section mapping", Alloc::Persistent),
            )?;
            phys = PhysAddr::new(phys.as_u64() + 0x1000);
            virt = VirtAddr::new(virt.as_u64() + 0x1000);
            remaining = remaining.saturating_sub(0x1000);
        }
    } else {
        // Use 4KB pages
        for (phys, virt) in section.pages() {
            setup.map_page(
                Ttbr::Ttbr1,
                virt,
                phys,
                section.permissions,
                ("Nucleus section mapping", Alloc::Persistent),
            )?;
        }
    }

    Ok(())
}

/// Create device memory mapping
pub fn create_device_mapping(
    setup: &mut MmuSetup,
    phys: PhysAddr,
    virt: VirtAddr,
    size: usize,
) -> Result<(), &'static str> {
    let pages = size.div_ceil(0x1000);
    let perms = MemoryPermissions {
        readable: true,
        writable: true,
        executable: false,
    };

    for i in 0..pages {
        let offset = (i * 0x1000) as u64;

        // let _l0_phys = setup.ttbr1_l0;
        let va = virt.as_u64() + offset;
        let pa = phys.as_u64() + offset;

        // Use device memory attributes
        let pte_flags = perms.as_pte_flags() | flags::ATTR_DEVICE;
        // PageFlags::KERNEL_RW | PageFlags::DEVICE_nGnRnE,
        setup.map_page_with_flags(
            Ttbr::Ttbr1,
            VirtAddr::new(va),
            PhysAddr::new(pa),
            pte_flags,
            perms,
            ("Nucleus device mapping", Alloc::Persistent),
        )?;
    }

    Ok(())
}

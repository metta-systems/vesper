/// Boot info collected during early init
/// Lives in .data section (not heap - no allocator yet!)
#[repr(C)]
struct BootInfo {
    dtb_phys: PhysAddr,
    dtb_size: usize,

    // Memory regions from DTB
    memory_regions: [MemoryRegion; 16],
    memory_region_count: usize,

    // Reserved regions (kernel image, DTB, modules)
    reserved_regions: [ReservedRegion; 32],
    reserved_region_count: usize,

    // Loaded modules (init process, drivers)
    modules: [LoadedModule; 8],
    module_count: usize,
    // Kernel image bounds
    // kernel_phys_start: PhysAddr,
    // kernel_phys_end: PhysAddr,

    // Init thread stack (will be reclaimed)
    // init_stack_phys: PhysAddr,
    // init_stack_size: usize,
}

#[repr(C)]
struct MemoryRegion {
    base: PhysAddr,
    size: usize,
    flags: MemoryFlags,
}

#[repr(C)]
struct LoadedModule {
    name: [u8; 32],
    phys_start: PhysAddr,
    size: usize,
    entry_point: u64, // Offset from phys_start
}

/// Static boot info - filled during early init
#[unsafe(link_section = ".init_thread.bss")]
static mut BOOT_INFO: BootInfo = BootInfo::zeroed();

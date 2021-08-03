#![no_std]
#![no_main]
#![allow(unused)]
#![feature(format_args_nl)]
#![feature(try_find)] // For DeviceTree iterators

// Init-thread process.
// - Start initializing the kernel
// - Enter itself into process list as high-priority privileged process
// - The bootup is driven by the init_thread which loads and parses devtree, maps the kernel, gives itself all necessary
// capabilities, (probably loads more things into their places) and transitions to user mode.
// - From user mode it can continue distributing capabilities and launching servers until everything is handed out.
// - After init is completed, it should create more low-priority user processes including idle, fs, some other handlers and
// a user-space boot process like /sbin/init or sth, with scripts to control the boot up.
// - At this point, exit the init_thread process normally.
// - The init-thread can be terminated and its memory freed up (should it inject a process descriptor for itself somehow to allow normal shutdown mechanisms to clean up? most probably).

// create "tracing" and "debug" components for kernel call keys (intercepting syscall caps)

// Kernel's main.rs just brings together all libs and syscall entry points.
// init_thread.rs provides a boot up entry point which sets up everything.

// init_thread should do some shared init
// and some system-specific loading like parsing the DTB and loading system drivers
// distribute keys - this should be listed somewhere in the definitions (CapDL?)

// kernel entities:
// - keys
// - key invocation
// - time access and timers activation - ??

// userspace entities:
// - processes
// - threads (first version - threads are in-process, kernel has no idea)
// - scheduler (invokes process upcall key)

mod boot;
mod device_tree;
mod el_switch;
mod embed;
mod loader;
mod memory;
mod paging;
mod syscall_test;

use {
    core::{alloc::Allocator, panic::PanicInfo, ptr::write_bytes},
    device_tree::{DeviceTree, DeviceTreeProp},
    fdt_rs::{
        base::DevTree,
        prelude::{FallibleIterator, PropReader},
    },
    libcpu::endless_sleep,
    libqemu::semi_println,
    memory::{BootAllocator, PhysAddr},
    syscall_test::protected_call6,
};

unsafe extern "C" {
    static __init_start: u8;
    static __init_end: u8;
    static __free_memory_start: u8;
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    semi_println!("PANICKED: {info}");
    endless_sleep()
}

fn dump_memory_map() {
    // Output the memory map as we could derive from FDT and information about our loaded image
    // Use it to imagine how the memmap would look like in the end.
    arch::memory::print_layout();
}

#[unsafe(no_mangle)]
pub extern "C" fn init_main(dtb_ptr: *const u8) -> ! {
    semi_println!("init_main started");

    // unsafe {
    //     BOOT_INFO.dtb_phys = PhysAddr::new(dtb_phys);
    // }

    // ─────────────────────────────────────────────────────────────────────
    // Initialize early UART for debug output
    // ─────────────────────────────────────────────────────────────────────

    // Hardcoded UART address for early boot (RPi4: 0xFE201000)
    // Will be properly mapped later
    // early_uart_init(0xFE20_1000);
    semi_println!("DTB at physical: {:#016X}", dtb_ptr as u64);

    // ─────────────────────────────────────────────────────────────────────
    // Parse Device Tree
    // ─────────────────────────────────────────────────────────────────────

    semi_println!("Parsing device tree...");

    // Safety: we got the address from the bootloader, if it lied - well, we're screwed!
    let device_tree = unsafe {
        DevTree::from_raw_pointer(dtb_ptr as *const _).expect("DeviceTree failed to read")
    };

    let layout = DeviceTree::layout(device_tree).expect("Couldn't calculate DeviceTree index");

    let mut block = allocator
        .alloc(layout.size)
        .expect("Couldn't allocate DeviceTree index");

    let device_tree =
        DeviceTree::new(device_tree, block).expect("Couldn't initialize indexed DeviceTree");

    let model = device_tree
        .get_prop_by_path("/model")
        .unwrap()
        .str()
        .expect("Model must be a string");
    semi_println!("Booting on {}", model);

    // To init memory allocation we need to parse memory regions from dtb and add the regions to
    // available memory regions list. Then initial BootRegionAllocator will get memory from these
    // regions and record their usage into some OTHER structures, removing these allocations from
    // the free regions list.
    // memory allocation is described by reg attribute of /memory block.
    // /#address-cells and /#size-cells specify the sizes of address and size attributes in reg.
    // To get memory size from DTB:
    // 1. Find nodes with unit-names `/memory`
    // 2. From those read reg entries, using `/#address-cells` and `/#size-cells` as units
    // 3. Union of all these reg entries will be the available memory. Enter it as mem-regions.

    let address_cells = device_tree
        .get_prop_by_path("/#address-cells")
        .expect("Unable to figure out #address-cells")
        .u32(0)
        .expect("Invalid format for #address-cells");

    let size_cells = device_tree
        .get_prop_by_path("/#size-cells")
        .expect("Unable to figure out #size-cells")
        .u32(0)
        .expect("Invalid format for #size-cells");

    // @todo boot this on 8Gb RasPi, because I'm not sure how it allocates memory regions there.
    semi_println!(
        "Address cells: {}, size cells {}",
        address_cells,
        size_cells
    );

    let mem_prop = device_tree
        .props()
        .find(|p| Ok(p.name()? == "device_type" && p.str()? == "memory"))
        .unwrap()
        .expect("Unable to find memory node.");
    let mem_node = mem_prop.node();
    // let parent_node = mem_node.parent_node();

    let reg_prop = device_tree
        .get_prop_by_path("/memory@0/reg")
        .expect("Unable to figure out memory-reg");

    semi_println!(
        "Found memnode with reg prop: name {:?}, size {}",
        reg_prop.name(),
        reg_prop.length()
    );

    let reg_prop = DeviceTreeProp::new(reg_prop);
    let mut mem_iter = reg_prop.payload_pairs_iter(address_cells, size_cells);

    while let Some((mem_addr, mem_size)) = mem_iter.next() {
        semi_println!("Memory: {} KiB at offset {}", mem_size / 1024, mem_addr);
    }

    // List unusable memory, and remove it from the memory regions for the allocator.
    let mut iter = device_tree.fdt().reserved_entries();
    while let Some(entry) = iter.next() {
        semi_println!(
            "Reserved memory: {:?} bytes at {:?}",
            entry.size,
            entry.address
        );
    }

    // Iterate compatible nodes (example):
    // let mut iter = device_tree.compatible_nodes("arm,pl011");
    // while let Some(entry) = iter.next() {
    //     semi_println!("reserved: {:?} (bytes at ?)", entry.name()/*, entry.address*/);
    // }

    // Also, remove the DTB memory region + index
    semi_println!(
        "DTB region: {} bytes at {:x}",
        device_tree.fdt().totalsize(),
        dtb
    );

    // let address_cells = device_tree.try_struct_u32_value("/#address-cells");
    // let size_cells = device_tree.try_struct_u32_value("/#size-cells");
    // let board = device_tree.try_struct_str_value("/model");

    // if board.is_ok() {
    //     semi_println!("Running on {}", board.unwrap());
    // }

    // semi_println!(
    //     "Memory DTB info: address-cells {:?}, size-cells {:?}",
    //     address_cells,
    //     size_cells
    // );

    dump_memory_map();

    // Next step: parse DTB!
    // let dtb = unsafe {
    //     // Direct physical access - MMU off
    //     DeviceTree::from_phys(PhysAddr::new(dtb_phys)).expect("Invalid DTB")
    // };

    // unsafe {
    //     BOOT_INFO.dtb_size = dtb.total_size();

    //     // Extract memory regions
    //     for region in dtb.memory_regions() {
    //         if BOOT_INFO.memory_region_count < 16 {
    //             BOOT_INFO.memory_regions[BOOT_INFO.memory_region_count] = region;
    //             BOOT_INFO.memory_region_count += 1;
    //             semi_println!(
    //                 "  RAM: {:#x} - {:#x}",
    //                 region.base.as_u64(),
    //                 region.base.as_u64() + region.size as u64
    //             );
    //         }
    //     }

    //     // Extract reserved regions
    //     for reserved in dtb.reserved_regions() {
    //         if BOOT_INFO.reserved_region_count < 32 {
    //             BOOT_INFO.reserved_regions[BOOT_INFO.reserved_region_count] = reserved;
    //             BOOT_INFO.reserved_region_count += 1;
    //         }
    //     }

    //     // Extract boot modules (loaded by bootloader)
    //     for module in dtb.modules() {
    //         if BOOT_INFO.module_count < 8 {
    //             BOOT_INFO.modules[BOOT_INFO.module_count] = module;
    //             BOOT_INFO.module_count += 1;
    //             semi_println!(
    //                 "  Module '{}': {:#x}, {} bytes",
    //                 core::str::from_utf8(&module.name).unwrap_or("???"),
    //                 module.phys_start.as_u64(),
    //                 module.size
    //             );
    //         }
    //     }
    // }

    // ─────────────────────────────────────────────────────────────────────
    // Further init
    // ─────────────────────────────────────────────────────────────────────

    let init_start = unsafe { &__init_start as *const u8 as u64 };
    let init_end = unsafe { &__init_end as *const u8 as u64 };
    let free_start = unsafe { &__free_memory_start as *const u8 as u64 };

    let memory_size = 256 * 1024 * 1024;
    let mut allocator = BootAllocator::new(PhysAddr::new(free_start), memory_size);
    let memory_end = allocator.end();
    semi_println!(
        "init_main: Created BootAllocator {memory_size} @ {:#016X}",
        free_start
    );

    // ═══════════════════════════════════════════════════════════════
    // PHASE 1: Load kernel
    // ═══════════════════════════════════════════════════════════════

    let kernel_layout = loader::load_kernel(&mut allocator).expect("Failed to load nucleus");
    semi_println!("init_main: Loaded nucleus image");

    // ═══════════════════════════════════════════════════════════════
    // PHASE 2: Set up page tables
    // ═══════════════════════════════════════════════════════════════

    let mut mmu_setup = paging::MmuSetup::new(&mut allocator).expect("Failed to create MMU setup");
    semi_println!("init_main: Created MmuSetup");

    // Identity map init_thread
    paging::create_identity_mapping(&mut mmu_setup, PhysAddr::new(init_start), memory_end)
        .expect("Failed to create identity mapping");
    semi_println!("init_main: Identity mapped the Init_Thread");

    // Create kernel mapping with per-section permissions
    paging::create_kernel_mapping(&mut mmu_setup, &kernel_layout)
        .expect("Failed to create kernel mapping");
    semi_println!("init_main: Higher-half mapped the nucleus");

    // ═══════════════════════════════════════════════════════════════
    // PHASE 3: Prepare for EL1
    // ═══════════════════════════════════════════════════════════════

    let ttbr0 = mmu_setup.ttbr0();
    let ttbr1 = mmu_setup.ttbr1();
    semi_println!("init_main: TTBR0_EL1 at {ttbr0:#016X}, TTBR1_EL1 at {ttbr1:#016X}");

    // Get vector table virtual address for VBAR_EL1
    // VBAR is only used after MMU is enabled, so we set the virtual address directly
    let vbar = kernel_layout.vbar_el1_virt();

    // Allocate EL1 stack
    let el1_stack = allocator
        .alloc_pages(16)
        .expect("Failed to allocate EL1 stack");
    let el1_stack_top = el1_stack.as_u64() + 16 * 4096;
    // FIXME: stack must be identity-mapped!
    semi_println!("init_main: EL1 stack at {el1_stack_top:#016X}, vbar {vbar:#016X}");

    // ═══════════════════════════════════════════════════════════════
    // PHASE 4: Enable MMU and drop to EL1
    // ═══════════════════════════════════════════════════════════════

    unsafe {
        el_switch::enable_mmu_and_drop_to_el1(
            ttbr0,
            ttbr1,
            vbar,
            init_thread_run as *const () as u64,
            el1_stack_top,
        );
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn init_thread_run(_dtb_ptr: *const u8) -> ! {
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // PHASE 5: Initialize kernel objects and structures
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    // Run initial thread further in EL1, seting up the capDL etc.
    semi_println!("init_main_run dropped to EL1");
    unsafe {
        protected_call6(0, 0, 0, 0, 0, 0, 0, 0);
    }
    semi_println!("init_main_run: Returned from syscall");

    // ─────────────────────────────────────────────────────────────────────
    // Initialize kernel subsystems
    // ─────────────────────────────────────────────────────────────────────

    semi_println!("Initializing kernel subsystems...");

    // Initialize per-CPU data structures
    // percpu::init();

    // Initialize exception vectors
    // exceptions::init();

    // Initialize interrupt controller (GIC on RPi4)
    // let boot_info = unsafe { &BOOT_INFO };
    // interrupts::init_gic(boot_info);

    // ─────────────────────────────────────────────────────────────────────
    // Build physical memory map and create Untyped caps
    // ─────────────────────────────────────────────────────────────────────

    // semi_println!("Building physical memory allocator...");

    // Create the root untyped capability list
    // This will be delegated to init process
    // let mut untyped_list = UntypedList::new();
    // // let untyped_list = create_untyped_caps();

    // for i in 0..boot_info.memory_region_count {
    //     let region = &boot_info.memory_regions[i];

    //     // Skip reserved regions (kernel image, DTB, modules, page tables)
    //     let usable_ranges = subtract_reserved_regions(region, boot_info);

    //     for range in usable_ranges {
    //         // Create untyped caps for each usable chunk
    //         // Align to largest power-of-2 for efficient retyping
    //         let untypeds = create_untyped_caps_for_range(range);
    //         untyped_list.extend(untypeds);

    //         semi_println!(
    //             "  Untyped: {:#x} - {:#x} ({} caps)",
    //             range.base.as_u64(),
    //             range.base.as_u64() + range.size as u64,
    //             untypeds.len()
    //         );
    //     }
    // }

    // semi_println!("Total untyped caps: {}", untyped_list.len());

    // ─────────────────────────────────────────────────────────────────────
    // Initialize DCB shared pages
    // ─────────────────────────────────────────────────────────────────────

    // semi_println!("Initializing DCB pages...");

    // // Allocate DCB pages from a reserved untyped
    // // These are special: mapped RW in kernel, RO in all user domains
    // let dcb_pages = allocate_dcb_pages(&mut untyped_list, MAX_DOMAINS);
    // dcb::init(dcb_pages);

    // ─────────────────────────────────────────────────────────────────────
    // Create kernel idle domain (domain 0)
    // ─────────────────────────────────────────────────────────────────────

    // semi_println!("Creating idle domain...");

    // let idle_domain = Domain::create_idle();
    // SCHEDULER.set_idle(idle_domain);

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // PHASE 6: Create the init domain and its capability space
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    // semi_println!("Creating init domain...");

    // let init_module = boot_info
    //     .modules
    //     .iter()
    //     .find(|m| {
    //         let name = core::str::from_utf8(&m.name).unwrap_or("");
    //         name.matches("init")
    //     })
    //     .expect("No init module found in boot modules");

    // let init_domain = create_init_domain(init_module, &mut untyped_list);

    // ─────────────────────────────────────────────────────────────────────
    // Mark init thread memory as reclaimable
    // ─────────────────────────────────────────────────────────────────────

    // semi_println!("Marking init thread memory for reclamation...");

    // // The init stack and any init-only code/data can now be reclaimed, the are in the Untypeds table now.
    // mark_init_memory_reclaimable(boot_info, &mut untyped_list);

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // PHASE 7: Delegate all resources to init domain
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    // semi_println!(
    //     "Delegating {} untyped caps to init...",
    //     untyped_list.len()
    // );

    // delegate_untypeds_to_init(&init_domain, untyped_list);

    // // Create module caps for other boot modules and delegate
    // delegate_module_caps_to_init(&init_domain, boot_info);

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // PHASE 8: Context switch to init domain
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    // semi_println!("Switching to init domain...");
    // semi_println!("═══════════════════════════════════════════════════════════");

    // // Create initial time budget for init
    // let init_time = TimeCap::create_root(INIT_TIME_BUDGET_US);

    // // Finally: switch to userspace init
    // // This replaces TTBR0 with init's page tables
    // // Kernel high map (TTBR1) is ready for when init makes syscalls
    // // This never returns
    // switch_to_domain(init_domain, init_time);
    endless_sleep()
}
/*
// ─────────────────────────────────────────────────────────────────────
// Create init domain from boot module
// ─────────────────────────────────────────────────────────────────────

/// Create the init domain from a loaded ELF module
fn create_init_domain(module: &LoadedModule, untyped_list: &mut UntypedList) -> DomainRef {
    // ─────────────────────────────────────────────────────────────────────
    // Allocate domain kernel structures
    // ─────────────────────────────────────────────────────────────────────

    // Take an untyped for domain structures
    let domain_untyped = untyped_list
        .take_of_size(DOMAIN_STRUCT_SIZE)
        .expect("No memory for init domain");

    let domain = Domain::create_from_untyped(
        domain_untyped,
        DomainId(1), // Init is domain 1 (0 is idle)
        "init",
    );

    // ─────────────────────────────────────────────────────────────────────
    // Create init's TTBR0 page tables
    // ─────────────────────────────────────────────────────────────────────

    // Allocate page table memory from untyped
    let pt_untyped = untyped_list
        .take_of_size(PAGE_TABLE_SIZE)
        .expect("No memory for init page tables");

    let page_tables = UserPageTables::create_from_untyped(pt_untyped);
    domain.set_page_tables(page_tables);

    // ─────────────────────────────────────────────────────────────────────
    // Parse ELF and create address space
    // ─────────────────────────────────────────────────────────────────────

    // Module is loaded in physical memory, accessible via kernel linear map
    let elf_data = unsafe {
        let virt = phys_to_kernel_virt(module.phys_start);
        core::slice::from_raw_parts(virt as *const u8, module.size)
    };

    let elf = Elf64::parse(elf_data).expect("Invalid ELF");

    semi_println!("  ELF entry point: {:#x}", elf.entry_point());

    // Map each loadable segment
    for phdr in elf.program_headers() {
        if phdr.p_type != PT_LOAD {
            continue;
        }

        let virt_start = VirtAddr::new(phdr.p_vaddr);
        let virt_end = virt_start + phdr.p_memsz as u64;
        let file_size = phdr.p_filesz as usize;
        let mem_size = phdr.p_memsz as usize;

        // Determine page flags from ELF flags
        let flags = elf_flags_to_page_flags(phdr.p_flags);

        semi_println!(
            "  Segment: {:#x} - {:#x} ({:?})",
            virt_start.as_u64(),
            virt_end.as_u64(),
            flags
        );

        // Allocate physical pages for this segment
        let pages_needed = mem_size.div_ceil(PAGE_SIZE);
        let segment_untyped = untyped_list
            .take_of_size(pages_needed * PAGE_SIZE)
            .expect("No memory for init segment");

        // Map pages into init's address space
        let phys_base = segment_untyped.phys_addr();
        domain
            .page_tables()
            .map_range(virt_start, phys_base, pages_needed, flags);

        // Copy segment data
        let src = &elf_data[phdr.p_offset as usize..][..file_size];
        let dst = unsafe {
            let virt = phys_to_kernel_virt(phys_base);
            core::slice::from_raw_parts_mut(virt as *mut u8, mem_size)
        };
        dst[..file_size].copy_from_slice(src);

        // Zero BSS portion
        write_bytes(&mut dst[file_size..], 0, dst.len() - file_size);

        // Create BufferCap for this region and add to init's cspace -- FIXME: This is how we pass the process images to init domain
        let buffer_cap = BufferCap::create_from_untyped(segment_untyped, flags.into());
        domain.cspace().insert_at_next_free(buffer_cap);
    }

    // ─────────────────────────────────────────────────────────────────────
    // Create init's stack
    // ─────────────────────────────────────────────────────────────────────

    const INIT_STACK_SIZE: usize = 64 * 1024; // 64KB
    const INIT_STACK_TOP: u64 = 0x7FFF_FFFF_0000; // should be dynamic..

    let stack_untyped = untyped_list
        .take_of_size(INIT_STACK_SIZE)
        .expect("No memory for init stack");

    domain.page_tables().map_range(
        VirtAddr::new(INIT_STACK_TOP - INIT_STACK_SIZE as u64),
        stack_untyped.phys_addr(),
        INIT_STACK_SIZE / PAGE_SIZE,
        PageFlags::USER_RW,
    );

    semi_println!(
        "  Stack: {:#x} - {:#x}",
        INIT_STACK_TOP - INIT_STACK_SIZE as u64,
        INIT_STACK_TOP
    );

    // ─────────────────────────────────────────────────────────────────────
    // Setup initial register state
    // ─────────────────────────────────────────────────────────────────────

    domain.set_entry_point(VirtAddr::new(elf.entry_point()));
    domain.set_stack_pointer(VirtAddr::new(INIT_STACK_TOP));

    // ─────────────────────────────────────────────────────────────────────
    // Create init's initial capability space
    // ─────────────────────────────────────────────────────────────────────

    setup_init_cspace(&domain);

    domain
}

// ─────────────────────────────────────────────────────────────────────
// Create domain's initial capability space
// ─────────────────────────────────────────────────────────────────────

/// Setup init's capability space with well-known slots
fn setup_init_cspace(domain: &DomainRef) {
    let cspace = domain.cspace();

    // Slot 0: NULL (always invalid)
    // Slot 1: Self domain cap
    cspace.insert(CSPACE_SLOT_SELF, domain.self_cap());

    // Slot 2: Parent domain cap (for init, this is invalid/null)
    // Slot 3: Current TimeCap (kernel sets this on activation)

    // Slots 0x10-0x1F: Will be filled with notification caps
    // Slots 0x20-0x2F: Will be filled with event count caps
    // etc. per the CSpace layout

    // Create a notification for init to receive kernel events
    let kernel_notify = NotifyCap::create();
    cspace.insert(CSPACE_SLOT_KERNEL_NOTIFY, kernel_notify);
}

// ─────────────────────────────────────────────────────────────────────
// Delegate all remaining untypeds to init
// ─────────────────────────────────────────────────────────────────────

/// Delegate all untyped caps to init's cspace
fn delegate_untypeds_to_init(init_domain: &DomainRef, untyped_list: UntypedList) {
    let cspace = init_domain.cspace();

    // Start placing untypeds at well-known slot range
    const UNTYPED_SLOT_START: u32 = 0x1000;
    let mut slot = UNTYPED_SLOT_START;

    for untyped in untyped_list.into_iter() {
        // Create capability referencing this untyped
        let cap = UntypedCap::new(untyped);
        cspace.insert(slot, cap);
        slot += 1;
    }

    // Store count so init knows how many it received
    init_domain.dcb_mut().untyped_cap_count = slot - UNTYPED_SLOT_START;

    semi_prinln!(
        "  Delegated {} untyped caps to slots {:#x}-{:#x}",
        slot - UNTYPED_SLOT_START,
        UNTYPED_SLOT_START,
        slot - 1
    );
}

/// Create caps for boot modules and delegate to init
fn delegate_module_caps_to_init(init_domain: &DomainRef, boot_info: &BootInfo) {
    let cspace = init_domain.cspace();

    const MODULE_SLOT_START: u32 = 0x2000;
    let mut slot = MODULE_SLOT_START;

    for i in 0..boot_info.module_count {
        let module = &boot_info.modules[i];

        // Skip the init module itself (already loaded)
        let name = core::str::from_utf8(&module.name).unwrap_or("");
        if name.matches("init") {
            continue;
        }

        // Create a read-only buffer cap for the module's memory - FIXME: this is how process images are passed on
        let module_cap =
            BufferCap::create_for_phys_range(module.phys_start, module.size, Rights::READ);

        cspace.insert(slot, module_cap);

        // Also store module metadata in a well-known location
        // (Init can query its DCB for module info)

        semi_println!("  Module '{}' at slot {:#x}", name, slot);
        slot += 1;
    }

    init_domain.dcb_mut().module_cap_count = slot - MODULE_SLOT_START;
}

/// Mark init thread memory as reclaimable
/// TODO: this memory should've been first removed from the DTB memory map!
fn mark_init_memory_reclaimable(boot_info: &BootInfo, untypeds_list: ) {
    // The init thread area is entirely one blob of reclaimable memory. Convert it to untyped and donate to init domain.

    // Also mark .init sections in kernel image
    extern "C" {
        static __init_start: u8;
        static __init_end: u8;
    }

    unsafe {
        let init_start = &__init_start as *const _ as u64;
        let init_end = &__init_end as *const _ as u64;
        let init_size = (init_end - init_start) as usize;

        if init_size > 0 {
            untypeds_list.push(ReclaimableRegion {
                phys: kernel_virt_to_phys(VirtAddr::new(init_start)),
                size: init_size,
                kind: ReclaimableKind::InitCode,
            });

            semi_println!(
                "  Init code memory {:#x}-{:#x} ({init_size} bytes) reclaimed",
                init_start,
                init_end
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// Switch to init domain
// ─────────────────────────────────────────────────────────────────────

/// Final step: switch to init domain with initial time budget
fn switch_to_domain(domain: DomainRef, time: TimeCap) -> ! {
    // Update domain state
    {
        let dcb = domain.dcb_mut();
        dcb.state
            .store(DomainState::Running as u32, Ordering::Release);
        dcb.time_remaining_ns
            .store(time.remaining_us() * 1000, Ordering::Relaxed);
        dcb.activation_count.fetch_add(1, Ordering::Relaxed);
    }

    // Set current time cap in init's cspace
    domain.cspace().insert(CSPACE_SLOT_CURRENT_TIME, time);

    // Setup TTBR0 for user space
    let ttbr0 = domain.page_tables().root_phys();

    // Get entry context
    let entry_point = domain.entry_point();
    let stack_pointer = domain.stack_pointer();

    // Record this as current domain -- why percpu??
    percpu::set_current_domain(domain);

    semi_println!(
        "Entering init at {:#x} with SP={:#x}",
        entry_point.as_u64(),
        stack_pointer.as_u64()
    );

    // Do the context switch - this never returns
    unsafe {
        context_switch_to_user(ttbr0, entry_point, stack_pointer);
    }
}

/// Low-level context switch to user mode
#[unsafe(naked)]
unsafe extern "C" fn context_switch_to_user(ttbr0: PhysAddr, entry: VirtAddr, sp: VirtAddr) -> ! {
    core::arch::naked_asm!(
        // Set user page tables (TTBR0)
        "msr ttbr0_el1, x0",
        "isb",
        // Invalidate TLB for ASID 0 (init)
        "tlbi aside1is, xzr",
        "dsb sy",
        "isb",
        // Set up ELR (return address) and SPSR (return state)
        "msr elr_el1, x1",
        // SPSR: EL0t, all interrupts enabled
        "mov x3, #0", // EL0t
        "msr spsr_el1, x3",
        // Set user stack pointer
        "msr sp_el0, x2",
        // Clear all general-purpose registers for security
        "mov x0, #0",
        "mov x1, #0",
        "mov x2, #0",
        "mov x3, #0",
        "mov x4, #0",
        "mov x5, #0",
        "mov x6, #0",
        "mov x7, #0",
        "mov x8, #0",
        "mov x9, #0",
        "mov x10, #0",
        "mov x11, #0",
        "mov x12, #0",
        "mov x13, #0",
        "mov x14, #0",
        "mov x15, #0",
        "mov x16, #0",
        "mov x17, #0",
        "mov x18, #0",
        "mov x19, #0",
        "mov x20, #0",
        "mov x21, #0",
        "mov x22, #0",
        "mov x23, #0",
        "mov x24, #0",
        "mov x25, #0",
        "mov x26, #0",
        "mov x27, #0",
        "mov x28, #0",
        "mov x29, #0",
        "mov x30, #0",
        // Enter user mode
        "eret",
        options(noreturn),
    );
}
*/

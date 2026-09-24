#![no_std]
#![no_main]
#![allow(unused)]
#![feature(format_args_nl)]

//! The real Vesper startup kernel: initialize the machine, build the initial
//! kernel state, and — as the boot path grows — bring up the whole system.
//! The e2e runtime/bootup tests that used to live here moved to the separate
//! `kicktest` kernel, which reuses the shared boot code from the `kickstart`
//! library crate.

// Init-thread process.
// - Start initializing the kernel
// - Enter itself into process list as high-priority privileged process
// - The bootup is driven by the kickstart which loads and parses devtree, maps the kernel, gives itself all necessary
//   capabilities, (probably loads more things into their places) and transitions to user mode.
// - From user mode it can continue distributing capabilities and launching servers until everything is handed out.
// - After init is completed, it should create more low-priority user processes including idle, fs, some other handlers and
//   a user-space boot process like /sbin/init or sth, with scripts to control the boot up.
// - At this point, exit the kickstart process normally.
// - The init-thread can be terminated and its memory freed up (should it inject a process descriptor for itself somehow to allow normal shutdown mechanisms to clean up? most probably).

// create "tracing" and "debug" components for kernel call keys (intercepting syscall caps)

// Kernel's main.rs just brings together all libs and syscall entry points.
// kickstart.rs provides a boot up entry point which sets up everything.

// kickstart should do some shared init
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

use {
    cfg_if::cfg_if,
    kickstart::{
        bootstrap::{PoolCapacities, bootstrap_nucleus},
        init_main_el2, print_my_sp,
    },
    libboot as boot,
    libcpu::endless_sleep,
    libqemu::semihosting as semi,
};

boot::entry!(boot_main);

/// EL2 entry: run the shared boot through the EL1 transition, then continue
/// in [`kickstart_run`].
fn boot_main(dtb: u32) -> ! {
    init_main_el2(dtb, kickstart_run as *const u8 as u64)
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    semi::println!("PANICKED: {info}");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_failure()
        } else {
            endless_sleep()
        }
    }
}

// DTB should be available to this code through BOOT_INFO records.
pub fn kickstart_run() -> ! {
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // PHASE 5: Initialize kernel objects and structures
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    // Run initial thread further in EL1, setting up the capDL etc.
    semi::println!("init_main_run: enabled MMU and dropped to EL1");
    print_my_sp();

    // ─────────────────────────────────────────────────────────────────────
    // Build the initial kernel state in carved memory (inert nucleus).
    // ─────────────────────────────────────────────────────────────────────

    // The real boot's own extents: the boot Thread, its AddressSpace, and the
    // boot ASID pool. Everything else is carved at runtime through Retype.
    let _boot_state = bootstrap_nucleus(&PoolCapacities {
        threads: 1,
        address_spaces: 1,
        notifications: 0,
        event_counts: 0,
        page_tables: 0,
        asid_pools: 1,
    });

    // ─────────────────────────────────────────────────────────────────────
    // Initialize kernel subsystems
    // ─────────────────────────────────────────────────────────────────────

    // semi::println!("Initializing kernel subsystems...");

    // Initialize per-CPU data structures
    // percpu::init();

    // Initialize interrupt controller (GIC on RPi4)
    // let boot_info = unsafe { &BOOT_INFO };
    // interrupts::init_gic(boot_info);

    // ─────────────────────────────────────────────────────────────────────
    // Build physical memory map and create Untyped caps
    // ─────────────────────────────────────────────────────────────────────

    // semi::println!("Building physical memory allocator...");

    // Create the root untyped capability list
    // This will be delegated to init process
    // let mut untyped_list = UntypedList::new();
    // // let mut untyped_list = create_untyped_caps();

    // for i in 0..boot_info.memory_region_count {
    //     let region = &boot_info.memory_regions[i];

    //     // Skip reserved regions (kernel image, DTB, modules, page tables)
    //     let usable_ranges = subtract_reserved_regions(region, boot_info);

    //     for range in usable_ranges {
    //         // Create untyped caps for each usable chunk
    //         // Align to largest power-of-2 for efficient retyping
    //         let untypeds = create_untyped_caps_for_range(range);
    //         untyped_list.extend(untypeds);
    //         semi::println!(
    //             "  Untyped: {:#x} - {:#x} ({} caps)",
    //             range.base.as_u64(),
    //             range.base.as_u64() + range.size as u64,
    //             untypeds.len()
    //         );
    //     }
    // }

    // semi::println!("Total untyped caps: {}", untyped_list.len());

    // ─────────────────────────────────────────────────────────────────────
    // Initialize DCB shared pages
    // ─────────────────────────────────────────────────────────────────────

    // semi::println!("Initializing DCB pages...");

    // // Allocate DCB pages from a reserved untyped
    // // These are special: mapped RW in kernel, RO in all user domains
    // let dcb_pages = allocate_dcb_pages(&mut untyped_list, MAX_DOMAINS);
    // dcb::init(dcb_pages);

    // ─────────────────────────────────────────────────────────────────────
    // Create kernel idle domain (domain 0)
    // ─────────────────────────────────────────────────────────────────────

    // semi::println!("Creating idle domain...");

    // let idle_domain = Domain::create_idle();
    // SCHEDULER.set_idle(idle_domain);

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // PHASE 6: Create the init domain and its capability space
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    // semi::println!("Creating init domain...");

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

    // semi::println!("Marking init thread memory for reclamation...");

    // // The init stack and any init-only code/data can now be reclaimed, the are in the Untypeds table now.
    // mark_init_memory_reclaimable(boot_info, &mut untyped_list);

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // PHASE 7: Delegate all resources to init domain
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    // semi::println!(
    //     "Delegating {} untyped caps to init...",
    //     untyped_list.len()
    // );

    // delegate_untypeds_to_init(&init_domain, untyped_list);

    // // Create module caps for other boot modules and delegate
    // delegate_module_caps_to_init(&init_domain, boot_info);

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // PHASE 8: Context switch to init domain
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    let (_, privilege_level) = libexception::current_privilege_level();
    liblog::info!("Current privilege level: {privilege_level}");

    liblog::info!("Exception handling state:");
    libexception::asynchronous::print_state();

    // semi::println!("Switching to init domain...");
    // semi::println!("═══════════════════════════════════════════════════════════");

    // // Create initial time budget for init
    // let init_time = TimeCap::create_root(INIT_TIME_BUDGET_US);

    // // Finally: switch to userspace init
    // // This replaces TTBR0 with init's page tables
    // // Kernel high map (TTBR1) is ready for when init makes syscalls
    // // This never returns
    // switch_to_domain(init_domain, init_time);
    print_my_sp();

    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            endless_sleep()
        }
    }

    // kernel_init_mmio_va_allocator()

    // SAFETY: Not safe!
    // if let Err(x) = unsafe { libplatform::drivers::init() } {
    //     panic!("Error initializing platform drivers: {}", x);
    // }

    // Initialize all device drivers.
    // SAFETY: Not safe!
    // unsafe {
    //     libplatform::drivers::driver_manager().init_drivers_and_irqs();
    // }

    // Unmask interrupts on the boot CPU core.
    // libexception::exception::asynchronous::local_irq_unmask();

    // Announce conclusion of the kernel_init() phase.
    // libkernel_state::state_manager().transition_to_single_core_main();

    // libconsole::init_logger();

    // info!("{}", libkernel::version());

    // info!(
    //     "{} version {}",
    //     env!("CARGO_PKG_NAME"),
    //     env!("CARGO_PKG_VERSION")
    // );

    // info!(
    //     "Booting on: {}",
    //     libplatform::BcmHost::board_name()
    // );

    // info!("MMU online. Special regions:");
    // machine::platform::memory::mmu::virt_mem_layout().print_layout();

    // dump_memory_map();

    // info!(
    //     "Architectural timer resolution: {} ns",
    //     libtime::time::time_manager().resolution().as_nanos()
    // );

    // info!("Drivers loaded:");
    // libplatform::drivers::driver_manager().enumerate();

    // info!("Registered IRQ handlers:");
    // libplatform::exception::asynchronous::irq_manager().print_handler();

    // // Test a failing timer case.
    // libtime::time::time_manager().spin_for(Duration::from_nanos(1));

    // for _ in 0..3 {
    //     info!("Spinning for 1 second");
    //     libtime::time::time_manager().spin_for(Duration::from_secs(1));
    // }
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

    semi::println!("  ELF entry point: {:#x}", elf.entry_point());

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

        semi::println!(
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

        // Pass the segment region to init as Untyped/Frame capabilities -- FIXME: This is how we pass
        // the process images to init domain. Buffer is a userspace/libOS construct (selected
        // 2026-09-15): the kernel hands over memory via Untyped/Frame capabilities only.
        // let segment_cap = FrameCap::create_from_untyped(segment_untyped, flags.into());
        // domain.cspace().insert_at_next_free(segment_cap);
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

    semi::println!(
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

        // Pass the module's memory to init as a read-only Untyped/Frame capability -- FIXME: this
        // is how process images are passed on. Buffer is a userspace/libOS construct (selected
        // 2026-09-15): the kernel hands over memory via Untyped/Frame capabilities only.
        // let module_cap =
        //     FrameCap::create_for_phys_range(module.phys_start, module.size, Rights::READ);

        // cspace.insert(slot, module_cap);

        // Also store module metadata in a well-known location
        // (Init can query its DCB for module info)

        semi::println!("  Module '{}' at slot {:#x}", name, slot);
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

            semi::println!(
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

    semi::println!(
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

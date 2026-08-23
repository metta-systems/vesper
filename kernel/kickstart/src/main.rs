#![no_std]
#![no_main]
#![allow(unused)]
#![feature(format_args_nl)]
#![feature(try_find)] // For DeviceTree iterators

// Init-thread process.
// - Start initializing the kernel
// - Enter itself into process list as high-priority privileged process
// - The bootup is driven by the kickstart which loads and parses devtree, maps the kernel, gives itself all necessary
// capabilities, (probably loads more things into their places) and transitions to user mode.
// - From user mode it can continue distributing capabilities and launching servers until everything is handed out.
// - After init is completed, it should create more low-priority user processes including idle, fs, some other handlers and
// a user-space boot process like /sbin/init or sth, with scripts to control the boot up.
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

mod boot_info;
mod device_tree;
mod el_switch;
mod embed;
mod loader;
mod memory;
mod paging;
mod qsort;

use {
    crate::{boot_info::BOOT_INFO, memory::Alloc},
    aarch64_cpu::registers::{SPSR_EL2, Writeable},
    core::{cell::UnsafeCell, panic::PanicInfo, ptr::write_bytes, slice},
    device_tree::{DeviceTree, DeviceTreeProp},
    fdt_rs::{
        base::DevTree,
        error::DevTreeError,
        prelude::{FallibleIterator, PropReader},
    },
    libaddress::{PhysAddr, VirtAddr},
    libboot as boot,
    libcpu::endless_sleep,
    liblocking::interface::Mutex,
    libmapping::{AccessPermissions, AttributeFields, MemAttributes},
    libobject::{DebugConsoleKey, KeySlot},
    libqemu::semihosting as semi,
    libsyscall::protected_call6,
    memory::BootAllocator,
};

unsafe extern "C" {
    static __INIT_START: UnsafeCell<()>;
    static __INIT_END: UnsafeCell<()>;
    static __FREE_MEMORY_START: UnsafeCell<()>;
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    semi::println!("PANICKED: {info}");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_failure()
        } else {
            endless_sleep()
        }
    }
}

fn dump_memory_map() {
    // Output the memory map as we could derive from FDT and information about our loaded image
    // Use it to imagine how the memmap would look like in the end.
    BOOT_INFO.lock(|bi| {
        bi.compact();
        bi.sort();
        bi.dump();
    });
}

boot::entry!(init_main_el2);

/// Kernel early init code.
/// `arch` crate is responsible for calling it.
///
/// Safety
///
/// - Only a single core must be active and running this function.
/// - The init calls in this function must appear in the correct order:
///     - MMU + Data caching must be activated at the earliest. Without it, any atomic operations,
///       e.g. the yet-to-be-introduced spinlocks in the device drivers (which currently employ
///       `IRQSafeNullLocks` instead of spinlocks), will fail to work (properly) on the `RPi` `SoCs`.
///
pub fn init_main_el2(dtb: u32) -> ! {
    let dtb_ptr = dtb as *const u8;

    SPSR_EL2.write(
        SPSR_EL2::D::Masked
            + SPSR_EL2::A::Masked
            + SPSR_EL2::I::Masked
            + SPSR_EL2::F::Masked
            + SPSR_EL2::M::EL1h, // Use SP_EL1/2
    );

    #[cfg(feature = "jtag")]
    libmachine::debug::jtag::wait_debugger();

    semi::println!("init_main started");

    // unsafe {
    //     BOOT_INFO.dtb_phys = PhysAddr::new(dtb_phys);
    // }

    // ─────────────────────────────────────────────────────────────────────
    // Initialize early UART for debug output
    // ─────────────────────────────────────────────────────────────────────

    // Hardcoded UART address for early boot (RPi4: 0xFE201000)
    // Will be properly mapped later
    // early_uart_init(0xFE20_1000);
    semi::println!("DTB at physical: {:#016x}", dtb_ptr as u64);

    // ─────────────────────────────────────────────────────────────────────
    // Start bump allocator
    // ─────────────────────────────────────────────────────────────────────

    // SAFETY: Unsafe
    let init_start = unsafe { __INIT_START.get() as u64 };
    // SAFETY: Unsafe
    let init_end = unsafe { __INIT_END.get() as u64 };
    // SAFETY: Unsafe
    let free_start = unsafe { __FREE_MEMORY_START.get() as u64 };

    let memory_size = 256 * 1024 * 1024;
    let mut allocator = BootAllocator::new(PhysAddr::new(free_start), memory_size);
    let memory_end = allocator.end();
    semi::println!(
        "init_main: Created BootAllocator {memory_size} @ {:#016x}",
        free_start
    );

    // ─────────────────────────────────────────────────────────────────────
    // Parse Device Tree
    // ─────────────────────────────────────────────────────────────────────

    semi::println!("Parsing device tree...");

    // Safety: we got the address from the bootloader, if it lied - well, we're screwed!
    let device_tree =
        unsafe { DevTree::from_raw_pointer(dtb_ptr).expect("DeviceTree failed to read") };

    let layout = DeviceTree::layout(device_tree).expect("Couldn't calculate DeviceTree index");

    let block = allocator
        .alloc_aligned(
            layout.size(),
            layout.align(),
            ("DTB index", Alloc::Droppable),
        )
        .expect("Couldn't allocate DeviceTree index");
    // SAFETY: Unsafe call.
    let raw_slice = unsafe { core::slice::from_raw_parts_mut(block.as_mut_ptr(), layout.size()) };

    let device_tree =
        DeviceTree::new(device_tree, raw_slice).expect("Couldn't initialize indexed DeviceTree");

    let board = device_tree.get_prop_by_path("/model").unwrap().str();
    if let Ok(board_name) = board {
        semi::println!("Running on {board_name}");
    }

    // let mut dumper = device_tree.dumper(0);
    // dumper.dump_metadata();
    // dumper.dump_root().expect("oof");

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

    let res: Result<_, DevTreeError> = device_tree
        .props()
        .try_find(|p| Ok(p.name()? == "device_type" && p.str()? == "memory"));
    let mem_prop = res.unwrap().expect("Unable to find memory node.");
    let _mem_node = mem_prop.node();
    // let parent_node = mem_node.parent_node();

    // reg == region, usually defines LOC+SIZE unless #size-cells is set to 0
    // can also be reg = <0x7e100000 0x00000114 0x7e00a000 0x00000024 >; to define two locations
    let reg_prop = device_tree
        .get_prop_by_path("/memory@0/reg")
        .expect("Unable to figure out memory-reg");

    semi::println!(
        "Found memnode with reg prop: name {:?}, size {}",
        reg_prop.name(),
        reg_prop.length()
    );

    let reg_prop = DeviceTreeProp::new(reg_prop);

    let mut total_memory = 0;

    for (mem_addr, mem_size) in reg_prop.payload_pairs_iter() {
        semi::println!("Memory: {} KiB at offset {}", mem_size / 1024, mem_addr);
        total_memory += mem_size;
        BOOT_INFO.lock(|bi| {
            bi.insert_free_region(
                PhysAddr::new(mem_addr),
                PhysAddr::new(mem_addr + mem_size),
                AttributeFields::default(),
                "RAM",
            )
            .expect("tough luck");
        });
    }

    // 4. List unusable memory, and remove it from the memory regions for the allocator.
    for entry in device_tree.fdt().reserved_entries() {
        let size: u64 = entry.size.into();
        let address: u64 = entry.address.into();
        semi::println!("Reserved memory: {size:?} bytes at {address:?}");
        BOOT_INFO.lock(|bi| {
            bi.insert_used_region(
                PhysAddr::new(entry.address.into()),
                PhysAddr::new(u64::from(entry.address) + u64::from(entry.size)),
                AttributeFields::default(),
                "Reserved",
            )
            .expect("tough luck");
        });
    }

    // 5. Also list memreserve entries, and remove then from allocator regions?
    // From FDT dump:
    //   memreserve = <0x3b400000 0x04c00000 >;

    // Iterate compatible nodes (example):
    for entry in device_tree.compatible_nodes("arm,pl011") {
        semi::println!("PL011 device: {:?}", entry.name() /*, entry.address*/);
    }

    // 6. Also, remove the DTB memory region + index
    semi::println!(
        "DTB region: {} bytes at {:#016x}",
        device_tree.fdt().totalsize(),
        dtb_ptr as usize
    ); // also include the raw_slice allocated bit
    BOOT_INFO.lock(|bi| {
        bi.insert_used_region(
            PhysAddr::new(dtb_ptr as u64),
            PhysAddr::new(dtb_ptr as u64 + device_tree.fdt().totalsize() as u64),
            AttributeFields {
                droppable: true,
                ..Default::default()
            },
            "DTB",
        )
        .expect("tough luck");
    });

    // Next step: parse DTB!
    // iterate nodes, look for reg, status, compat props

    #[expect(clippy::items_after_statements)]
    #[derive(Default, Copy, Clone)]
    struct Node {
        name: &'static str,
        compat: &'static str,
        phandle: u32,
        disabled: bool,
        start: u64,
        size: u64,
    }

    let mut nodes: [Node; 100] = [Node::default(); 100];
    let mut num_nodes = 0;

    // See https://mjmwired.net/kernel/Documentation/devicetree/bindings/display/brcm,bcm-vc4.txt
    // All these DT thingies are Broadcom= and Linux-specific, so need to read both to decode anything useful.
    // https://mjmwired.net/kernel/Documentation/devicetree/usage-model.rst <- entry point

    // The trick is that the kernel starts at the root of the tree and looks
    // for nodes that have a 'compatible' property.  First, it is generally
    // assumed that any node with a 'compatible' property represents a device
    // of some kind, and second, it can be assumed that any node at the root
    // of the tree is either directly attached to the processor bus, or is a
    // miscellaneous system device that cannot be described any other way.
    // For each of these nodes, Linux allocates and registers a
    // platform_device, which in turn may get bound to a platform_driver.

    // Gather and print the following info: reg (start + size) x times, phandle if any, name, compat
    // Sort them by start address to get ordered device map.
    //
    // e.g.:
    // mmcnr@7e300000 @ 0x7e300000 +0x100 ("brcm,bcm2835-mmc", "brcm,bcm2835-sdhci")
    // [0x2f] mmc@7e300000 @ 0x7e300000 +0x100 ("brcm,bcm2835-mmc", "brcm,bcm2835-sdhci")
    //
    // To add later: clocks and interrupts, if any
    // Print "-" prefix is status = disabled;

    for entry in device_tree.nodes() {
        if let Some(item) = entry.props().find(|p| p.name() == Ok("reg")) {
            let compat_names = entry
                .props()
                .find(|p| p.name() == Ok("compatible"))
                .and_then(|prop| prop.str().ok())
                .unwrap_or("");
            let phandle = entry
                .props()
                .find(|p| p.name() == Ok("phandle"))
                .and_then(|prop| prop.phandle(0).ok());
            let disabled = entry
                .props()
                .find(|p| p.name() == Ok("status"))
                .and_then(|prop| prop.str().ok())
                .is_some_and(|value| value == "disabled");
            let name = entry.name().unwrap();
            let name = name.split_once('@').unwrap_or((name, "")).0;

            let reg_prop = DeviceTreeProp::new(item);
            for (mem_base, mem_size) in reg_prop.payload_pairs_iter() {
                nodes[num_nodes] = Node {
                    start: mem_base,
                    size: mem_size,
                    name,
                    compat: compat_names,
                    disabled,
                    phandle: phandle.unwrap_or_default(),
                };
                num_nodes += 1;
            }
        }
    }

    let mut nodes = &mut nodes[..num_nodes];

    // Other in-place sorting available:
    if !nodes.is_sorted_by_key(|item| item.start) {
        nodes.sort_unstable_by_key(|item| item.start);
    }

    for node in nodes {
        semi::println!(
            "{}[{:02x}] {:<22} @ {} +{} ({})",
            if node.disabled { "-" } else { " " },
            node.phandle,
            node.name,
            PhysAddr::new(node.start),
            node.size,
            node.compat
        );

        if node.name != "memory" && node.name != "gpio" && node.name != "mmc" && node.name != "smi"
        {
            BOOT_INFO.lock(|bi| {
                bi.insert_used_region(
                    PhysAddr::new(node.start),
                    PhysAddr::new(node.start + node.size),
                    AttributeFields {
                        mem_attributes: MemAttributes::Device,
                        ..AttributeFields::default()
                    },
                    node.name,
                )
                .expect("tough luck");
            });
        }
    }

    semi::println!();
    semi::println!();

    for entry in device_tree.nodes() {
        if entry.name() == Ok("chosen") {
            semi::println!("Found /chosen node");
        }
    }

    // unsafe {
    //     BOOT_INFO.dtb_size = dtb.total_size();

    //     // Extract memory regions
    //     for region in dtb.memory_regions() {
    //         if BOOT_INFO.memory_region_count < 16 {
    //             BOOT_INFO.memory_regions[BOOT_INFO.memory_region_count] = region;
    //             BOOT_INFO.memory_region_count += 1;
    //             semi::println!(
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
    //             semi::println!(
    //                 "  Module '{}': {:#x}, {} bytes",
    //                 core::str::from_utf8(&module.name).unwrap_or("???"),
    //                 module.phys_start.as_u64(),
    //                 module.size
    //             );
    //         }
    //     }
    // }

    // ═══════════════════════════════════════════════════════════════
    // PHASE 1: Load kernel
    // ═══════════════════════════════════════════════════════════════

    semi::println!("init_main: Load nucleus");

    let kernel_layout = loader::load_kernel(&mut allocator).expect("Failed to load nucleus");
    semi::println!("init_main: Loaded nucleus image");

    // ═══════════════════════════════════════════════════════════════
    // PHASE 2: Set up page tables
    // ═══════════════════════════════════════════════════════════════

    let (el1_stack, el1_stack_size) = {
        // Allocate EL1 stack
        let el1_stack_size = 128; // pages
        let el1_stack = allocator
            .alloc_pages(el1_stack_size, ("Nucleus stack", Alloc::Persistent))
            .expect("Failed to allocate EL1 stack");
        let el1_stack_size = el1_stack_size * 4096; // 64KiB stack
        (el1_stack, el1_stack_size)
    };

    // Mark kernel memory used:
    // TODO: add alignment requirements to boot_info regions (align up to)
    BOOT_INFO.lock(|bi| {
        for sec in kernel_layout.iter_sections() {
            bi.insert_used_region(
                sec.phys_start,
                sec.phys_start + sec.size,
                AttributeFields {
                    acc_perms: if sec.permissions.writable {
                        AccessPermissions::ReadWrite
                    } else {
                        AccessPermissions::ReadOnly
                    },
                    executable: sec.permissions.executable,
                    ..Default::default()
                },
                sec.name,
            );
        }
        bi.insert_used_region(
            kernel_layout.bss_phys,
            kernel_layout.bss_phys + kernel_layout.bss_size,
            AttributeFields::defaulted(),
            "Nucleus BSS",
        );
        bi.insert_used_region(
            el1_stack,
            el1_stack + el1_stack_size,
            AttributeFields::defaulted(),
            "Nucleus stack",
        );
    });

    let mut mmu_setup = paging::MmuSetup::new(&mut allocator).expect("Failed to create MMU setup");
    semi::println!("init_main: Created MmuSetup");

    // Identity map kickstart
    paging::create_identity_mapping(&mut mmu_setup, PhysAddr::new(init_start), memory_end)
        .expect("Failed to create identity mapping");
    semi::println!("init_main: Identity mapped the Kickstart");

    // Create kernel mapping with per-section permissions
    let (el1_stack_top,) = paging::create_kernel_mapping(
        &mut mmu_setup,
        &kernel_layout,
        total_memory,
        el1_stack.as_u64(),
        el1_stack_size,
    )
    .expect("Failed to create kernel mapping");
    semi::println!("init_main: Higher-half mapped the nucleus");

    // ═══════════════════════════════════════════════════════════════
    // Interlude: Print the BOOT_INFO region map
    // ═══════════════════════════════════════════════════════════════

    BOOT_INFO.lock(|bi| {
        bi.insert_overlay_region(
            PhysAddr::new(init_start),
            mmu_setup.memory_top(), // Up to allocated watermark
            AttributeFields {
                droppable: true,
                ..Default::default()
            },
            "Kickstart",
        );
    });

    semi::println!("init_main: BOOT_INFO map after kernel load and mapping");
    dump_memory_map();

    // ═══════════════════════════════════════════════════════════════
    // PHASE 3: Prepare for EL1
    // ═══════════════════════════════════════════════════════════════

    let ttbr0 = mmu_setup.ttbr0();
    let ttbr1 = mmu_setup.ttbr1();
    semi::println!("init_main: TTBR0_EL1 at {ttbr0:#016x}, TTBR1_EL1 at {ttbr1:#016x}");

    // Get vector table virtual address for VBAR_EL1
    // VBAR is only used after MMU is enabled, so we set the virtual address directly
    let vbar = kernel_layout.vbar_el1_virt();

    semi::println!("init_main: EL1 stack at {el1_stack_top:#016x}, vbar {vbar:#016x}");

    // ═══════════════════════════════════════════════════════════════
    // PHASE 4: Enable MMU and drop to EL1
    // ═══════════════════════════════════════════════════════════════

    semi::println!("Init thread image covers phys ?:? identity mapped");
    semi::println!("Init thread mapping tables filled in as ? entries");
    semi::println!("Kernel image covers phys ?:? mapped to KERNEL_HIGH_BASE:?");
    semi::println!("Kernel mapping tables filled in as ? for kernel, as ? for phys memory");

    print_my_sp();

    unsafe extern "Rust" {
        // Stack top
        static __STACK_TOP: UnsafeCell<()>;
    }

    // SAFETY: Not safe.
    unsafe {
        #[expect(clippy::fn_to_numeric_cast_any)]
        el_switch::enable_mmu_and_drop_to_el1(
            ttbr0,
            ttbr1,
            vbar,
            kickstart_run as *const u8 as u64,
            // el1_stack_top, // This is solely for the kernel
            __STACK_TOP.get() as u64,
        );
    }
}

// DTB should be available to this code through BOOT_INFO records.
pub fn kickstart_run() -> ! {
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // PHASE 5: Initialize kernel objects and structures
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    // Run initial thread further in EL1, seting up the capDL etc.
    semi::println!("init_main_run: enabled MMU and dropped to EL1");
    print_my_sp();
    // SAFETY: Not safe.
    unsafe {
        protected_call6(0, 0, 0, 0, 0, 0, 0, 0);
    }
    semi::println!("init_main_run: Returned from fake syscall");
    print_my_sp();

    // ─────────────────────────────────────────────────────────────────────
    // Initialize kernel subsystems
    // ─────────────────────────────────────────────────────────────────────

    semi::println!("Initializing kernel subsystems...");

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

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // PHASE 6: Create the init domain and its capability space
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

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

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // PHASE 7: Delegate all resources to init domain
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    // semi::println!(
    //     "Delegating {} untyped caps to init...",
    //     untyped_list.len()
    // );

    // delegate_untypeds_to_init(&init_domain, untyped_list);

    // // Create module caps for other boot modules and delegate
    // delegate_module_caps_to_init(&init_domain, boot_info);

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // PHASE 8: Context switch to init domain
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    // We have domain caps here, can use:
    let dbg = DebugConsoleKey::new();
    dbg.write(
        "DEBCON| Debug output via capability invocation on domain's debug console capability\n",
    );

    let err = DebugConsoleKey::new_slot(KeySlot::CAPTBL_SELF);
    err.write("DEBCON| Invalid capability invocation - no output");

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
    // if let Err(x) = unsafe { libplatform::platform::drivers::init() } {
    //     panic!("Error initializing platform drivers: {}", x);
    // }

    // Initialize all device drivers.
    // SAFETY: Not safe!
    // unsafe {
    //     libplatform::platform::drivers::driver_manager().init_drivers_and_irqs();
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
    //     libplatform::platform::BcmHost::board_name()
    // );

    // info!("MMU online. Special regions:");
    // machine::platform::memory::mmu::virt_mem_layout().print_layout();

    // dump_memory_map();

    // info!(
    //     "Architectural timer resolution: {} ns",
    //     libtime::time::time_manager().resolution().as_nanos()
    // );

    // info!("Drivers loaded:");
    // libplatform::platform::drivers::driver_manager().enumerate();

    // info!("Registered IRQ handlers:");
    // libplatform::platform::exception::asynchronous::irq_manager().print_handler();

    // // Test a failing timer case.
    // libtime::time::time_manager().spin_for(Duration::from_nanos(1));

    // for _ in 0..3 {
    //     info!("Spinning for 1 second");
    //     libtime::time::time_manager().spin_for(Duration::from_secs(1));
    // }
}

fn print_my_sp() {
    use aarch64_cpu::registers::Readable;
    let sp = aarch64_cpu::registers::SP.get();
    semi::println!("Current SP: {sp:016x}");
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

        // Create a read-only buffer cap for the module's memory - FIXME: this is how process images are passed on
        let module_cap =
            BufferCap::create_for_phys_range(module.phys_start, module.size, Rights::READ);

        cspace.insert(slot, module_cap);

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

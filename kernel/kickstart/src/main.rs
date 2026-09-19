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
mod bootstrap;
mod device_tree;
mod el_switch;
mod embed;
mod loader;
mod memory;
mod paging;
mod qsort;

use {
    crate::{
        boot_info::BOOT_INFO,
        bootstrap::{PoolCapacities, build_initial_nucleus},
        embed::NUCLEUS_SET_ANCHOR_VIRT,
        memory::Alloc,
    },
    aarch64_cpu::registers::{Readable, SPSR_EL2, TTBR0_EL1, Writeable},
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
    libobject::{
        KeySlot, ObjectType, Rights,
        domain::{DomainId, DomainKey},
    },
    libqemu::semihosting as semi,
    memory::BootAllocator,
    nucleus::{
        api::key_entry::KeyEntry,
        objects::{
            ArchObjects, ArchObjectsImpl, Domain, ExecutionContext, KeyTable, Nucleus,
            access::{ObjectId, PoolTag},
            completion::PendingState,
        },
    },
};

#[cfg(feature = "debug_kernel")]
use libobject::{
    ASIDPoolKey, CapError, DebugConsoleKey, EventCountKey, FrameKey, InvalidKeyReason, KeyTableKey,
    NotificationKey, PageTableKey, RawKey, UntypedKey,
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

/// Size (as log2) of the boot Untyped region carved for the initial kernel state.
const BOOT_UNTYPED_SIZE_BITS: u8 = 24; // 16 MiB

// DTB should be available to this code through BOOT_INFO records.
pub fn kickstart_run() -> ! {
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // PHASE 5: Initialize kernel objects and structures
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    // Run initial thread further in EL1, seting up the capDL etc.
    semi::println!("init_main_run: enabled MMU and dropped to EL1");
    print_my_sp();

    // ─────────────────────────────────────────────────────────────────────
    // Build the initial kernel state in carved memory (inert nucleus).
    // ─────────────────────────────────────────────────────────────────────

    // Allocate a power-of-2 boot region for the boot Untyped.
    let boot_region = BOOT_INFO
        .lock(|bi| bi.alloc_region(usize::from(BOOT_UNTYPED_SIZE_BITS), "Boot Untyped"))
        .expect("no free region for the boot Untyped");

    // Create the boot Untyped capability over that region.
    let mut boot_untyped = KeyEntry::new_untyped(
        boot_region.as_u64(),
        BOOT_UNTYPED_SIZE_BITS,
        false,
        Rights::all(),
    );

    // Carve the initial Nucleus + pools from the boot Untyped's watermark.
    let Ok(boot_payload) = boot_untyped.as_region_mut() else {
        panic!("boot Untyped is not a region")
    };
    let Ok((nucleus_ptr, keytable_addr)) = build_initial_nucleus::<ArchObjectsImpl>(
        boot_payload,
        &PoolCapacities {
            domains: 2,
            notifications: 4,
            event_counts: 4,
            page_tables: 16,
            asid_pools: 1,
        },
    ) else {
        panic!("failed to build the initial nucleus")
    };

    // SAFETY: nucleus_ptr points to the freshly carved, exclusively-owned region.
    let nucleus = unsafe { &mut *nucleus_ptr };
    // The boot Domain is the first (index 0) allocation; make it current.
    nucleus.current_domain = Some(0);

    // Allocate the boot Domain in the carved pool; its KeyTable was carved and
    // initialized kernel-privately by build_initial_nucleus.
    let boot_domain_id = nucleus
        .pools
        .domains
        .allocate(Domain {
            keytable_addr,
            translation_root: None,
            asid: None,
            context: ExecutionContext::Running,
        })
        .expect("no boot Domain slot")
        .0;

    // Install the boot Domain's self-table capability, the boot Domain itself
    // (the bootstrap-era mapping context for `PageTable.Map`/`Frame.Map`), and
    // the boot Untyped as the first grants.
    // SAFETY: keytable_addr names the freshly carved, live boot KeyTable.
    let boot_table = unsafe { &mut *(keytable_addr as *mut KeyTable) };
    let self_table_key = boot_table
        .insert(
            KeySlot::CAPTBL_SELF,
            KeyEntry::new_keytable(keytable_addr, Rights::all(), 0),
        )
        .unwrap_or_else(|failure| {
            panic!("boot self-table install failed: {:?}", failure.error.code())
        });
    let boot_untyped_key = boot_table
        .insert(KeySlot::BOOT_UNTYPED, boot_untyped)
        .unwrap_or_else(|failure| {
            panic!("boot Untyped install failed: {:?}", failure.error.code())
        });
    let _boot_domain_key = boot_table
        .insert(
            KeySlot::SELF_DOMAIN,
            KeyEntry::new::<Domain>(boot_domain_id, Rights::all(), 0),
        )
        .unwrap_or_else(|failure| panic!("boot Domain install failed: {:?}", failure.error.code()));

    // Provision the boot ASID pool (seL4-style, selected 2026-09-15): ASIDs
    // are a hardware namespace, not memory-backed, so the pool is carved and
    // initialized kernel-privately here rather than Retype-created. Its
    // capability is the authoritative grant the bootstrap builder assigns
    // hardware translation contexts from.
    let boot_asid_pool_id = nucleus
        .pools
        .arch
        .asid_pools
        .allocate(ArchObjectsImpl::new_asid_pool())
        .expect("no boot ASID-pool slot")
        .0;
    let _boot_asid_pool_key = boot_table
        .insert(
            KeySlot::BOOT_ASID_POOL,
            KeyEntry::new::<<ArchObjectsImpl as ArchObjects>::ASIDPool>(
                boot_asid_pool_id,
                Rights::all(),
                0,
            ),
        )
        .unwrap_or_else(|failure| {
            panic!("boot ASID-pool install failed: {:?}", failure.error.code())
        });

    // Install the debug console grant (debug_kernel) and use it below.
    #[cfg(feature = "debug_kernel")]
    let debug_console_key = boot_table
        .insert(
            KeySlot::DEBUG_CONSOLE,
            KeyEntry::from_id(
                ObjectType::DEBUG_CONSOLE,
                ObjectId {
                    pool: PoolTag::Region,
                    index: 0,
                    generation: 0,
                },
                Rights::all(),
                0,
            ),
        )
        .unwrap_or_else(|failure| {
            panic!("debug console install failed: {:?}", failure.error.code())
        });

    // Record the carved Nucleus address for the inert nucleus.
    // SAFETY: The paired nucleus image is loaded and mapped; the setter is a
    // boot-only one-shot write before any syscall.
    unsafe {
        let setter = core::mem::transmute::<u64, unsafe extern "C" fn(*mut Nucleus<ArchObjectsImpl>)>(
            NUCLEUS_SET_ANCHOR_VIRT,
        );
        setter(nucleus_ptr);
    }
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

    #[cfg(feature = "debug_kernel")]
    {
        // We have domain caps here, can use:
        // Prototype status: use only the key actually issued to this boot Domain.
        let dbg = DebugConsoleKey::from_key(debug_console_key);
        dbg.write(
            "DEBCON| Debug output via capability invocation on domain's debug console capability\n",
        )
        .unwrap_or_else(|error| {
            panic!(
                "Issued debug console key invocation failed: {:?}",
                error.code()
            );
        });

        // Deliberately malformed key for rejection testing, never a slot-only fallback.
        let invalid_key = RawKey::new(debug_console_key.slot(), 0);
        let err = DebugConsoleKey::from_key(invalid_key);
        assert!(matches!(
            err.write("DEBCON| Invalid capability invocation - no output"),
            Err(CapError::InvalidKey {
                key,
                reason: InvalidKeyReason::ZeroIncarnation,
                operand: 0,
            }) if key == invalid_key
        ));

        // Retype a KeyTable from the boot Untyped into the boot table through
        // the real SVC path (the direct map is live, so the carve lands in the
        // boot Untyped's unused watermark range).
        let untyped = UntypedKey::from_key(boot_untyped_key);
        let self_table = KeyTableKey::from_key(self_table_key);
        let new_table_key = untyped
            .retype(
                ObjectType::KEY_TABLE,
                0,
                1,
                &self_table,
                KeySlot(5).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("boot Retype failed: {:?}", error.code()));
        assert_eq!(new_table_key.slot(), KeySlot(5));
        assert_ne!(new_table_key.incarnation(), 0);

        // The new table is a distinct carved object: CopyDerive the self-table
        // capability into it through the real SVC path.
        let derived_key = self_table
            .copy_derive(
                self_table_key,
                &KeyTableKey::from_key(new_table_key),
                KeySlot(1).0,
                Rights(Rights::DERIVE),
            )
            .unwrap_or_else(|error| panic!("cross-table CopyDerive failed: {:?}", error.code()));
        assert_eq!(derived_key.slot(), KeySlot(1));

        // Validation failures leave the Untyped and destination unchanged.
        assert!(matches!(
            untyped.retype(
                ObjectType::DOMAIN,
                0,
                1,
                &self_table,
                KeySlot(6).0,
                Rights::all(),
            ),
            Err(CapError::InvalidObjectType(ObjectType::DOMAIN))
        ));
        assert!(matches!(
            untyped.retype(
                ObjectType::KEY_TABLE,
                0,
                1,
                &self_table,
                KeySlot(5).0,
                Rights::all(),
            ),
            Err(CapError::SlotOccupied(KeySlot(5)))
        ));
        assert!(matches!(
            untyped.retype(
                ObjectType::KEY_TABLE,
                0,
                u32::MAX,
                &self_table,
                KeySlot(7).0,
                Rights::all(),
            ),
            Err(CapError::InsufficientMemory)
        ));

        // A second Retype must not disturb the first carved table: its
        // storage and bookkeeping survive the next carve (non-overlap).
        let second_table_key = untyped
            .retype(
                ObjectType::KEY_TABLE,
                0,
                1,
                &self_table,
                KeySlot(6).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("second boot Retype failed: {:?}", error.code()));
        assert_eq!(second_table_key.slot(), KeySlot(6));

        // The first new table still accepts a cross-table derivation after
        // the second carve.
        let derived_again = self_table
            .copy_derive(
                self_table_key,
                &KeyTableKey::from_key(new_table_key),
                KeySlot(2).0,
                Rights(Rights::DERIVE),
            )
            .unwrap_or_else(|error| panic!("post-carve CopyDerive failed: {:?}", error.code()));
        assert_eq!(derived_again.slot(), KeySlot(2));

        // Retype a Frame (4 KiB, the AArch64 small-granule baseline) from the
        // boot Untyped through the real SVC path. The kernel sanitizes the
        // carved contents (zeroes them) before installing the capability.
        let frame_key = untyped
            .retype(
                ObjectType::FRAME,
                12,
                1,
                &self_table,
                KeySlot(9).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("frame Retype failed: {:?}", error.code()));
        assert_eq!(frame_key.slot(), KeySlot(9));

        // Non-granular frame sizes are rejected with the architecture's own
        // error, leaving the table unchanged (slot 10 stays free).
        assert!(matches!(
            untyped.retype(
                ObjectType::FRAME,
                13,
                1,
                &self_table,
                KeySlot(10).0,
                Rights::all(),
            ),
            Err(CapError::InvalidFrameSize(13))
        ));

        // The frame entry records the aligned absolute carve and the granule.
        let frame_paddr = {
            // SAFETY: keytable_addr names the live boot KeyTable.
            let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
            let frame = boot_table
                .lookup(frame_key)
                .unwrap_or_else(|_| panic!("frame entry missing"))
                .as_frame()
                .unwrap_or_else(|_| panic!("frame entry is not a Frame cap"));
            assert_eq!(frame.size_bits, 12);
            assert!(!frame.is_device());
            frame.paddr
        };
        assert_eq!(
            frame_paddr % 4096,
            0,
            "the frame carve must be frame-aligned"
        );

        // Sanitization: the carved frame contents were zeroed at retype.
        {
            // SAFETY: the frame lies in the boot Untyped's committed range;
            // the direct map is live.
            let words = unsafe {
                slice::from_raw_parts(
                    PhysAddr::new(frame_paddr).user_to_kernel().as_ptr::<u64>(),
                    4096 / 8,
                )
            };
            assert!(
                words.iter().all(|word| *word == 0),
                "frame contents must be zeroed at retype"
            );
        }

        // A subsequent same-size frame carve continues the watermark exactly:
        // no gap and no overlap between consecutive frame carves.
        let second_frame_key = untyped
            .retype(
                ObjectType::FRAME,
                12,
                1,
                &self_table,
                KeySlot(10).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("second frame Retype failed: {:?}", error.code()));
        assert_eq!(second_frame_key.slot(), KeySlot(10));
        let second_frame_paddr = {
            // SAFETY: see above.
            let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
            boot_table
                .lookup(second_frame_key)
                .unwrap_or_else(|_| panic!("second frame entry missing"))
                .as_frame()
                .unwrap_or_else(|_| panic!("second frame entry is not a Frame cap"))
                .paddr
        };
        assert_eq!(
            second_frame_paddr,
            frame_paddr + 4096,
            "consecutive frame carves must neither gap nor overlap"
        );

        // A larger-granule frame (2 MiB) aligns its own carve and is zeroed
        // too (spot-checked at the first words).
        let large_frame_key = untyped
            .retype(
                ObjectType::FRAME,
                21,
                1,
                &self_table,
                KeySlot(11).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("large frame Retype failed: {:?}", error.code()));
        assert_eq!(large_frame_key.slot(), KeySlot(11));
        let large_frame_paddr = {
            // SAFETY: see above.
            let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
            boot_table
                .lookup(large_frame_key)
                .unwrap_or_else(|_| panic!("large frame entry missing"))
                .as_frame()
                .unwrap_or_else(|_| panic!("large frame entry is not a Frame cap"))
                .paddr
        };
        assert_eq!(
            large_frame_paddr % (2 * 1024 * 1024),
            0,
            "the 2 MiB frame carve must be 2 MiB-aligned"
        );
        assert!(
            large_frame_paddr >= second_frame_paddr + 4096,
            "the large frame must not overlap the earlier carves"
        );
        {
            // SAFETY: see above.
            let words = unsafe {
                slice::from_raw_parts(
                    PhysAddr::new(large_frame_paddr)
                        .user_to_kernel()
                        .as_ptr::<u64>(),
                    8,
                )
            };
            assert!(
                words.iter().all(|word| *word == 0),
                "the large frame must be zeroed at retype"
            );
        }

        // A region whose base is not aligned still yields an aligned carve:
        // the absolute carve address (base + watermark) is aligned up, not
        // just the watermark. Fabricate a RAM region at a deliberately
        // misaligned free physical address (just past the boot Untyped's
        // committed watermark) and retype from it through the real SVC path.
        let (boot_paddr, boot_wm) = {
            // SAFETY: keytable_addr names the live boot KeyTable.
            let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
            let region = boot_table
                .lookup(boot_untyped_key)
                .unwrap_or_else(|_| panic!("boot Untyped entry missing"))
                .as_untyped()
                .unwrap_or_else(|_| panic!("boot Untyped entry is not a region"));
            (
                region.paddr,
                u64::try_from(region.watermark_bytes()).unwrap(),
            )
        };
        // The committed watermark is object-aligned, so +24 keeps the address
        // inside the boot Untyped's free RAM while making the region base
        // misaligned for the KeyTable alignment (32 under the current layout;
        // 16 is the watermark encoding granularity — mirror the kernel's
        // max(align_of, MIN_ALIGN)).
        let align = u64::try_from(core::mem::align_of::<KeyTable>().max(16)).unwrap();
        let misaligned_base = boot_paddr + boot_wm + 24;
        assert_ne!(misaligned_base % align, 0, "fixture must be misaligned");
        let misaligned_untyped_key = {
            // SAFETY: see above.
            let boot_table = unsafe { &mut *(keytable_addr as *mut KeyTable) };
            boot_table
                .insert(
                    KeySlot(7),
                    KeyEntry::new_untyped(misaligned_base, 14, false, Rights::all()),
                )
                .unwrap_or_else(|_| panic!("misaligned region install failed"))
        };
        let carved_key = UntypedKey::from_key(misaligned_untyped_key)
            .retype(
                ObjectType::KEY_TABLE,
                0,
                1,
                &self_table,
                KeySlot(8).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("misaligned-base Retype failed: {:?}", error.code()));
        assert_eq!(carved_key.slot(), KeySlot(8));

        // Read the carved address back: it must be the aligned base, not the
        // region's misaligned start. The capability stores the kernel-window
        // address of the carve; convert it back to physical to compare.
        let carved_paddr = {
            // SAFETY: see above.
            let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
            let window_addr = boot_table
                .lookup(carved_key)
                .unwrap_or_else(|_| panic!("carved entry missing"))
                .keytable_address()
                .unwrap_or_else(|_| panic!("carved entry is not a KeyTable cap"));
            VirtAddr::new(window_addr).kernel_to_user().as_u64()
        };
        assert_eq!(
            carved_paddr,
            misaligned_base + (align - misaligned_base % align) % align,
            "the carve must start at the aligned absolute address"
        );

        // The aligned carve produced a live table: derive into it.
        let derived_misaligned = self_table
            .copy_derive(
                self_table_key,
                &KeyTableKey::from_key(carved_key),
                KeySlot(1).0,
                Rights(Rights::DERIVE),
            )
            .unwrap_or_else(|error| {
                panic!("misaligned-base CopyDerive failed: {:?}", error.code())
            });
        assert_eq!(derived_misaligned.slot(), KeySlot(1));

        // ─────────────────────────────────────────────────────────────────
        // Mapping vertical slice (2026-09-15): carved page tables, real
        // descriptor installation, mapping bookkeeping, and teardown.
        // ─────────────────────────────────────────────────────────────────

        // The boot Domain capability (installed at the well-known self slot)
        // is the bootstrap-era mapping context.
        let boot_domain_key = RawKey::new(KeySlot::SELF_DOMAIN, 1);

        // PageTable Retype: a fixed 4 KiB carve; other size_bits are rejected
        // with the architecture's own error, leaving the slot free.
        assert!(matches!(
            untyped.retype(
                ObjectType::PAGE_TABLE,
                13,
                1,
                &self_table,
                KeySlot(20).0,
                Rights::all(),
            ),
            Err(CapError::InvalidSize(13))
        ));
        let root_pt_key = untyped
            .retype(
                ObjectType::PAGE_TABLE,
                12,
                1,
                &self_table,
                KeySlot(20).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("root PageTable Retype failed: {:?}", error.code()));
        assert_eq!(root_pt_key.slot(), KeySlot(20));

        // The carved table is sanitized (zeroed) at retype: stale descriptors
        // must never leak prior contents into hardware walks.
        {
            // SAFETY: keytable_addr names the live boot KeyTable.
            let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
            let id = boot_table
                .lookup(root_pt_key)
                .unwrap_or_else(|_| panic!("root PageTable entry missing"))
                .object_id()
                .unwrap_or_else(|_| panic!("root PageTable entry has no identity"));
            let paddr = nucleus
                .pools
                .arch
                .page_tables
                .get_live(usize::from(id.index))
                .unwrap_or_else(|| panic!("root PageTable metadata missing"))
                .paddr;
            assert_eq!(paddr % 4096, 0, "the table carve must be 4 KiB-aligned");
            // SAFETY: the table lies in the boot Untyped's committed range;
            // the direct map is live.
            let words = unsafe {
                slice::from_raw_parts(PhysAddr::new(paddr).user_to_kernel().as_ptr::<u64>(), 512)
            };
            assert!(
                words.iter().all(|word| *word == 0),
                "page-table contents must be zeroed at retype"
            );
        }

        // Carve the rest of the chain toward vaddr 0x1000_0000.
        let l1_pt_key = untyped
            .retype(
                ObjectType::PAGE_TABLE,
                12,
                1,
                &self_table,
                KeySlot(21).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("L1 PageTable Retype failed: {:?}", error.code()));
        let l2_pt_key = untyped
            .retype(
                ObjectType::PAGE_TABLE,
                12,
                1,
                &self_table,
                KeySlot(22).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("L2 PageTable Retype failed: {:?}", error.code()));
        let l3_pt_key = untyped
            .retype(
                ObjectType::PAGE_TABLE,
                12,
                1,
                &self_table,
                KeySlot(23).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("L3 PageTable Retype failed: {:?}", error.code()));

        // Install the root into the boot Domain (vaddr must be zero).
        //
        // ASID binding (2026-09-15) through the real SVC path: before a
        // translation root exists, the assignment is rejected — an ASID binds
        // to a Domain's root, not to the Domain in the abstract.
        let boot_asid_pool_key = RawKey::new(KeySlot::BOOT_ASID_POOL, 1);
        let boot_asid_pool = ASIDPoolKey::from_key(boot_asid_pool_key);
        assert!(matches!(
            boot_asid_pool.assign(boot_domain_key),
            Err(CapError::NotMapped)
        ));
        // A non-Domain target key is a type mismatch, not a lookup success.
        assert!(matches!(
            boot_asid_pool.assign(boot_untyped_key),
            Err(CapError::TypeMismatch { .. })
        ));

        // Domain activation (2026-09-15) through the real SVC path: before a
        // translation root exists, activation is rejected — there is no
        // hardware context to install. (A non-Domain invoked key never reaches
        // the Domain handler: dispatch selects the handler by the invoked
        // key's own type.)
        let boot_domain = DomainKey::from_key(boot_domain_key, DomainId(0));
        assert!(matches!(boot_domain.activate(), Err(CapError::NotMapped)));

        let root_pt = PageTableKey::from_key(root_pt_key);
        root_pt
            .map(boot_domain_key, 0)
            .unwrap_or_else(|error| panic!("root PageTable.Map failed: {:?}", error.code()));
        // A second root is rejected: the Domain's root slot is occupied.
        assert!(matches!(
            root_pt.map(boot_domain_key, 0),
            Err(CapError::AlreadyMapped)
        ));
        {
            let domain = nucleus
                .pools
                .domains
                .get_live(0)
                .unwrap_or_else(|| panic!("boot Domain missing"));
            assert!(domain.translation_root.is_some());
        }

        // A root without a bound ASID still establishes no hardware context.
        assert!(matches!(boot_domain.activate(), Err(CapError::NotMapped)));

        // With a root installed, the assignment binds the lowest free ASID
        // (ASID 0 is reserved for the kernel's boot context, so the first
        // grant is 1) and records it on the Domain.
        let bound_asid = boot_asid_pool
            .assign(boot_domain_key)
            .unwrap_or_else(|error| panic!("ASIDPool.Assign failed: {:?}", error.code()));
        assert_eq!(bound_asid, 1);
        {
            let domain = nucleus
                .pools
                .domains
                .get_live(0)
                .unwrap_or_else(|| panic!("boot Domain missing"));
            assert_eq!(domain.asid, Some(1));
        }
        // A second assignment to the same Domain is rejected: one ASID per
        // translation context.
        assert!(matches!(
            boot_asid_pool.assign(boot_domain_key),
            Err(CapError::AlreadyMapped)
        ));

        // ─────────────────────────────────────────────────────────────────
        // Notification: Retype-creatable synchronization state (2026-09-16)
        // ─────────────────────────────────────────────────────────────────

        // Retype two Notifications: pure kernel state allocated from the
        // bootstrap-carved pool — no Untyped bytes are carved.
        let notification_key = untyped
            .retype(
                ObjectType::NOTIFICATION,
                0,
                2,
                &self_table,
                KeySlot(16).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("notification Retype failed: {:?}", error.code()));
        assert_eq!(notification_key.slot(), KeySlot(16));

        // A nonzero size_bits is rejected before any pool slot is taken.
        assert!(matches!(
            untyped.retype(
                ObjectType::NOTIFICATION,
                12,
                1,
                &self_table,
                KeySlot(18).0,
                Rights::all(),
            ),
            Err(CapError::InvalidSize(12))
        ));

        let notification = NotificationKey::from_key(notification_key);

        // Signal coalesces distinct bits into the bitmap; Poll consumes all
        // pending bits at once.
        notification
            .signal(0b0110)
            .unwrap_or_else(|error| panic!("Notification.Signal failed: {:?}", error.code()));
        notification.signal(0b0001).unwrap_or_else(|error| {
            panic!("second Notification.Signal failed: {:?}", error.code())
        });
        assert_eq!(
            notification
                .poll()
                .unwrap_or_else(|error| panic!("Notification.Poll failed: {:?}", error.code())),
            0b0111
        );
        // Nothing pending: Poll returns zero, never blocking.
        assert_eq!(
            notification.poll().unwrap_or_else(|error| panic!(
                "second Notification.Poll failed: {:?}",
                error.code()
            )),
            0
        );

        // An already-satisfied Wait consumes and returns the bits
        // immediately through the real SVC path.
        notification
            .signal(0b1)
            .unwrap_or_else(|error| panic!("third Notification.Signal failed: {:?}", error.code()));
        assert_eq!(
            notification
                .wait(NotificationKey::WAIT_INFINITE)
                .unwrap_or_else(|error| panic!(
                    "satisfied Notification.Wait failed: {:?}",
                    error.code()
                )),
            0b1
        );

        // A wait that would block now blocks for real (completion foundation,
        // 2026-09-16): the end-to-end proof below parks the boot domain and
        // resumes it through the Bounce fixture domain. A finite timeout is
        // still rejected: the time subsystem does not exist yet.
        assert!(matches!(
            notification.wait(1_000_000),
            Err(CapError::InvalidOperation)
        ));

        // ─────────────────────────────────────────────────────────────────
        // Blocking Wait end-to-end: the Bounce fixture domain (N4-A, 2026-09-16)
        // ─────────────────────────────────────────────────────────────────

        // N1: the notification the boot domain blocks on. N2: the one
        // Bounce parks on between its rounds. EC: the event count whose
        // awaits the boot domain blocks on and Bounce advances. All carved
        // through the public Retype path.
        let n1_key = untyped
            .retype(
                ObjectType::NOTIFICATION,
                0,
                1,
                &self_table,
                KeySlot(18).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("N1 Retype failed: {:?}", error.code()));
        let n2_key = untyped
            .retype(
                ObjectType::NOTIFICATION,
                0,
                1,
                &self_table,
                KeySlot(19).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("N2 Retype failed: {:?}", error.code()));
        let ec_key = untyped
            .retype(
                ObjectType::EVENT_COUNT,
                0,
                1,
                &self_table,
                KeySlot(37).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("EC Retype failed: {:?}", error.code()));

        // Bounce's kernel stack: eight contiguous 4 KiB frames (32 KiB)
        // carved through the public path. The SVC entry path nests several
        // semihosting println buffers (4 KiB each: the syscall entry, the
        // dispatch, and the per-object handler all format one), so a single
        // 4 KiB stack frame overflows into the carves directly below it and
        // corrupts them — observed as the later Frame.Map alias walk
        // following garbage L2 entries into a fault. 32 KiB holds the
        // deepest handler chain with headroom. Full-descending, so the stack
        // top is the last frame's kernel end.
        let bounce_stack_key = untyped
            .retype(
                ObjectType::FRAME,
                12,
                8,
                &self_table,
                KeySlot(41).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("Bounce stack Retype failed: {:?}", error.code()));
        let (bounce_stack_paddr, _bounce_stack_size) = FrameKey::from_key(bounce_stack_key)
            .get_extent()
            .unwrap_or_else(|error| panic!("Bounce stack GetExtent failed: {:?}", error.code()));
        let bounce_stack_top =
            PhysAddr::new(bounce_stack_paddr).user_to_kernel().as_u64() + 8 * 4096;

        // Bounce's capability table, carved through the public Retype path.
        // The fixture's slots (40–48) sit outside the later pool-refill
        // test's destination range (24–35), which requires those slots
        // vacant.
        let bounce_table_key = untyped
            .retype(
                ObjectType::KEY_TABLE,
                0,
                1,
                &self_table,
                KeySlot(40).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("Bounce KeyTable Retype failed: {:?}", error.code()));
        let bounce_table_addr = {
            // SAFETY: the boot table is the live carved boot KeyTable.
            let entry = unsafe { &*(keytable_addr as *const KeyTable) }
                .lookup(bounce_table_key)
                .unwrap_or_else(|_| panic!("Bounce KeyTable entry missing"));
            entry
                .keytable_address()
                .unwrap_or_else(|_| panic!("Bounce KeyTable entry is not carved"))
        };

        // Bootstrap grant: install Bounce's notification keys (the boot
        // test's authority delegated kernel-privately by the bootstrap
        // builder, like the boot console grant).
        {
            // SAFETY: the boot table is the live carved boot KeyTable.
            let boot_table_ref = unsafe { &*(keytable_addr as *const KeyTable) };
            let n1_id = boot_table_ref
                .lookup(n1_key)
                .and_then(KeyEntry::object_id)
                .unwrap_or_else(|_| panic!("N1 entry missing or not a pool identity"));
            let n2_id = boot_table_ref
                .lookup(n2_key)
                .and_then(KeyEntry::object_id)
                .unwrap_or_else(|_| panic!("N2 entry missing or not a pool identity"));
            let ec_id = boot_table_ref
                .lookup(ec_key)
                .and_then(KeyEntry::object_id)
                .unwrap_or_else(|_| panic!("EC entry missing or not a pool identity"));
            // SAFETY: Bounce's table is the freshly carved, live KeyTable.
            let bounce_table = unsafe { &mut *(bounce_table_addr as *mut KeyTable) };
            bounce_table
                .insert(
                    KeySlot(1),
                    KeyEntry::from_id(ObjectType::NOTIFICATION, n1_id, Rights::all(), 0),
                )
                .unwrap_or_else(|failure| {
                    panic!("Bounce N1 grant failed: {:?}", failure.error.code())
                });
            bounce_table
                .insert(
                    KeySlot(2),
                    KeyEntry::from_id(ObjectType::NOTIFICATION, n2_id, Rights::all(), 0),
                )
                .unwrap_or_else(|failure| {
                    panic!("Bounce N2 grant failed: {:?}", failure.error.code())
                });
            bounce_table
                .insert(
                    KeySlot(3),
                    KeyEntry::from_id(ObjectType::EVENT_COUNT, ec_id, Rights::all(), 0),
                )
                .unwrap_or_else(|failure| {
                    panic!("Bounce EC grant failed: {:?}", failure.error.code())
                });
        }

        // Allocate Bounce's Domain and queue it runnable: it starts only
        // when the boot domain blocks.
        let (bounce_id, _bounce_domain) = nucleus
            .pools
            .domains
            .allocate(Domain {
                keytable_addr: bounce_table_addr,
                translation_root: None,
                asid: None,
                context: ExecutionContext::NotStarted {
                    pc: bounce_entry as *const () as u64,
                    stack_top: bounce_stack_top,
                },
            })
            .unwrap_or_else(|| panic!("no Bounce Domain slot"));
        assert_eq!(bounce_id.index, 1);
        assert!(nucleus.scheduler.push(bounce_id.index));

        // The boot domain blocks on N1: this SVC does not return — the
        // kernel parks it, starts Bounce (which signals N1 and parks on N2),
        // then resumes the boot domain with the delivered bitmap.
        let received = NotificationKey::from_key(n1_key)
            .wait(NotificationKey::WAIT_INFINITE)
            .unwrap_or_else(|error| {
                panic!("blocking Notification.Wait failed: {:?}", error.code())
            });
        assert_eq!(received, BOUNCE_MAGIC_BITS);
        // Bounce is parked on N2; the boot domain resumed with the bits.
        {
            let bounce = nucleus
                .pools
                .domains
                .get_live(usize::from(bounce_id.index))
                .unwrap_or_else(|| panic!("Bounce Domain missing"));
            assert!(matches!(bounce.context, ExecutionContext::Parked { .. }));
        }

        // ─────────────────────────────────────────────────────────────────
        // EventCount end-to-end (2026-09-18): monotonic counting, the
        // selected overflow policy, and blocking Await through the Bounce
        // fixture (broadcast wakeups, error wakeups)
        // ─────────────────────────────────────────────────────────────────

        let event_count = EventCountKey::from_key(ec_key);

        // A fresh counter reads zero.
        assert_eq!(
            event_count
                .read()
                .unwrap_or_else(|error| panic!("EventCount.Read failed: {:?}", error.code())),
            0
        );
        // Every advance is counted and returns the new value.
        assert_eq!(
            event_count
                .advance(7)
                .unwrap_or_else(|error| panic!("EventCount.Advance failed: {:?}", error.code())),
            7
        );
        assert_eq!(
            event_count.read().unwrap_or_else(|error| panic!(
                "second EventCount.Read failed: {:?}",
                error.code()
            )),
            7
        );
        // Zero is invalid: an advance must strictly increase (selected
        // 2026-09-18).
        assert!(matches!(
            event_count.advance(0),
            Err(CapError::InvalidOperation)
        ));
        // Overflow rejects, leaves the counter unchanged, and carries the
        // shared status (selected 2026-09-18). No waiter is queued here, so
        // only the advancer observes the error.
        assert!(matches!(
            event_count.advance(u64::MAX),
            Err(CapError::CounterOverflow)
        ));
        assert_eq!(
            event_count
                .read()
                .unwrap_or_else(|error| panic!("third EventCount.Read failed: {:?}", error.code())),
            7
        );
        // An already-satisfied await returns the current value without
        // blocking; awaiting does not consume the counter.
        assert_eq!(
            event_count
                .await_ge(7, EventCountKey::WAIT_INFINITE)
                .unwrap_or_else(|error| panic!(
                    "satisfied EventCount.Await failed: {:?}",
                    error.code()
                )),
            7
        );
        // A finite timeout is still rejected: the time subsystem does not
        // exist yet.
        assert!(matches!(
            event_count.await_ge(100, 1_000_000),
            Err(CapError::InvalidOperation)
        ));

        // Blocking Await end-to-end: request Bounce's +3 advance by
        // signaling N2 (bit 0), then block on target 10. Bounce advances the
        // counter, this domain's record completes with the new value, and
        // Bounce parks again.
        NotificationKey::from_key(n2_key)
            .signal(0b1)
            .unwrap_or_else(|error| panic!("N2 trigger signal failed: {:?}", error.code()));
        assert_eq!(
            event_count
                .await_ge(10, EventCountKey::WAIT_INFINITE)
                .unwrap_or_else(|error| {
                    panic!("blocking EventCount.Await failed: {:?}", error.code())
                }),
            10
        );
        assert_eq!(
            event_count.read().unwrap_or_else(|error| panic!(
                "fourth EventCount.Read failed: {:?}",
                error.code()
            )),
            10
        );

        // Overflow wakes blocked waiters with the shared error (selected
        // 2026-09-18): request Bounce's overflowing advance (bit 1), then
        // block on an unreachable target. The advance completes the await
        // with `CounterOverflow`, the counter stays unchanged, and Bounce
        // parks for good.
        NotificationKey::from_key(n2_key)
            .signal(0b10)
            .unwrap_or_else(|error| panic!("N2 overflow trigger failed: {:?}", error.code()));
        assert!(matches!(
            event_count.await_ge(u64::MAX - 2, EventCountKey::WAIT_INFINITE),
            Err(CapError::CounterOverflow)
        ));
        assert_eq!(
            event_count
                .read()
                .unwrap_or_else(|error| panic!("fifth EventCount.Read failed: {:?}", error.code())),
            10
        );
        // Bounce is parked on N2 for good; the boot domain resumed with the
        // error completion.
        {
            let bounce = nucleus
                .pools
                .domains
                .get_live(usize::from(bounce_id.index))
                .unwrap_or_else(|| panic!("Bounce Domain missing"));
            assert!(matches!(bounce.context, ExecutionContext::Parked { .. }));
        }

        // ─────────────────────────────────────────────────────────────────
        // Domain.Retire end-to-end (2026-09-19): the Domain-control teardown
        // trigger — cancel every pending record naming the target as waiter,
        // purge its queued wakeup, reclaim its pool slot — through the real
        // SVC path under `RETIRE` authority.
        // ─────────────────────────────────────────────────────────────────
        {
            // Bounce is parked on N2 with a Waiting record: the canonical
            // teardown state of a blocked Domain.
            let ExecutionContext::Parked { record, .. } = nucleus
                .pools
                .domains
                .get_live(usize::from(bounce_id.index))
                .unwrap_or_else(|| panic!("Bounce Domain missing"))
                .context
            else {
                panic!("Bounce is not parked")
            };
            assert_eq!(
                nucleus.pending.state(record).ok(),
                Some(PendingState::Waiting)
            );
            assert_eq!(nucleus.pending.waiter(record).ok(), Some(bounce_id));

            // Bootstrap grants: Bounce's Domain capability in the boot table,
            // kernel-privately (like the boot console grant) — one with full
            // rights and one without `RETIRE`, so the authority check is
            // observable through the real SVC path. Domain is not on the
            // CopyDerive allowlist, so no public path could build these.
            let (bounce_domain_key, unprivileged_bounce_key) = {
                // SAFETY: keytable_addr names the live carved boot KeyTable.
                let boot_table = unsafe { &mut *(keytable_addr as *mut KeyTable) };
                let full = boot_table
                    .insert(
                        KeySlot(38),
                        KeyEntry::new::<Domain>(bounce_id, Rights::all(), 0),
                    )
                    .unwrap_or_else(|failure| {
                        panic!("Bounce Domain grant failed: {:?}", failure.error.code())
                    });
                let limited = boot_table
                    .insert(
                        KeySlot(39),
                        KeyEntry::new::<Domain>(bounce_id, Rights(Rights::READ), 0),
                    )
                    .unwrap_or_else(|failure| {
                        panic!(
                            "limited Bounce Domain grant failed: {:?}",
                            failure.error.code()
                        )
                    });
                (full, limited)
            };

            // Authority is explicit: a capability without `RETIRE` is
            // rejected before any teardown effect — Bounce stays parked.
            assert!(matches!(
                DomainKey::from_key(unprivileged_bounce_key, DomainId(1)).retire(),
                Err(CapError::InsufficientRights)
            ));
            assert_eq!(
                nucleus.pending.state(record).ok(),
                Some(PendingState::Waiting)
            );

            // Self-retirement is rejected: the current Domain must survive
            // its own invocation (never-returns self-retirement is recorded
            // in the contract as wanted follow-up) — and again nothing was
            // torn down.
            assert!(matches!(
                DomainKey::from_key(boot_domain_key, DomainId(0)).retire(),
                Err(CapError::InvalidOperation)
            ));
            assert_eq!(
                nucleus.pending.state(record).ok(),
                Some(PendingState::Waiting)
            );

            // Retire Bounce through the real SVC path: the parked record is
            // cancelled and released, and no queued wakeup survives.
            DomainKey::from_key(bounce_domain_key, DomainId(1))
                .retire()
                .unwrap_or_else(|error| panic!("Bounce Retire failed: {:?}", error.code()));
            assert!(nucleus.pending.is_empty());
            assert!(nucleus.scheduler.is_empty());
            // The released record's identity is stale.
            nucleus.pending.state(record).unwrap_err();

            // N2 no longer holds Bounce: a signal through the real SVC path
            // delivers to no dead waiter — it succeeds and the bits stay
            // pending — and the notification remains fully usable.
            NotificationKey::from_key(n2_key)
                .signal(0b100)
                .unwrap_or_else(|error| panic!("post-retire Signal failed: {:?}", error.code()));
            let bits = NotificationKey::from_key(n2_key)
                .wait(NotificationKey::WAIT_INFINITE)
                .unwrap_or_else(|error| panic!("post-retire Wait failed: {:?}", error.code()));
            assert_eq!(bits, 0b100);

            // The retired Domain's pool slot is reclaimed and its capability
            // is stale: the identity no longer resolves, and a further Retire
            // through the same capability fails with a defined error.
            nucleus.pools.domains.validate(bounce_id).unwrap_err();
            assert!(
                nucleus
                    .pools
                    .domains
                    .get_live(usize::from(bounce_id.index))
                    .is_none()
            );
            assert!(matches!(
                DomainKey::from_key(bounce_domain_key, DomainId(1)).retire(),
                Err(CapError::InvalidOperation)
            ));
        }

        // Build the intermediate chain: L1 under the root, L2 under L1,
        // L3 under L2, all selecting the slots for vaddr 0x1000_0000.
        let l1_pt = PageTableKey::from_key(l1_pt_key);
        l1_pt
            .map(root_pt_key, 0x1000_0000)
            .unwrap_or_else(|error| panic!("L1 PageTable.Map failed: {:?}", error.code()));
        let l2_pt = PageTableKey::from_key(l2_pt_key);
        l2_pt
            .map(l1_pt_key, 0x1000_0000)
            .unwrap_or_else(|error| panic!("L2 PageTable.Map failed: {:?}", error.code()));
        let l3_pt = PageTableKey::from_key(l3_pt_key);
        l3_pt
            .map(l2_pt_key, 0x1000_0000)
            .unwrap_or_else(|error| panic!("L3 PageTable.Map failed: {:?}", error.code()));

        // Mapping a table into itself is rejected as an alias.
        assert!(matches!(
            l1_pt.map(l1_pt_key, 0x1000_0000),
            Err(CapError::InvalidOperation)
        ));

        // Frame.Map with a missing intermediate fails with the faulting vaddr.
        let frame = FrameKey::from_key(frame_key);
        assert!(matches!(
            frame.map(
                boot_domain_key,
                0x2000_0000,
                Rights(Rights::READ | Rights::WRITE),
                0
            ),
            Err(CapError::MissingIntermediate { vaddr: 0x2000_0000 })
        ));
        // A misaligned virtual address is rejected with the frame's size.
        assert!(matches!(
            frame.map(
                boot_domain_key,
                0x1000_0001,
                Rights(Rights::READ | Rights::WRITE),
                0
            ),
            Err(CapError::InvalidSize(12))
        ));
        // Unsupported attributes are rejected.
        assert!(matches!(
            frame.map(
                boot_domain_key,
                0x1000_0000,
                Rights(Rights::READ | Rights::WRITE),
                1
            ),
            Err(CapError::InvalidOperation)
        ));

        // Real mapping: the walk installs the page descriptor at level 3.
        frame
            .map(
                boot_domain_key,
                0x1000_0000,
                Rights(Rights::READ | Rights::WRITE),
                0,
            )
            .unwrap_or_else(|error| panic!("Frame.Map failed: {:?}", error.code()));
        // A second mapping of the same capability is rejected.
        assert!(matches!(
            frame.map(
                boot_domain_key,
                0x1000_0000,
                Rights(Rights::READ | Rights::WRITE),
                0
            ),
            Err(CapError::AlreadyMapped)
        ));

        // Verify the descriptor chain by hand through the direct map.
        let root_paddr = nucleus
            .pools
            .domains
            .get_live(0)
            .unwrap_or_else(|| panic!("boot Domain missing"))
            .translation_root
            .expect("translation root missing");
        {
            const ADDR_MASK: u64 = 0x0000_FFFF_FFFF_F000;
            // SAFETY: the tables are carved RAM in the boot Untyped's committed
            // range; the direct map is live.
            let read_entry = |paddr: u64, slot: usize| unsafe {
                *(PhysAddr::new(paddr).user_to_kernel().as_ptr::<u64>()).add(slot)
            };
            let l0e = read_entry(root_paddr, 0);
            assert!(
                l0e & 0b11 == 0b11,
                "L0 entry must be a valid table descriptor"
            );
            let l1_paddr = l0e & ADDR_MASK;
            let l1e = read_entry(l1_paddr, 0);
            assert!(
                l1e & 0b11 == 0b11,
                "L1 entry must be a valid table descriptor"
            );
            let l2_paddr = l1e & ADDR_MASK;
            let l2e = read_entry(l2_paddr, 128);
            assert!(
                l2e & 0b11 == 0b11,
                "L2 entry must be a valid table descriptor"
            );
            let l3_paddr = l2e & ADDR_MASK;
            let pte = read_entry(l3_paddr, 0);
            assert_eq!(pte & ADDR_MASK, frame_paddr, "the PTE must name the frame");
            assert!(pte & 0b1 != 0, "the PTE must be valid");
            assert!(pte & 0b10 != 0, "a level-3 entry must be a page descriptor");
            assert!(pte & (1 << 10) != 0, "the access flag must be set");
            assert_eq!(
                pte & (0b11 << 6),
                0b01 << 6,
                "a READ|WRITE mapping must be user read/write"
            );
            assert!(
                pte & (1 << 53) != 0 && pte & (1 << 54) != 0,
                "a mapping without the EXECUTE right must stay UXN|PXN"
            );
        }

        // The frame entry records the full mapping identity.
        {
            // SAFETY: keytable_addr names the live boot KeyTable.
            let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
            let f = boot_table
                .lookup(frame_key)
                .unwrap_or_else(|_| panic!("frame entry missing"))
                .as_frame()
                .unwrap_or_else(|_| panic!("frame entry is not a Frame cap"));
            assert!(f.is_mapped());
            assert_eq!(f.mapping().unwrap().vaddr, 0x1000_0000);
        }

        // CopyDerive of a mapped frame yields an unmapped derived capability:
        // Copy is capability-only derivation with no mapping association.
        let derived_frame_key = self_table
            .copy_derive(
                frame_key,
                &self_table,
                KeySlot(12).0,
                Rights(Rights::READ | Rights::WRITE),
            )
            .unwrap_or_else(|error| panic!("frame CopyDerive failed: {:?}", error.code()));
        assert!(matches!(
            FrameKey::from_key(derived_frame_key).unmap(),
            Err(CapError::NotMapped)
        ));

        // Alias policy: the derived capability names the same physical frame
        // the original maps at 0x1000_0000. Mapping it at a second virtual
        // address in the same Domain is rejected whatever capability carries
        // it; the error names the conflicting live mapping's physical base.
        assert!(matches!(
            FrameKey::from_key(derived_frame_key).map(
                boot_domain_key,
                0x1000_1000,
                Rights(Rights::READ | Rights::WRITE),
                0
            ),
            Err(CapError::PhysicalAlias { paddr }) if paddr == frame_paddr
        ));

        // A 2 MiB frame installs a block descriptor at level 2 in the same
        // chain (a distinct slot, read-only).
        let large_frame = FrameKey::from_key(large_frame_key);
        large_frame
            .map(boot_domain_key, 0x1020_0000, Rights(Rights::READ), 0)
            .unwrap_or_else(|error| panic!("2 MiB Frame.Map failed: {:?}", error.code()));
        {
            const ADDR_MASK: u64 = 0x0000_FFFF_FFFF_F000;
            // SAFETY: see above.
            let read_entry = |paddr: u64, slot: usize| unsafe {
                *(PhysAddr::new(paddr).user_to_kernel().as_ptr::<u64>()).add(slot)
            };
            let l0e = read_entry(root_paddr, 0);
            let l1e = read_entry(l0e & ADDR_MASK, 0);
            let block = read_entry(l1e & ADDR_MASK, 129);
            assert_eq!(block & ADDR_MASK, large_frame_paddr);
            assert!(block & 0b1 != 0, "the block descriptor must be valid");
            assert!(
                block & 0b10 == 0,
                "a level-2 entry must be a block descriptor"
            );
            assert_eq!(
                block & (0b11 << 6),
                0b11 << 6,
                "a READ-only mapping must be user read-only"
            );
        }

        // ─────────────────────────────────────────────────────────────────
        // Domain activation (2026-09-15): install the bound root into
        // TTBR0_EL1 with the bound ASID, making the carved tables the live
        // hardware translation context for the low half. The kernel executes
        // through the TTBR1 high map, so kernel code keeps running unchanged
        // while the Domain's context is installed.
        // ─────────────────────────────────────────────────────────────────

        // Distinct marker contents: the original frame carries one magic
        // word, the second carved frame another, so a stale cached
        // translation is distinguishable from a freshly walked one.
        #[expect(clippy::items_after_statements)]
        const MAGIC_ORIGINAL: u64 = 0x1111_2222_3333_4444;
        #[expect(clippy::items_after_statements)]
        const MAGIC_SECOND: u64 = 0x5555_6666_7777_8888;
        // SAFETY: both frames lie in the boot Untyped's committed range; the
        // direct map is live.
        unsafe {
            *PhysAddr::new(frame_paddr)
                .user_to_kernel()
                .as_mut_ptr::<u64>() = MAGIC_ORIGINAL;
            *PhysAddr::new(second_frame_paddr)
                .user_to_kernel()
                .as_mut_ptr::<u64>() = MAGIC_SECOND;
        }

        // Save the boot identity-map context so the test can restore it after
        // the observation (deactivation is not a capability operation yet).
        let boot_ttbr0 = TTBR0_EL1.get();

        // The bootstrap caller (this test) executes in the low half through
        // the boot identity map: its stack sits below the image base at
        // 0x80000 and the kickstart image extends beyond the 2 MiB boundary.
        // A real Domain's address space contains its own image and stack by
        // construction, and the bootstrap caller is no exception — map its
        // low-half working set into the boot Domain's context as two 2 MiB
        // blocks (fabricated bootstrap-test fixtures naming the in-use
        // physical range, like the misaligned-region fixture above), so
        // execution can continue under the activated tables.
        let low_block_keys: [RawKey; 2] = [
            {
                // SAFETY: keytable_addr names the live boot KeyTable.
                let boot_table = unsafe { &mut *(keytable_addr as *mut KeyTable) };
                boot_table
                    .insert(
                        KeySlot(14),
                        KeyEntry::new_frame(0, 21, false, Rights::all()),
                    )
                    .unwrap_or_else(|_| panic!("low-half block A install failed"))
            },
            {
                // SAFETY: see above.
                let boot_table = unsafe { &mut *(keytable_addr as *mut KeyTable) };
                boot_table
                    .insert(
                        KeySlot(15),
                        KeyEntry::new_frame(0x20_0000, 21, false, Rights::all()),
                    )
                    .unwrap_or_else(|_| panic!("low-half block B install failed"))
            },
        ];
        // The blocks are requested with the EXECUTE right (selected
        // 2026-09-15): the bootstrap caller must keep executing inside its
        // Domain's context, so its image is executable there.
        for (block, vaddr) in low_block_keys.iter().zip([0_u64, 0x20_0000_u64]) {
            FrameKey::from_key(*block)
                .map(
                    boot_domain_key,
                    vaddr,
                    Rights(Rights::READ | Rights::WRITE | Rights::EXECUTE),
                    0,
                )
                .unwrap_or_else(|error| {
                    panic!(
                        "low-half Frame.Map at {vaddr:#x} failed: {:?}",
                        error.code()
                    )
                });
        }
        {
            const ADDR_MASK: u64 = 0x0000_FFFF_FFFF_F000;
            // SAFETY: see above.
            let read_entry = |paddr: u64, slot: usize| unsafe {
                *(PhysAddr::new(paddr).user_to_kernel().as_ptr::<u64>()).add(slot)
            };
            let l0e = read_entry(root_paddr, 0);
            let l1e = read_entry(l0e & ADDR_MASK, 0);
            let l2_paddr = l1e & ADDR_MASK;
            for (slot, base) in [(0, 0_u64), (1, 0x20_0000)] {
                let block = read_entry(l2_paddr, slot);
                assert_eq!(
                    block & ADDR_MASK,
                    base,
                    "the low-half block must be identity"
                );
                assert!(block & 0b1 != 0, "the low-half block must be valid");
                assert!(
                    block & (1 << 53) == 0 && block & (1 << 54) == 0,
                    "an EXECUTE-requested mapping must have UXN|PXN clear"
                );
                assert_eq!(
                    block & (0b11 << 6),
                    0,
                    "a writable EXECUTE mapping must be kernel-privilege (AP=00)"
                );
            }
        }

        // Activate through the real SVC path: the tables become hardware-live.
        boot_domain
            .activate()
            .unwrap_or_else(|error| panic!("Domain.Activate failed: {:?}", error.code()));

        // A load from the mapped virtual address now walks the Domain's
        // tables: the marker written through the direct map must come back
        // through the level-3 page descriptor.
        // SAFETY: the activated translation context maps this virtual address
        // to the original frame; the boot test runs at EL1 with PAN inactive.
        let observed = unsafe { *(0x1000_0000_u64 as *const u64) };
        assert_eq!(
            observed, MAGIC_ORIGINAL,
            "the activated context must serve the real mapping"
        );

        // Unmapping a non-empty table is rejected: L3 still holds the page.
        assert!(matches!(l3_pt.unmap(), Err(CapError::InvalidOperation)));
        // L2 still holds the L3 table descriptor.
        assert!(matches!(l2_pt.unmap(), Err(CapError::InvalidOperation)));

        // Frame.Unmap clears the descriptor and the record, and withdraws the
        // cached translation under the bound ASID (tlbi vae1is + dsb/isb) —
        // executing the real maintenance sequence here proves it is safe on
        // the live kernel context.
        frame
            .unmap()
            .unwrap_or_else(|error| panic!("Frame.Unmap failed: {:?}", error.code()));
        assert!(matches!(frame.unmap(), Err(CapError::NotMapped)));
        {
            const ADDR_MASK: u64 = 0x0000_FFFF_FFFF_F000;
            // SAFETY: see above.
            let read_entry = |paddr: u64, slot: usize| unsafe {
                *(PhysAddr::new(paddr).user_to_kernel().as_ptr::<u64>()).add(slot)
            };
            let l0e = read_entry(root_paddr, 0);
            let l1e = read_entry(l0e & ADDR_MASK, 0);
            let l2e = read_entry(l1e & ADDR_MASK, 128);
            assert_eq!(read_entry(l2e & ADDR_MASK, 0), 0, "the PTE must be cleared");
        }

        // The unmap above cleared the descriptor and invalidated the cached
        // translation under the bound ASID on the live context. Prove the
        // invalidation: map the second frame (distinct physical backing and
        // contents) at the same virtual address and read through it — a
        // stale cached entry would still serve the original frame's marker.
        FrameKey::from_key(second_frame_key)
            .map(
                boot_domain_key,
                0x1000_0000,
                Rights(Rights::READ | Rights::WRITE),
                0,
            )
            .unwrap_or_else(|error| panic!("second frame Frame.Map failed: {:?}", error.code()));
        // SAFETY: the activated context now maps this virtual address to the
        // second frame; PAN is inactive at EL1.
        let observed = unsafe { *(0x1000_0000_u64 as *const u64) };
        assert_eq!(
            observed, MAGIC_SECOND,
            "the unmap's TLB invalidation must withdraw the stale translation"
        );
        FrameKey::from_key(second_frame_key)
            .unmap()
            .unwrap_or_else(|error| panic!("second frame Frame.Unmap failed: {:?}", error.code()));

        // Restore the boot identity-map context; the remaining assertions walk
        // tables through the direct map and need no live Domain context.
        // SAFETY: the saved value is the boot TTBR0_EL1 installed by
        // `enable_mmu_and_drop_to_el1`.
        unsafe {
            TTBR0_EL1.set(boot_ttbr0);
            core::arch::asm!("isb", options(nostack));
        }

        // Withdraw the caller's low-half blocks before the table teardown
        // below: the L2 teardown requires an empty table.
        for block in low_block_keys {
            FrameKey::from_key(block)
                .unmap()
                .unwrap_or_else(|error| panic!("low-half Frame.Unmap failed: {:?}", error.code()));
        }

        // With the original unmapped, the physical extent is free in this
        // Domain again: the derived capability now maps at the second address,
        // proving the policy tracks live physical mappings, not capability
        // identity. The 2 MiB block remains mapped and disjoint, so the overlap
        // walk must not reject the unrelated extent.
        FrameKey::from_key(derived_frame_key)
            .map(
                boot_domain_key,
                0x1000_1000,
                Rights(Rights::READ | Rights::WRITE),
                0,
            )
            .unwrap_or_else(|error| {
                panic!("post-unmap derived Frame.Map failed: {:?}", error.code())
            });
        {
            const ADDR_MASK: u64 = 0x0000_FFFF_FFFF_F000;
            // SAFETY: see above.
            let read_entry = |paddr: u64, slot: usize| unsafe {
                *(PhysAddr::new(paddr).user_to_kernel().as_ptr::<u64>()).add(slot)
            };
            let l0e = read_entry(root_paddr, 0);
            let l1e = read_entry(l0e & ADDR_MASK, 0);
            let l2e = read_entry(l1e & ADDR_MASK, 128);
            let pte = read_entry(l2e & ADDR_MASK, 1);
            assert_eq!(
                pte & ADDR_MASK,
                frame_paddr,
                "the derived mapping's PTE must name the same frame"
            );
            assert!(pte & 0b1 != 0, "the derived PTE must be valid");
        }
        FrameKey::from_key(derived_frame_key)
            .unmap()
            .unwrap_or_else(|error| panic!("derived Frame.Unmap failed: {:?}", error.code()));
        // The 2 MiB block clears too.
        large_frame
            .unmap()
            .unwrap_or_else(|error| panic!("2 MiB Frame.Unmap failed: {:?}", error.code()));

        // Now the empty tables unmap cleanly, innermost first.
        l3_pt
            .unmap()
            .unwrap_or_else(|error| panic!("L3 PageTable.Unmap failed: {:?}", error.code()));
        l2_pt
            .unmap()
            .unwrap_or_else(|error| panic!("L2 PageTable.Unmap failed: {:?}", error.code()));
        l1_pt
            .unmap()
            .unwrap_or_else(|error| panic!("L1 PageTable.Unmap failed: {:?}", error.code()));
        root_pt
            .unmap()
            .unwrap_or_else(|error| panic!("root PageTable.Unmap failed: {:?}", error.code()));
        // The root unmap withdrew the whole context: every cached translation
        // under the bound ASID was invalidated (tlbi aside1is + dsb/isb).
        assert!(matches!(root_pt.unmap(), Err(CapError::NotMapped)));
        {
            let domain = nucleus
                .pools
                .domains
                .get_live(0)
                .unwrap_or_else(|| panic!("boot Domain missing"));
            assert_eq!(domain.translation_root, None);
        }

        // Page-table pool accounting: a batch that cannot fit releases its
        // partially allocated metadata slots, and a later smaller batch
        // succeeds (capacity 16, four tables carved so far).
        assert!(matches!(
            untyped.retype(
                ObjectType::PAGE_TABLE,
                12,
                13,
                &self_table,
                KeySlot(24).0,
                Rights::all(),
            ),
            Err(CapError::PoolExhausted)
        ));
        let refill = untyped
            .retype(
                ObjectType::PAGE_TABLE,
                12,
                12,
                &self_table,
                KeySlot(24).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("refill PageTable Retype failed: {:?}", error.code()));
        assert_eq!(refill.slot(), KeySlot(24));
        assert!(matches!(
            untyped.retype(
                ObjectType::PAGE_TABLE,
                12,
                1,
                &self_table,
                KeySlot(36).0,
                Rights::all(),
            ),
            Err(CapError::PoolExhausted)
        ));
    }

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

fn print_my_sp() {
    use aarch64_cpu::registers::Readable;
    let sp = aarch64_cpu::registers::SP.get();
    semi::println!("Current SP: {sp:016x}");
}

// ─────────────────────────────────────────────────────────────────────
// Bounce fixture domain (completion foundation, 2026-09-16)
// ─────────────────────────────────────────────────────────────────────

/// The bits Bounce delivers to the blocked boot domain.
#[cfg(feature = "debug_kernel")]
const BOUNCE_MAGIC_BITS: u64 = 0b1010_1010;

/// The Bounce fixture domain's entry (N4-A, 2026-09-16): a second EL1
/// execution context that unlocks the first blocked domain for testing.
///
/// Bounce first signals the notification the boot domain is blocked on
/// (waking it). It then serves one `EventCount` advance per N2 trigger:
/// trigger bit 0 requests a plain +3 advance, trigger bit 1 an overflowing
/// one (which completes waiters with the shared `CounterOverflow` error,
/// 2026-09-18). After two rounds it parks forever on N2 — nothing ever
/// signals it again, so the scheduler resumes the boot domain.
/// Bootstrap-era fixture mechanism, not the Phase 7 Activate contract: no
/// budget, no EL0 entry, no legal-transition enforcement beyond what this
/// path exercises.
#[cfg(feature = "debug_kernel")]
#[unsafe(no_mangle)]
extern "C" fn bounce_entry() -> ! {
    let n1 = NotificationKey::from_key(RawKey::new(KeySlot(1), 1));
    let n2 = NotificationKey::from_key(RawKey::new(KeySlot(2), 1));
    let ec = EventCountKey::from_key(RawKey::new(KeySlot(3), 1));
    n1.signal(BOUNCE_MAGIC_BITS)
        .unwrap_or_else(|error| panic!("Bounce: Notification.Signal failed: {:?}", error.code()));
    // Serve one EventCount advance per N2 trigger, then park forever.
    for round in 0..2_u64 {
        let trigger = n2
            .wait(NotificationKey::WAIT_INFINITE)
            .unwrap_or_else(|error| {
                panic!("Bounce: round {round} wait failed: {:?}", error.code())
            });
        let result = match trigger {
            // A plain +3 advance that satisfies the boot domain's target.
            0b1 => ec.advance(3),
            // The overflowing advance: it completes waiters with the shared
            // error and leaves the counter unchanged (selected 2026-09-18).
            0b10 => ec.advance(u64::MAX),
            other => panic!("Bounce: unexpected trigger bits {other:#x}"),
        };
        if trigger == 0b10 {
            assert!(
                matches!(result, Err(CapError::CounterOverflow)),
                "Bounce: round {round} advance should overflow"
            );
        } else {
            result.unwrap_or_else(|error| {
                panic!("Bounce: round {round} advance failed: {:?}", error.code())
            });
        }
    }
    // Park forever: this wait blocks, so the scheduler resumes the boot
    // domain. Reaching either arm below is a fixture failure.
    match n2.wait(NotificationKey::WAIT_INFINITE) {
        Ok(bits) => panic!("Bounce: unexpected wakeup with bits {bits:#x}"),
        Err(error) => panic!("Bounce: Notification.Wait failed: {:?}", error.code()),
    }
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

// ─────────────────────────────────────────────────────────────────────
// Bounce fixture domain (completion foundation, 2026-09-16)
// ─────────────────────────────────────────────────────────────────────

/// The bits Bounce delivers to the blocked boot domain.
const BOUNCE_MAGIC_BITS: u64 = 0b1010_1010;

/// The Bounce fixture domain's entry (N4-A, 2026-09-16): a second EL1
/// execution context that unlocks the first blocked domain for testing.
///
/// Bounce signals the notification the boot domain is blocked on (waking
/// it), then parks forever on its own notification — nothing ever signals
/// it, so the scheduler resumes the boot domain. Bootstrap-era fixture
/// mechanism, not the Phase 7 Activate contract: no budget, no EL0 entry,
/// no legal-transition enforcement beyond what this path exercises.
#[unsafe(no_mangle)]
extern "C" fn bounce_entry() -> ! {
    let n1 = NotificationKey::from_key(RawKey::new(KeySlot(1), 1));
    let n2 = NotificationKey::from_key(RawKey::new(KeySlot(2), 1));
    n1.signal(BOUNCE_MAGIC_BITS)
        .unwrap_or_else(|error| panic!("Bounce: Notification.Signal failed: {:?}", error.code()));
    // Park forever: this wait blocks, so the scheduler resumes the boot
    // domain. Reaching either arm below is a fixture failure.
    match n2.wait(NotificationKey::WAIT_INFINITE) {
        Ok(bits) => panic!("Bounce: unexpected wakeup with bits {bits:#x}"),
        Err(error) => panic!("Bounce: Notification.Wait failed: {:?}", error.code()),
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

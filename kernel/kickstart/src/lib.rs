//! Vesper boot library, shared by the two boot-kernel images:
//!
//! - **kickstart** — the real startup kernel that initializes the machine and
//!   will eventually bring up the whole system;
//! - **kicktest** — the e2e runtime/bootup-test kernel that reuses this boot
//!   path and runs the capability end-to-end suite against the real SVC path.
//!
//! Everything here is real boot machinery: early EL2 init, device-tree
//! parsing, nucleus image loading and mapping, the EL1 transition, and (in
//! [`bootstrap`]) construction of the initial kernel state. Test fixtures and
//! the e2e suite live in the kicktest crate, never here.

#![no_std]
#![allow(unused)]
#![feature(format_args_nl)]
#![feature(try_find)] // For DeviceTree iterators

mod boot_info;
pub mod bootstrap;
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
        device_tree::{DeviceTree, DeviceTreeProp},
        memory::{Alloc, BootAllocator},
    },
    aarch64_cpu::registers::{Readable, SPSR_EL2, Writeable},
    core::{cell::UnsafeCell, slice},
    fdt_rs::{
        base::DevTree,
        error::DevTreeError,
        prelude::{FallibleIterator, PropReader},
    },
    libaddress::{PhysAddr, VirtAddr},
    liblocking::interface::Mutex,
    libmapping::{AccessPermissions, AttributeFields, MemAttributes},
    libqemu::semihosting as semi,
};

unsafe extern "C" {
    static __INIT_START: UnsafeCell<()>;
    static __INIT_END: UnsafeCell<()>;
    static __FREE_MEMORY_START: UnsafeCell<()>;
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

/// Kernel early init code.
/// `arch` crate is responsible for calling it.
///
/// `run_entry` is the virtual address of the post-boot continuation that runs
/// after the MMU is enabled and execution has dropped to EL1 (the boot image's
/// own run function, e.g. kickstart's `kickstart_run` or kicktest's
/// `kicktest_run`).
///
/// Safety
///
/// - Only a single core must be active and running this function.
/// - The init calls in this function must appear in the correct order:
///     - MMU + Data caching must be activated at the earliest. Without it, any atomic operations,
///       e.g. the yet-to-be-introduced spinlocks in the device drivers (which currently employ
///       `IRQSafeNullLocks` instead of spinlocks), will fail to work (properly) on the `RPi` `SoCs`.
///
pub fn init_main_el2(dtb: u32, run_entry: u64) -> ! {
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
    // of some kind, and second, it can be assumed that any node at the root of
    // the tree is either directly attached to the processor bus, or is a
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
    //
    // The EL1 stack is not inserted here: `alloc_pages` already records every
    // named allocation into BOOT_INFO, and a second insert would be rejected
    // as an overlapping used region.
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
            run_entry,
            // el1_stack_top, // This is solely for the kernel
            __STACK_TOP.get() as u64,
        );
    }
}

/// Print the current stack pointer through semihosting.
pub fn print_my_sp() {
    use aarch64_cpu::registers::Readable;
    let sp = aarch64_cpu::registers::SP.get();
    semi::println!("Current SP: {sp:016x}");
}

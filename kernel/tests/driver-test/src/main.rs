#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! driver-test: Kickstart driver matching, end to end.
//!
//! The image bundles one driver, `driver-probe` (see `driver-protocol`),
//! serving `arm,pl011`, `brcm,bcm2835-aux` and `brcm,bcm2835-aux-uart`.
//! Kickstart loads one instance per enabled node that names one of them. On
//! raspi3b and raspi4b that is the PL011 `uart0`, which has a register page
//! to itself, and `aux` and `uart1`, which share one: the PL011 receives a
//! page-sized device Untyped, the other two copies of one device Frame.
//!
//! Before each instance starts, the boot Thread checks its init record
//! against the device tree and installs a report key badged with a bit for
//! the instance. Each probe maps its registers, reads one, acks its
//! interrupts, carves an object from its own memory, calls `echo_v1` through
//! its imports and reports; the boot Thread waits until every instance has.
//!
//! Every probe also exports `echo_v1`, and its `imports.if` uses it `via
//! clocks`: Kickstart wires `uart1` (`clocks = <&aux 0>`) to the `aux`
//! instance, and nothing for the others, whose first clock is the clock
//! manager. The boot Thread checks each init record's Return key and import
//! records; the probes check the calls, an exception included.
//!
//! In-guest assertions and the QEMU semihosting exit status are the result
//! (`just test-driver`, `just test-driver-rpi4`).

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    driver_echo::echo_v1::TYPE_CODE as ECHO_V1,
    driver_protocol::{AUX, AUX_UART, PL011, REPORT_SLOT},
    kickstart::{
        bootstrap::{bootstrap_nucleus, device_untypeds, retained_init_memory, spawn_idle},
        drivers::{BootKeys, LoadedDriver, load_drivers},
        kickstart_init_el2,
    },
    libcomponent_loader::{
        builder::{Builder, ImageArchive, ImageTarget, verify_retained_image},
        component::ttbr,
        keys::{BOOT_TABLE_SIZE_BITS, SlotCursor, boot_key},
        paging::image_table_count,
    },
    libdevice_driver::{PAGE_FRAME, PAGE_UNTYPED},
    libdevicetree::{interrupts::interrupts, is_compatible, is_enabled},
    // `libkicktest` supplies the panic handler.
    libkicktest as _,
    libobject::{
        KeySlot, KeyTableKey, NotificationKey, ObjectType, RawKey, Rights, UntypedKey,
        address_space::AddressSpaceKey, thread::ThreadReturnKey,
    },
    libqemu::semihosting as semi,
    nucleus::{api::key_entry::KeyEntry, objects::KeyTable},
};

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

/// First boot `KeyTable` slot the builder carves fixtures into.
const FIRST_FIXTURE_SLOT: u32 = 64;

/// Guard of the `KeyTable` archiving the builder's image Frames.
const BUILDER_ARCHIVE_GUARD: u32 = 0x0A_4C01;

/// Instances the board device trees produce: `uart0`, `aux` and `uart1`.
const INSTANCES: usize = 3;

/// What the boot Thread saw of one instance before it started.
#[derive(Clone, Copy, Default)]
struct Seen {
    compatible_index: u64,
    page_kind: u64,
    interrupts: u64,
}

pub fn run() -> ! {
    semi::println!("driver-test: enabled MMU and dropped to EL1");
    let retained = retained_init_memory();
    let image_tables = image_table_count(&retained);
    let boot = bootstrap_nucleus();
    let keys = BootKeys::of(&boot);
    let (nucleus, keytable_addr, boot_as_id) = (boot.nucleus, boot.keytable_addr, boot.boot_as_id);
    let untyped = UntypedKey::from_key(keys.untyped);
    let self_table = KeyTableKey::from_key(keys.self_table);
    let builder = Builder {
        untyped: &untyped,
        self_table: &self_table,
        boot_table_addr: keytable_addr,
        retained: &retained,
    };
    let devices = device_untypeds();
    let mut slots =
        SlotCursor::starting_at(FIRST_FIXTURE_SLOT).avoiding(devices.first_slot, devices.count);

    // ── The builder's own AddressSpace ────────────────────────────────────
    let builder_as_key = boot_key(KeySlot::SELF_ADDRESS_SPACE.0, 1);
    let builder_l2 = builder.root_chain(builder_as_key, &mut slots);
    assert_eq!(Builder::assign_asid(builder_as_key), 1);
    builder.map_retained_image(
        &[ImageTarget {
            address_space: builder_as_key,
            l2: builder_l2,
            first_table_slot: slots.take(u32::try_from(image_tables).unwrap_or(u32::MAX)),
        }],
        &ImageArchive {
            slot: slots.take(1),
            guard: BUILDER_ARCHIVE_GUARD,
            grant_scratch: slots.take(1),
            copy_scratch: slots.take(1),
        },
    );
    verify_retained_image(&retained, &[ttbr(nucleus, boot_as_id)]);
    AddressSpaceKey::from_key(builder_as_key)
        .activate()
        .unwrap_or_else(|error| panic!("builder Activate failed: {:?}", error.code()));

    // The instances the device tree asks for.
    let tree = kickstart::device_tree::at_el1().index();
    let expected = tree
        .nodes()
        .filter(|node| is_enabled(node))
        .filter(|node| {
            components::PROBE_DRIVER
                .compatible
                .iter()
                .any(|compatible| is_compatible(node, compatible))
        })
        .count();
    assert_eq!(expected, INSTANCES, "the board device tree changed");

    let report = untyped
        .retype(
            ObjectType::NOTIFICATION,
            0,
            0,
            1,
            &self_table,
            slots.take(1),
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("report Notification Retype failed: {:?}", error.code()));
    let report_id = builder
        .entry(report)
        .carved_id()
        .unwrap_or_else(|error| panic!("report identity: {:?}", error.code()));

    let _idle = spawn_idle(
        nucleus,
        keytable_addr,
        RawKey::from_wire(untyped.to_wire()),
        boot_as_id,
    );

    // ── Driver matching ───────────────────────────────────────────────────
    let mut seen = [Seen::default(); INSTANCES];
    let mut count = 0;
    let started = load_drivers(
        nucleus,
        &builder,
        keys,
        &[&components::PROBE_DRIVER],
        &mut slots,
        |loaded: &LoadedDriver<'_>| {
            let init = loaded.init;
            // The record matches the device tree node.
            let node = tree
                .nodes()
                .find(|node| node.name().ok() == Some(loaded.node))
                .expect("the loaded node is in the tree");
            assert_eq!(
                init.interrupts().len(),
                interrupts(tree, &node).count(),
                "{}: every interrupt is handed over",
                loaded.node
            );
            assert!(!init.pages().is_empty() && !init.regions().is_empty());
            assert_eq!(init.memory_size, 64 * 1024);
            // Its exports answer with the key at its table's Return slot.
            assert_eq!(
                init.return_key,
                ThreadReturnKey::provisioned(loaded.component.guard, BOOT_TABLE_SIZE_BITS)
                    .raw()
                    .to_wire()
            );
            // `uart1` imports three `echo_v1` functions of the `aux` instance,
            // on one target stack; the other nodes link to no probe.
            let imports = init.imports();
            if init.compatible_index == AUX_UART {
                assert_eq!(
                    imports.len(),
                    3,
                    "uart1 imports identify, twice and frame_base"
                );
                assert!(imports.iter().all(|record| record.type_code == ECHO_V1));
                assert_eq!(
                    [
                        imports[0].function,
                        imports[1].function,
                        imports[2].function
                    ],
                    [0, 1, 2]
                );
                assert_ne!(imports[0].invocation, imports[1].invocation);
                assert_eq!(imports[0].stack_end, imports[1].stack_end);
                assert_eq!(imports[0].stack_end, imports[2].stack_end);
                assert_ne!(imports[1].invocation, imports[2].invocation);
            } else {
                assert!(imports.is_empty(), "{}: links to no probe", loaded.node);
            }
            semi::println!(
                "driver-test: {} on {}: compatible {}, {} pages from {:#x}, {} interrupts",
                loaded.driver.component.name,
                loaded.node,
                init.compatible_index,
                init.pages().len(),
                init.pages()[0].paddr,
                init.interrupts().len()
            );
            seen[count] = Seen {
                compatible_index: init.compatible_index,
                page_kind: init.pages()[0].kind,
                interrupts: init.interrupts().len() as u64,
            };
            // The report key, badged with this instance's bit.
            // SAFETY: the driver's retained table; no invocation overlaps.
            unsafe { &mut *(loaded.component.table_addr as *mut KeyTable) }
                .insert(
                    KeySlot(REPORT_SLOT),
                    KeyEntry::new_carved(report_id, Rights(Rights::SEND), 1 << count),
                    loaded.component.guard,
                )
                .unwrap_or_else(|failure| {
                    panic!("report grant failed: {:?}", failure.error.code())
                });
            count += 1;
        },
    );
    assert_eq!(started, INSTANCES);

    // The PL011 has its page to itself; `aux` and `uart1` share one.
    for instance in &seen[..count] {
        match instance.compatible_index {
            PL011 => {
                assert_eq!(instance.page_kind, PAGE_UNTYPED);
                assert_eq!(instance.interrupts, 1);
            }
            AUX => {
                assert_eq!(instance.page_kind, PAGE_FRAME);
                assert_eq!(instance.interrupts, 0);
            }
            AUX_UART => {
                assert_eq!(instance.page_kind, PAGE_FRAME);
                assert_eq!(instance.interrupts, 1);
            }
            other => panic!("unexpected compatible index {other}"),
        }
    }

    // ── Every instance reports ────────────────────────────────────────────
    let report = NotificationKey::from_key(report);
    let all = (1_u64 << INSTANCES) - 1;
    let mut reported = 0;
    while reported != all {
        reported |= report
            .wait(NotificationKey::WAIT_INFINITE)
            .unwrap_or_else(|error| panic!("report Wait failed: {:?}", error.code()));
        semi::println!("driver-test: reports {reported:#b}");
    }

    semi::println!("driver-test: ✅ driver matching, handoff and probe checks passed");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            libcpu::endless_sleep()
        }
    }
}

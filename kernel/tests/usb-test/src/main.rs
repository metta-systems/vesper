#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! usb-test: the DWC2 host controller component and a client, loaded by
//! Kickstart.
//!
//! The image bundles the `usb-hcd-dwc2` component and `usb-test-client`.
//! Kickstart loads one controller instance per enabled `brcm,bcm2708-usb`
//! node, which is the Raspberry Pi 3's controller, and starts the client as a
//! program that imports `usb_hcd_v1` from it. The client resets the root port,
//! enumerates the device behind it over a control pipe and checks the pipe
//! rules. On raspi3b, QEMU attaches a hub there, so the root device must be a
//! hub.
//!
//! The boot Thread installs the report Notification key at `REPORT_SLOT` of
//! the client's table, as mailbox-test does. The client signals it once the
//! checks pass, and then the boot Thread exits the run successfully. A failed
//! check panics in the client and fails the run instead.
//!
//! In-guest assertions and the QEMU semihosting exit status are the result
//! (`just test-usb`).

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    driver_protocol::REPORT_SLOT,
    kickstart::{
        bootstrap::{bootstrap_nucleus, device_untypeds, retained_init_memory},
        drivers::{BootKeys, LoadedDriver, load_drivers},
        kickstart_init_el2,
    },
    libcomponent_loader::{
        builder::{Builder, ImageArchive, ImageTarget, verify_retained_image},
        component::ttbr,
        keys::{SlotCursor, boot_key},
        paging::image_table_count,
    },
    libdevice_driver::DATA_WINDOW_VA,
    libdevicetree::{is_compatible, is_enabled},
    // `libkicktest` supplies the panic handler.
    libkicktest as _,
    libobject::{
        KeySlot, KeyTableKey, NotificationKey, ObjectType, Rights, UntypedKey,
        address_space::AddressSpaceKey,
    },
    libqemu::semihosting as semi,
    nucleus::{api::key_entry::KeyEntry, objects::KeyTable},
    usb_hcd::usb_hcd_v1::TYPE_CODE as USB_HCD_V1,
};

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

/// First boot `KeyTable` slot the builder carves fixtures into.
const FIRST_FIXTURE_SLOT: u32 = 64;

/// Guard of the `KeyTable` archiving the builder's image Frames.
const BUILDER_ARCHIVE_GUARD: u32 = 0x0A_4C01;

/// Instances the board device tree produces: the Raspberry Pi 3's DWC2 node.
const INSTANCES: usize = 1;

pub fn run() -> ! {
    semi::println!("usb-test: enabled MMU and dropped to EL1");
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
            components::USB_DRIVER
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

    // ── Driver loading ────────────────────────────────────────────────────
    let mut controller_area = 0;
    let mut client_area = 0;
    let mut seen = 0;
    let started = load_drivers(
        nucleus,
        &builder,
        keys,
        &[
            &components::USB_DRIVER,
            &components::CLIENT_DRIVER,
            &components::DISPATCHER_DRIVER,
        ],
        &mut slots,
        |loaded: &LoadedDriver<'_>| {
            let init = loaded.init;
            let name = loaded.driver.component.name;
            let imports = init.imports();
            if name == components::USB.name {
                // The controller: registers, no imports, and one client with
                // a data area and a stack of its own.
                assert!(!init.pages().is_empty());
                assert!(imports.is_empty());
                let [area] = init.areas() else {
                    panic!("one client, one data area, found {}", init.areas().len());
                };
                assert_eq!(area.len, 4096);
                assert_eq!(
                    area.channel_base,
                    area.base + 4096,
                    "the channel follows the data area"
                );
                assert!(area.stack_bottom < area.stack_end);
                assert!(area.base >= DATA_WINDOW_VA);
                controller_area = area.base;
            } else {
                // The client: the three functions of `usb_hcd_v1` it uses, sharing one
                // data area with the controller. It has no registers.
                assert!(init.pages().is_empty());
                assert_eq!(imports.len(), 3, "the three functions of usb_hcd_v1");
                assert!(
                    imports.iter().all(|record| record.type_code == USB_HCD_V1
                        && record.area_len == 4096
                        && record.area_base == imports[0].area_base
                        && record.channel_base == record.area_base + 4096),
                    "the functions of one pair share one area"
                );
                assert!(imports.iter().map(|record| record.function).eq(0..3));
                client_area = imports[0].area_base;
                // The report key, in the client's table only.
                // SAFETY: the driver's retained table; no invocation overlaps.
                unsafe { &mut *KeyTable::at_carve(loaded.component.table_addr) }
                    .insert(
                        KeySlot(REPORT_SLOT),
                        KeyEntry::new_carved(report_id, Rights(Rights::SEND), 1),
                        loaded.component.guard,
                    )
                    .unwrap_or_else(|failure| {
                        panic!("report grant failed: {:?}", failure.error.code())
                    });
            }
            assert_ne!(
                init.return_key, 0,
                "{name}: exports answer with a Return key"
            );
            seen += 1;
            semi::println!(
                "usb-test: {name} on {}: {} pages, {} interrupts, {} imports",
                loaded.node,
                init.pages().len(),
                init.interrupts().len(),
                imports.len()
            );
        },
    );
    assert_eq!(
        started,
        INSTANCES + 2,
        "the instances, the client and the root dispatcher"
    );
    assert_eq!(seen, INSTANCES + 1);
    assert_eq!(
        client_area, controller_area,
        "the area the client was handed"
    );

    // ── The client reports once the hub is enumerated and the rules hold ──
    let bits = NotificationKey::from_key(report)
        .await_bits(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("report Wait failed: {:?}", error.code()));
    assert_eq!(bits, 1);

    semi::println!("usb-test: ✅ DWC2 root hub enumerated through usb_hcd_v1");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            libcpu::endless_sleep()
        }
    }
}

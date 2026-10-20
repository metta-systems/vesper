#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! usb-hid-test: the DWC2 host controller component, the PL011 driver, the USB
//! core, the USB HID component and the input-trace program, loaded by Kickstart.
//!
//! `usb-core` enumerates the hub on the root port and the keyboard behind it
//! (on raspi3b QEMU attaches both, `just test-usb-hid`). `usb-hid` claims the
//! keyboard's boot interface from it, puts the keyboard in boot protocol and
//! polls its interrupt endpoint through the controller. `input-trace` is a
//! client of `usb-hid`'s `input_v1`: it asks for the device descriptor and the
//! state, and traces every event on the PL011's serial channel.
//!
//! The boot Thread installs the report Notification key at `REPORT_SLOT` of
//! `input-trace`'s table. `input-trace` signals it once it has read the
//! keyboard's descriptor and state through `input_v1`, and then the boot Thread
//! exits the run successfully. A failed check panics in a component and fails the
//! run instead.
//!
//! The same image on a Raspberry Pi 3 (`just boot-usb-hid-test`) prints the
//! descriptors and, as keys are pressed, the key events on the UART.

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
    libdevicetree::{is_compatible, is_enabled},
    // `libkicktest` supplies the panic handler.
    libkicktest as _,
    libobject::{
        KeySlot, KeyTableKey, NotificationKey, ObjectType, Rights, UntypedKey,
        address_space::AddressSpaceKey,
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

/// Instances the board device tree produces: the Raspberry Pi 3's DWC2 node.
const INSTANCES: usize = 1;

pub fn run() -> ! {
    semi::println!("usb-hid-test: enabled MMU and dropped to EL1");
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
    let mut seen = 0;
    let started = load_drivers(
        nucleus,
        &builder,
        keys,
        &[
            &components::USB_DRIVER,
            &components::UART_DRIVER,
            &components::MAILBOX_DRIVER,
            &components::USB_CORE_DRIVER,
            &components::USB_HID_DRIVER,
            &components::INPUT_TRACE_DRIVER,
            &components::DISPATCHER_DRIVER,
        ],
        &mut slots,
        |loaded: &LoadedDriver<'_>| {
            let init = loaded.init;
            let name = loaded.driver.component.name;
            let imports = init.imports();
            if name == components::USB.name {
                // The controller: registers, one import (the mailbox, to power
                // its domain), and a client each for usb-core and usb-hid, each
                // with a data area and a channel.
                assert!(!init.pages().is_empty());
                assert_eq!(imports.len(), 1, "set_power_state of mailbox_v1");
                assert_eq!(init.areas().len(), 2, "usb-core and usb-hid");
                assert!(init.areas().iter().all(|area| area.channel_event != 0));
            } else if name == components::UART.name {
                // The serial driver: its registers, and a client attached.
                assert!(!init.pages().is_empty());
                assert!(imports.is_empty());
            } else if name == components::MAILBOX.name {
                // The mailbox: its registers, and the controller as a client.
                assert!(!init.pages().is_empty());
                assert!(imports.is_empty());
            } else if name == components::USB_CORE.name {
                // usb-core: the functions of `usb_hcd_v1` it uses, and usb_v1
                // served to usb-hid.
                assert!(init.pages().is_empty());
                assert_eq!(imports.len(), 3, "the three functions of usb_hcd_v1");
                assert_eq!(init.areas().len(), 1, "usb-hid is its client");
            } else if name == components::USB_HID.name {
                // usb-hid: usb_v1 from the core, one function of usb_hcd_v1,
                // and input-trace as its client.
                assert!(init.pages().is_empty());
                assert_eq!(imports.len(), 4, "three of usb_v1 and one of usb_hcd_v1");
                assert_eq!(init.areas().len(), 1, "input-trace is its client");
                assert_ne!(init.areas()[0].channel_base, 0);
                assert_ne!(init.areas()[0].channel_event, 0);
            } else if name == components::INPUT_TRACE.name {
                // input-trace: the two functions of input_v1 and the serial
                // channel; the report key goes in its table only.
                assert_ne!(init.serial.transmit_submitted, 0, "a serial channel");
                assert_eq!(imports.len(), 2, "descriptor and snapshot");
                assert_ne!(imports[0].channel_event, 0);
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
                "usb-hid-test: {name} on {}: {} pages, {} interrupts, {} imports",
                loaded.node,
                init.pages().len(),
                init.interrupts().len(),
                imports.len()
            );
        },
    );
    assert_eq!(
        started,
        INSTANCES + 6,
        "the controller, the PL011, the mailbox, usb-core, usb-hid, input-trace and the root dispatcher"
    );
    assert_eq!(seen, INSTANCES + 5);

    // ── input-trace reports once it has read the keyboard through input_v1 ──
    let bits = NotificationKey::from_key(report)
        .await_bits(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("report Wait failed: {:?}", error.code()));
    assert_eq!(bits, 1);

    semi::println!("usb-hid-test: ✅ usb-hid served the keyboard to input-trace");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            libcpu::endless_sleep()
        }
    }
}

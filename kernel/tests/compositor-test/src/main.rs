#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! compositor-test: the DWC2 host controller component, the PL011 driver, the
//! mailbox and fb components, the USB core, the USB HID component and the
//! compositor, loaded by Kickstart.
//!
//! `usb-core` enumerates the hub on the root port and the keyboard behind it
//! (on raspi3b QEMU attaches both, `just test-compositor`). `usb-hid` claims the
//! keyboard and serves it on `input_v1`. The compositor is that interface's
//! only client: it reads the device's descriptor and state, posts buffers on
//! the event channel and routes the records it gets to its windows, tracing
//! what each window receives on the PL011's serial channel. It also owns the
//! display: it maps the framebuffer through the fb component and draws the
//! background and the cursor, which it says under QEMU (the recipe checks).
//!
//! The boot Thread installs the report Notification key at `REPORT_SLOT` of the
//! compositor's table. The compositor signals it once it has read the
//! keyboard's descriptor and state, and then the boot Thread exits the run
//! successfully. A failed check panics in a component and fails the run instead.
//!
//! The same image on a Raspberry Pi 3 (`just boot-compositor-test`) traces on the
//! UART what the compositor routes as the mouse moves and keys are pressed.

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
    semi::println!("compositor-test: enabled MMU and dropped to EL1");
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
            &components::FB_DRIVER,
            &components::USB_CORE_DRIVER,
            &components::USB_HID_DRIVER,
            &components::COMPOSITOR_DRIVER,
            &components::DRAW_DEMO_DRIVER,
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
            } else if name == components::USB_CORE.name {
                // usb-core: the functions of `usb_hcd_v1` it uses, and usb_v1
                // served to usb-hid.
                assert!(init.pages().is_empty());
                assert_eq!(imports.len(), 4, "the four functions of usb_hcd_v1");
                assert_eq!(init.areas().len(), 1, "usb-hid is its client");
            } else if name == components::USB_HID.name {
                // usb-hid: usb_v1 from the core, one function of usb_hcd_v1,
                // and the compositor as its only client.
                assert!(init.pages().is_empty());
                assert_eq!(imports.len(), 4, "three of usb_v1 and one of usb_hcd_v1");
                assert_eq!(init.areas().len(), 1, "the compositor is its client");
                assert_ne!(init.areas()[0].channel_base, 0);
                assert_ne!(init.areas()[0].channel_event, 0);
            } else if name == components::COMPOSITOR.name {
                // The compositor: the five functions of fb_v1, the two of
                // input_v1 and the serial channel; the report key goes in its
                // table only.
                assert_ne!(init.serial.transmit_submitted, 0, "a serial channel");
                assert_eq!(imports.len(), 7, "five of fb_v1, descriptor and snapshot");
                assert!(imports.iter().any(|record| record.channel_event != 0));
                assert_ne!(init.map_window.l2, 0, "a window to map the framebuffer");
                assert!(init.map_window.tables.iter().all(|&table| table != 0));
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
                "compositor-test: {name} on {}: {} pages, {} interrupts, {} imports",
                loaded.node,
                init.pages().len(),
                init.interrupts().len(),
                imports.len()
            );
        },
    );
    assert_eq!(
        started,
        INSTANCES + 8,
        "the controller, the PL011, the mailbox, fb, usb-core, usb-hid, the compositor, draw-demo and the root dispatcher"
    );
    assert_eq!(seen, INSTANCES + 7);

    // ── The compositor reports once it has read the keyboard through input_v1 ──
    let bits = NotificationKey::from_key(report)
        .await_bits(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("report Wait failed: {:?}", error.code()));
    assert_eq!(bits, 1);

    semi::println!("compositor-test: ✅ the compositor owns the input stream");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            // On the board the components keep serving whoever types: this
            // Thread waits for good instead of stopping the CPU.
            loop {
                let _woken = NotificationKey::from_key(report)
                    .await_bits(NotificationKey::WAIT_INFINITE);
            }
        }
    }
}

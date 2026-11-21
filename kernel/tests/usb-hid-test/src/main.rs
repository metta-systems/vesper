#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! usb-hid-test: the DWC2 host controller component, the PL011 driver, the USB
//! core, the USB HID component and the input-trace program, loaded by Kickstart.
//!
//! `usb-core` enumerates the hub on the root port and the keyboard behind it
//! (on raspi3b QEMU attaches both, `just _test-usb-hid`). `usb-hid` claims the
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
//! The same image on a Raspberry Pi 3 (`just _boot-usb-hid-test`) prints the
//! descriptors and, as keys are pressed, the key events on the UART.

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    driver_protocol::REPORT_SLOT,
    kickstart::{
        device_tree::enabled_nodes,
        image::{BUILDER_ARCHIVE_GUARD, BootObjects},
        kickstart_init_el2,
    },
    // `libkicktest` supplies the panic handler.
    libkicktest as _,
    libobject::{KeySlot, NotificationKey, Rights},
    libqemu::semihosting as semi,
    nucleus::{api::key_entry::KeyEntry, objects::KeyTable},
};

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

pub fn run() -> ! {
    semi::println!("usb-hid-test: enabled MMU and dropped to EL1");
    let mut boot = BootObjects::bootstrap();
    let mut image = boot.builder_address_space(BUILDER_ARCHIVE_GUARD);

    // The test drives the DWC2 controller: the Raspberry Pi 3's one node
    // (QEMU's raspi3b).
    assert_eq!(
        enabled_nodes(components::USB_DRIVER.compatible),
        1,
        "the board device tree has one enabled DWC2 node"
    );

    let report = image.notification();
    let report_id = image
        .builder
        .entry(report)
        .carved_id()
        .unwrap_or_else(|error| panic!("report identity: {:?}", error.code()));

    // ── Driver loading ────────────────────────────────────────────────────
    image.start_drivers(components::DRIVERS, |loaded, _builder| {
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
            // served to usb-hid and to input-trace (its trace).
            assert!(init.pages().is_empty());
            assert_eq!(imports.len(), 7, "the seven functions of usb_hcd_v1");
            assert_eq!(
                init.areas().len(),
                2,
                "usb-hid and input-trace are its clients"
            );
        } else if name == components::USB_HID.name {
            // usb-hid: usb_v1 from the core, one function of usb_hcd_v1,
            // and input-trace as its client.
            assert!(init.pages().is_empty());
            assert_eq!(imports.len(), 4, "three of usb_v1 and one of usb_hcd_v1");
            assert_eq!(init.areas().len(), 1, "input-trace is its client");
            assert_ne!(init.areas()[0].channel_base, 0);
            assert_ne!(init.areas()[0].channel_event, 0);
        } else if name == components::INPUT_TRACE.name {
            // input-trace: three functions of input_v1, usb_v1's trace and
            // the serial channel; the report key goes in its table only.
            assert_ne!(init.serial.transmit_submitted, 0, "a serial channel");
            assert_eq!(imports.len(), 4, "descriptor, snapshot and two traces");
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
        semi::println!(
            "usb-hid-test: {name} on {}: {} pages, {} interrupts, {} imports",
            loaded.node,
            init.pages().len(),
            init.interrupts().len(),
            imports.len()
        );
    });

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
            // On the board the components keep serving whoever types: this
            // Thread waits for good instead of stopping the CPU.
            loop {
                let _woken = NotificationKey::from_key(report)
                    .await_bits(NotificationKey::WAIT_INFINITE);
            }
        }
    }
}

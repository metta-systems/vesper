#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! usb-core-test: the DWC2 host controller component, the PL011 driver and the USB
//! core, loaded by Kickstart.
//!
//! The image bundles `usb-hcd-dwc2`, `uart-pl011` and `usb-core`. Kickstart loads
//! one controller instance per enabled `brcm,bcm2708-usb` node, which is the
//! Raspberry Pi 3's controller, and starts `usb-core` as a program that imports
//! `usb_hcd_v1` from it and traces on the PL011's serial channel. On raspi3b,
//! QEMU attaches a hub on the root port and a keyboard behind it
//! (`just _test-usb-core`): `usb-core` enumerates both.
//!
//! The boot Thread installs the report Notification key at `REPORT_SLOT` of
//! `usb-core`'s table. `usb-core` signals it once every port of the hub has settled
//! with a device behind the hub, and then the boot Thread exits the run
//! successfully. A failed check panics in `usb-core` and fails the run instead.
//!
//! The same image on a Raspberry Pi 3 prints `usb-core`'s trace on the UART.
//!
//! In-guest assertions and the QEMU semihosting exit status are the result
//! (`just _test-usb-core`).

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
    libdevice_driver::DATA_WINDOW_VA,
    // `libkicktest` supplies the panic handler.
    libkicktest as _,
    libobject::{KeySlot, NotificationKey, Rights},
    libqemu::semihosting as semi,
    nucleus::{api::key_entry::KeyEntry, objects::KeyTable},
    usb_hcd::usb_hcd_v1::TYPE_CODE as USB_HCD_V1,
};

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

pub fn run() -> ! {
    semi::println!("usb-core-test: enabled MMU and dropped to EL1");
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
    let mut controller_area = 0;
    let mut client_area = 0;
    image.start_drivers(components::DRIVERS, |loaded, _builder| {
        let init = loaded.init;
        let name = loaded.driver.component.name;
        let imports = init.imports();
        if name == components::USB.name {
            // The controller: registers, one import (the mailbox, to power
            // its domain), and one client with a data area and a stack of
            // its own.
            assert!(!init.pages().is_empty());
            assert_eq!(imports.len(), 1, "set_power_state of mailbox_v1");
            let [area] = init.areas() else {
                panic!("one client, one data area, found {}", init.areas().len());
            };
            assert_eq!(area.len, 4096);
            assert_eq!(
                area.channel_base,
                area.base + 4096,
                "the channel follows the data area"
            );
            assert_ne!(init.export_pool, 0, "the controller serves calls on a pool");
            assert!(area.base >= DATA_WINDOW_VA);
            controller_area = area.base;
        } else if name == components::UART.name {
            // The serial driver: its registers, and a client attached.
            assert!(!init.pages().is_empty());
            assert!(imports.is_empty());
        } else if name == components::MAILBOX.name {
            // The mailbox: its registers, and the controller as a client.
            assert!(!init.pages().is_empty());
            assert!(imports.is_empty());
        } else {
            // usb-core: the seven functions of `usb_hcd_v1` it uses, sharing one
            // data area with the controller, and a serial channel. It has no
            // registers.
            assert_ne!(init.serial.transmit_submitted, 0, "a serial channel");
            assert!(init.pages().is_empty());
            assert_eq!(imports.len(), 7, "the seven functions of usb_hcd_v1");
            assert!(
                imports.iter().all(|record| record.type_code == USB_HCD_V1
                    && record.area_len == 4096
                    && record.area_base == imports[0].area_base
                    && record.channel_base == record.area_base + 4096),
                "the functions of one pair share one area"
            );
            assert!(imports.iter().map(|record| record.function).eq(0..7));
            client_area = imports[0].area_base;
            // The report key, in usb-core's table only.
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
            "usb-core-test: {name} on {}: {} pages, {} interrupts, {} imports",
            loaded.node,
            init.pages().len(),
            init.interrupts().len(),
            imports.len()
        );
    });
    assert_eq!(client_area, controller_area, "the area usb-core was handed");

    // ── usb-core reports once the hub and the devices behind it are enumerated ──
    let bits = NotificationKey::from_key(report)
        .await_bits(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("report Wait failed: {:?}", error.code()));
    assert_eq!(bits, 1);

    semi::println!("usb-core-test: ✅ usb-core enumerated the hub and the devices behind it");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            // On the board the components keep running for whoever plugs
            // devices in: this Thread waits for good instead of stopping the CPU.
            loop {
                let _woken = NotificationKey::from_key(report)
                    .await_bits(NotificationKey::WAIT_INFINITE);
            }
        }
    }
}

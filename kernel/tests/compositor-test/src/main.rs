#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! compositor-test: the DWC2 host controller component, the PL011 driver, the
//! mailbox and fb components, the USB core, the USB HID component and the
//! compositor, loaded by Kickstart.
//!
//! `usb-core` enumerates the hub on the root port and the keyboard behind it
//! (on raspi3b QEMU attaches both, `just _test-compositor`). `usb-hid` claims the
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
//! The same image on a Raspberry Pi 3 (`just _boot-compositor-test`) traces on the
//! UART what the compositor routes as the mouse moves and keys are pressed.

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
    semi::println!("compositor-test: enabled MMU and dropped to EL1");
    let mut boot = BootObjects::bootstrap();
    let mut image = boot.builder_address_space(BUILDER_ARCHIVE_GUARD);

    // The keyboard is on the Raspberry Pi 3's DWC2 controller.
    assert_eq!(
        enabled_nodes(components::USB_DRIVER.compatible),
        1,
        "one enabled DWC2 node"
    );

    let report = image.notification();
    let report_id = image
        .builder
        .entry(report)
        .carved_id()
        .unwrap_or_else(|error| panic!("report identity: {:?}", error.code()));

    // ── Driver loading ────────────────────────────────────────────────────
    let mut seen = 0;
    let started = image.start_drivers(components::DRIVERS, |loaded, _builder| {
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
            assert_eq!(imports.len(), 7, "the seven functions of usb_hcd_v1");
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
    });
    // The root dispatcher is bound, not shown to the callback.
    assert_eq!(seen, started - 1);

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

#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! window-events-test: compositor-test's components (the DWC2 host controller
//! component, the PL011 driver, the mailbox and fb components, the USB core,
//! the USB HID component and the compositor) plus window-events, a client of
//! the compositor, loaded by Kickstart.
//!
//! window-events makes two windows and reads the events the compositor routes
//! to them on its event channel, while `just _test-window-events` clicks, drags
//! and types through the QEMU monitor on the USB mouse and keyboard. The
//! program ends the run: status 0 when every expected event arrived, 1
//! otherwise.
//!
//! As in compositor-test, the boot Thread installs the report Notification key
//! at `REPORT_SLOT` of the compositor's table and the compositor signals it
//! once it has read a device. Then the boot Thread waits for good. A failed
//! check panics in a component and ends the run under QEMU.

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    driver_protocol::REPORT_SLOT,
    kickstart::{
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
    semi::println!("window-events-test: enabled MMU and dropped to EL1");
    let mut boot = BootObjects::bootstrap();
    let mut image = boot.builder_address_space(BUILDER_ARCHIVE_GUARD);

    let report = image.notification();
    let report_id = image
        .builder
        .entry(report)
        .carved_id()
        .unwrap_or_else(|error| panic!("report identity: {:?}", error.code()));

    // ── Driver loading: what the device tree asks for ─────────────────────
    image.start_drivers(components::DRIVERS, |loaded, _builder| {
        let init = loaded.init;
        let name = loaded.driver.component.name;
        let imports = init.imports();
        if name == components::COMPOSITOR.name {
            // The compositor serves window-events, which has an event
            // channel; the report key goes in the compositor's table only.
            assert_eq!(init.areas().len(), 1, "window-events is its client");
            assert_ne!(init.areas()[0].channel_event, 0, "an event channel");
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
        } else if name == components::WINDOW_EVENTS.name {
            // window-events: four functions of compositor_v1 and its
            // event channel.
            assert_eq!(imports.len(), 4, "four functions of compositor_v1");
            assert!(imports.iter().all(|record| record.channel_event != 0));
        }
        semi::println!(
            "window-events-test: {name} on {}: {} pages, {} interrupts, {} imports",
            loaded.node,
            init.pages().len(),
            init.interrupts().len(),
            imports.len()
        );
    });

    // ── The compositor reports once it has read a device through input_v1 ──
    let bits = NotificationKey::from_key(report)
        .await_bits(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("report Wait failed: {:?}", error.code()));
    assert_eq!(bits, 1);
    semi::println!("window-events-test: the compositor owns the input stream");
    // window-events ends the run; until then this Thread waits for good.
    loop {
        let _woken = NotificationKey::from_key(report).await_bits(NotificationKey::WAIT_INFINITE);
    }
}

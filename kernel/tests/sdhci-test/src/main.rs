#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! sdhci-test: the SDHCI component and a client, loaded by Kickstart.
//!
//! The image bundles the `sdhci` component, the mailbox component it may ask
//! for the base clock, and `sd-test-client`. Kickstart loads the controller
//! for every enabled node it serves: the Arasan controller
//! (`brcm,bcm2835-sdhci`), which holds QEMU's card on raspi3b and raspi4b,
//! and on raspi4b also EMMC2 (`brcm,bcm2711-emmc2`), which QEMU gives no
//! card. The client is a program that imports `block_v1` from the Arasan
//! instance. It reads marked blocks of the card image `just _test-sdhci`
//! attaches, writes blocks and reads them back, one and several per request,
//! and checks the requests the driver must refuse. QEMU's Arasan controller
//! has no ADMA2, so the blocks move through the data port.
//!
//! The boot Thread installs the report Notification key at `REPORT_SLOT` of
//! the client's table, as mailbox-test does. The client signals it once the
//! checks pass, and then the boot Thread exits the run successfully. A failed
//! check panics in the client and fails the run instead.
//!
//! In-guest assertions and the QEMU semihosting exit status are the result
//! (`just _test-sdhci`).

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    block::block_v1::{CHANNEL_PAGES, TYPE_CODE as BLOCK_V1},
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
};

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

pub fn run() -> ! {
    semi::println!("sdhci-test: enabled MMU and dropped to EL1");
    let mut boot = BootObjects::bootstrap();
    let mut image = boot.builder_address_space(BUILDER_ARCHIVE_GUARD);

    // The instances the device tree asks for: the Arasan controller, and on
    // the Raspberry Pi 4 EMMC2 as well.
    let instances = enabled_nodes(components::SD_DRIVER.compatible);
    assert!(
        (1..=2).contains(&instances),
        "the board device tree changed: {instances} controllers"
    );

    let report = image.notification();
    let report_id = image
        .builder
        .entry(report)
        .carved_id()
        .unwrap_or_else(|error| panic!("report identity: {:?}", error.code()));

    // ── Driver loading ────────────────────────────────────────────────────
    let mut controller_channel = 0;
    let mut client_channel = 0;
    let mut served = 0;
    image.start_drivers(components::DRIVERS, |loaded, _builder| {
        let init = loaded.init;
        let name = loaded.driver.component.name;
        let imports = init.imports();
        if name == components::SD.name {
            // A controller: registers, its interrupt line and the
            // mailbox's `get_clock_rate`. On the Raspberry Pi 4 the
            // Arasan controller and EMMC2 share one line, which the first
            // instance receives; the other polls.
            assert!(!init.pages().is_empty());
            assert!(init.interrupts().len() <= 1, "the controller's line");
            assert_eq!(imports.len(), 1, "get_clock_rate of mailbox_v1");
            // The client imports from the Arasan instance only.
            match init.areas() {
                [] => {}
                [area] => {
                    assert_eq!(area.len, 0, "block_v1 has no data area");
                    assert_ne!(area.channel_base, 0, "block_v1 has a channel");
                    assert_ne!(area.channel_event, 0, "the channel has an EventCount");
                    assert!(area.channel_base >= DATA_WINDOW_VA);
                    assert_ne!(init.export_pool, 0, "the controller serves calls on a pool");
                    controller_channel = area.channel_base;
                    served += 1;
                }
                areas => panic!("at most one client, found {}", areas.len()),
            }
        } else if name == components::MAILBOX.name {
            // The mailbox: its registers, and the controllers as clients.
            assert!(!init.pages().is_empty());
            assert!(imports.is_empty());
        } else {
            // The client: both functions of `block_v1`, sharing one
            // channel with the controller. It has no registers.
            assert!(init.pages().is_empty());
            assert_eq!(imports.len(), 2, "the two functions of block_v1");
            assert!(
                imports.iter().all(|record| record.type_code == BLOCK_V1
                    && record.area_len == 0
                    && record.channel_base != 0
                    && record.channel_base == imports[0].channel_base
                    && record.channel(CHANNEL_PAGES).is_some()),
                "the functions of one pair share one channel"
            );
            assert!(imports.iter().map(|record| record.function).eq(0..2));
            client_channel = imports[0].channel_base;
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
        semi::println!(
            "sdhci-test: {name} on {}: {} pages, {} interrupts, {} imports",
            loaded.node,
            init.pages().len(),
            init.interrupts().len(),
            imports.len()
        );
    });
    assert_eq!(served, 1, "one controller serves the client");
    assert_eq!(
        client_channel, controller_channel,
        "the channel the client was handed"
    );

    // ── The client reports once the blocks moved and the rules hold ───────
    let bits = NotificationKey::from_key(report)
        .await_bits(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("report Wait failed: {:?}", error.code()));
    assert_eq!(bits, 1);

    semi::println!("sdhci-test: ✅ SD card read and written through block_v1");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            libcpu::endless_sleep()
        }
    }
}

#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! sd-test: the SDHOST component and a client, loaded by Kickstart.
//!
//! The image bundles the `sdhost-bcm2835` component, the mailbox component it
//! asks for the core clock, the gpio component it routes the card's pins
//! through, and `sd-test-client`. Kickstart loads the
//! controller for the enabled `brcm,bcm2835-sdhost` node, the Raspberry Pi 3's
//! SD card slot, and starts the client as a program that imports `block_v1`
//! from it. The client reads marked blocks of the card image `just _test-sd`
//! attaches, writes a block and reads it back, and checks the requests the
//! driver must refuse.
//!
//! The boot Thread installs the report Notification key at `REPORT_SLOT` of
//! the client's table, as mailbox-test does. The client signals it once the
//! checks pass, and then the boot Thread exits the run successfully. A failed
//! check panics in the client and fails the run instead.
//!
//! In-guest assertions and the QEMU semihosting exit status are the result
//! (`just _test-sd`).

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    block::block_v1::{CHANNEL_PAGES, TYPE_CODE as BLOCK_V1},
    driver_protocol::REPORT_SLOT,
    gpio::gpio_v1::TYPE_CODE as GPIO_V1,
    kickstart::{
        device_tree::enabled_nodes,
        image::{BUILDER_ARCHIVE_GUARD, BootObjects},
        kickstart_init_el2,
    },
    libdevice_driver::{DATA_WINDOW_VA, NO_PULL},
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
    semi::println!("sd-test: enabled MMU and dropped to EL1");
    let mut boot = BootObjects::bootstrap();
    let mut image = boot.builder_address_space(BUILDER_ARCHIVE_GUARD);

    // The card is on the Raspberry Pi 3's SDHOST node.
    assert_eq!(
        enabled_nodes(components::SD_DRIVER.compatible),
        1,
        "one enabled SDHOST node"
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
    let mut seen = 0;
    let started = image.start_drivers(components::DRIVERS, |loaded, _builder| {
        let init = loaded.init;
        let name = loaded.driver.component.name;
        let imports = init.imports();
        if name == components::SD.name {
            // The controller: registers, three imports (the mailbox, for
            // the core clock, and the gpio component, for the card's
            // pins), and one client with an io-channel and no data area.
            assert!(!init.pages().is_empty());
            assert_eq!(
                imports.len(),
                3,
                "get_clock_rate of mailbox_v1, set_function and set_pull of gpio_v1"
            );
            assert!(
                imports
                    .iter()
                    .filter(|record| record.type_code == GPIO_V1)
                    .map(|record| record.function)
                    .eq(0..2),
                "both functions of gpio_v1"
            );
            // Its node's pin group: the card's pins on ALT0, no pulls.
            assert!(
                init.pins()
                    .iter()
                    .map(|setting| (setting.pin, setting.function, setting.pull))
                    .eq((48..54).map(|pin| (pin, 4, NO_PULL))),
                "the card's pins from the device tree"
            );
            let [area] = init.areas() else {
                panic!("one client, one channel, found {}", init.areas().len());
            };
            assert_eq!(area.len, 0, "block_v1 has no data area");
            assert_ne!(area.channel_base, 0, "block_v1 has a channel");
            assert_ne!(area.channel_event, 0, "the channel has an EventCount");
            assert_ne!(init.export_pool, 0, "the controller serves calls on a pool");
            assert!(area.channel_base >= DATA_WINDOW_VA);
            controller_channel = area.channel_base;
        } else if name == components::MAILBOX.name || name == components::GPIO.name {
            // The mailbox and the gpio component: their registers, and
            // the controller as a client.
            assert!(!init.pages().is_empty());
            assert!(imports.is_empty());
        } else {
            // The client: both functions of `block_v1`, sharing one
            // channel with the controller, and `set_pull` of `gpio_v1`.
            // It has no registers.
            assert!(init.pages().is_empty());
            assert_eq!(imports.len(), 3, "block_v1 and set_pull of gpio_v1");
            let block = || imports.iter().filter(|record| record.type_code == BLOCK_V1);
            assert_eq!(block().count(), 2, "the two functions of block_v1");
            assert!(
                block().all(|record| record.area_len == 0
                    && record.channel_base != 0
                    && record.channel_base == imports[0].channel_base
                    && record.channel(CHANNEL_PAGES).is_some()),
                "the functions of one pair share one channel"
            );
            assert!(block().map(|record| record.function).eq(0..2));
            assert!(
                imports
                    .iter()
                    .any(|record| record.type_code == GPIO_V1 && record.function == 1),
                "set_pull of gpio_v1"
            );
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
        seen += 1;
        semi::println!(
            "sd-test: {name} on {}: {} pages, {} interrupts, {} imports",
            loaded.node,
            init.pages().len(),
            init.interrupts().len(),
            imports.len()
        );
    });
    // The root dispatcher is bound, not shown to the callback.
    assert_eq!(seen, started - 1);
    assert_eq!(
        client_channel, controller_channel,
        "the channel the client was handed"
    );

    // ── The client reports once the blocks moved and the rules hold ───────
    let bits = NotificationKey::from_key(report)
        .await_bits(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("report Wait failed: {:?}", error.code()));
    assert_eq!(bits, 1);

    semi::println!("sd-test: ✅ SD card read and written through block_v1");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            libcpu::endless_sleep()
        }
    }
}

#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! audio-test: the clock manager component, the DMA controller component and
//! the PWM audio driver, loaded by Kickstart (stages A1 and A2 of
//! `doc/agentic/audio-implementation-plan.md`).
//!
//! The image bundles `cprman-bcm2835`, which serves `clock_v1` for the
//! `brcm,bcm2835-cprman` node, `dma-bcm2835`, which serves `dma_v1` for the
//! `brcm,bcm2835-dma` node, `audio-pwm-bcm2835` for the `brcm,bcm2835-pwm`
//! node, the gpio component it routes the jack's pins through, and the PL011
//! driver, whose serial channel the audio driver traces on. The PWM node is
//! disabled in the stock device trees: `just test-audio` and `just test-dma`
//! boot QEMU with the test tree that `targets/overlays/audio-test-pwm.dts`
//! makes, and the board needs `dtoverlay=pwm-2chan,pin=40,func=4,pin2=41,func2=4`
//! in `config.txt`.
//!
//! The boot Thread checks what Kickstart hands the DMA controller: its page,
//! its channel mask and each of its interrupt lines once (the node lists the
//! line of channels 11 to 14 four times).
//!
//! The audio driver sets the PWM clock to 100 MHz through `clock_v1` and reads
//! it back. Under QEMU, which emulates the clock manager and the DMA
//! controller but not the PWM block, it then checks that `clock_v1` refuses an
//! unknown clock and an unreachable rate, and copies memory through `dma_v1`
//! with chains of linked control blocks, waiting for one completion per block
//! (QEMU's DMA model runs a chain inside the write that starts it, so no ring
//! runs there); on the board it plays a 440 Hz test tone on the left, the
//! right and both sides by polling, then from a DMA ring, measuring the sample
//! rate. Either way it signals the report Notification key the boot Thread
//! installs at `REPORT_SLOT` of its table, and the boot Thread ends the QEMU
//! run successfully. A failed check panics and fails the run instead.

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    clock::clock_v1::TYPE_CODE as CLOCK_V1,
    dma::dma_v1::{
        CHANNEL_PAGES as DMA_CHANNEL_PAGES, FUNCTION_COUNT as DMA_V1_FUNCTIONS, TYPE_CODE as DMA_V1,
    },
    driver_protocol::REPORT_SLOT,
    gpio::gpio_v1::TYPE_CODE as GPIO_V1,
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
    libdevice_driver::NO_PULL,
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

/// The jack's pins in the PWM node's pin group: GPIO40 and GPIO41 on ALT0
/// (`brcm,function` 4), no pulls.
const JACK_PINS: [(u32, u32, u32); 2] = [(40, 4, NO_PULL), (41, 4, NO_PULL)];

/// The Raspberry Pi 3 tree's `brcm,dma-channel-mask`: channels 0, 2, 4, 5
/// and 8 to 14.
const DMA_CHANNEL_MASK: u64 = 0x7f35;

/// The DMA node's interrupts once each line is issued once: `dma0` to
/// `dma10`, the line `dma11` to `dma14` share, and `dma-shared-all`.
const DMA_INTERRUPTS: usize = 13;

/// Enabled nodes of the tree compatible with one of `compatible`.
fn enabled_nodes(compatible: &[&str]) -> usize {
    let tree = kickstart::device_tree::at_el1().index();
    tree.nodes()
        .filter(|node| is_enabled(node))
        .filter(|node| compatible.iter().any(|name| is_compatible(node, name)))
        .count()
}

pub fn run() -> ! {
    semi::println!("audio-test: enabled MMU and dropped to EL1");
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

    // One clock manager and one enabled PWM node: the stock trees leave the
    // PWM node disabled.
    assert_eq!(
        enabled_nodes(components::CPRMAN_DRIVER.compatible),
        1,
        "the clock manager node"
    );
    assert_eq!(
        enabled_nodes(components::AUDIO_DRIVER.compatible),
        1,
        "no enabled PWM node: boot with the test tree (just test-audio) or the pwm-2chan overlay"
    );

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
    let mut audio_channel = 0;
    let mut dma_channel = 0;
    let started = load_drivers(
        nucleus,
        &builder,
        keys,
        &[
            &components::AUDIO_DRIVER,
            &components::CPRMAN_DRIVER,
            &components::DMA_DRIVER,
            &components::GPIO_DRIVER,
            &components::UART_DRIVER,
            &components::DISPATCHER_DRIVER,
        ],
        &mut slots,
        |loaded: &LoadedDriver<'_>| {
            let init = loaded.init;
            let name = loaded.driver.component.name;
            let imports = init.imports();
            if name == components::AUDIO.name {
                // The audio driver: the PWM block's page, three functions of
                // clock_v1, set_function of gpio_v1, the jack's pins from the
                // node's pin group and the serial channel.
                assert_eq!(init.pages().len(), 1, "the PWM block's page");
                assert!(
                    imports
                        .iter()
                        .filter(|record| record.type_code == CLOCK_V1)
                        .map(|record| record.function)
                        .eq([0, 1, 3]),
                    "set_rate, enable and rate of clock_v1 (not disable, 2)"
                );
                assert!(
                    imports
                        .iter()
                        .filter(|record| record.type_code == GPIO_V1)
                        .map(|record| record.function)
                        .eq(0..1),
                    "set_function of gpio_v1"
                );
                let dma = || imports.iter().filter(|record| record.type_code == DMA_V1);
                assert!(
                    dma()
                        .map(|record| record.function)
                        .eq(0..DMA_V1_FUNCTIONS as u64),
                    "every function of dma_v1"
                );
                assert!(
                    dma().all(|record| record.channel_base != 0
                        && record.channel_base == dma().next().unwrap().channel_base
                        && record.channel(DMA_CHANNEL_PAGES).is_some()),
                    "the functions of dma_v1 share one io-channel"
                );
                assert_eq!(imports.len(), 4 + DMA_V1_FUNCTIONS);
                audio_channel = dma().next().map_or(0, |record| record.channel_base);
                assert!(
                    init.pins()
                        .iter()
                        .map(|setting| (setting.pin, setting.function, setting.pull))
                        .eq(JACK_PINS),
                    "the jack's pins from the device tree"
                );
                assert_ne!(init.serial.transmit_submitted, 0, "a serial channel");
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
            } else if name == components::DMA.name {
                // The DMA controller: its page, the channel mask, each
                // interrupt line once (the node lists line 27 four times),
                // no imports, and the audio driver as its client.
                assert_eq!(init.pages().len(), 1, "the DMA controller's page");
                assert_eq!(init.dma_channel_mask, DMA_CHANNEL_MASK);
                let interrupts = init.interrupts();
                assert_eq!(
                    interrupts.len(),
                    DMA_INTERRUPTS,
                    "dma0 to dma11, dma-shared-all"
                );
                for (index, interrupt) in interrupts.iter().enumerate() {
                    assert!(
                        interrupts[..index]
                            .iter()
                            .all(|earlier| earlier.line != interrupt.line),
                        "interrupt line {} issued once",
                        interrupt.line
                    );
                }
                assert!(imports.is_empty());
                assert_ne!(init.export_pool, 0, "the DMA controller serves calls");
                let [area] = init.areas() else {
                    panic!("one client, found {}", init.areas().len());
                };
                assert_ne!(area.channel_base, 0, "dma_v1 has an io-channel");
                assert_ne!(area.channel_event, 0, "the io-channel has an EventCount");
                dma_channel = area.channel_base;
            } else if name == components::CPRMAN.name {
                // The clock manager: its two pages, no imports, and the audio
                // driver as its client.
                assert_eq!(init.pages().len(), 2, "the clock manager's two pages");
                assert!(imports.is_empty());
                assert_ne!(init.export_pool, 0, "the clock manager serves calls");
            } else if name != components::DISPATCHER.name {
                // The gpio component and the PL011 driver: their registers.
                assert!(!init.pages().is_empty());
                assert!(imports.is_empty());
            }
            seen += 1;
            semi::println!(
                "audio-test: {name} on {}: {} pages, {} interrupts, {} imports",
                loaded.node,
                init.pages().len(),
                init.interrupts().len(),
                imports.len()
            );
        },
    );
    assert_eq!(
        started, 6,
        "the audio driver, the clock manager, the DMA controller, the gpio component, the PL011 and the root dispatcher"
    );
    assert_eq!(seen, 5);
    assert_eq!(
        audio_channel, dma_channel,
        "the io-channel the audio driver shares with the DMA controller"
    );

    // ── The audio driver reports once the clock is set and checked ────────
    let bits = NotificationKey::from_key(report)
        .await_bits(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("report Wait failed: {:?}", error.code()));
    assert_eq!(bits, 1);

    semi::println!(
        "audio-test: ✅ the PWM clock runs at 100 MHz through clock_v1, DMA chains run through dma_v1"
    );
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            // On the board the components stay up: this Thread waits for good
            // instead of stopping the CPU.
            loop {
                let _woken = NotificationKey::from_key(report)
                    .await_bits(NotificationKey::WAIT_INFINITE);
            }
        }
    }
}

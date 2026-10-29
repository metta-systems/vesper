#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! midi-test: Wood's music path alone, loaded by Kickstart: the MIDI
//! synthesiser component with its SoundFont, a player program with one song
//! (Doom's `D_E1M1`), audio-test's drivers (the gpio component, the clock
//! manager, the DMA controller and the PWM audio driver), the mailbox
//! component, the PL011 driver and the log server. No Wood, no sound effects,
//! no compositor, no USB: it exists to debug the music on the board.
//!
//! The player sends the song to the synthesiser, looping, at Wood's volume,
//! then pulls each period's frames with `service` and streams them to the
//! audio driver through `audio_v1`, as Wood's playback Thread does. The log
//! server holds the PL011's serial channel: the player's lines (the song's
//! timing and first notes, the clocks, a summary every 256 periods with the
//! `service` times, level, voices, underruns and the song position), the
//! synthesiser's and the audio driver's reach the UART through it, and the
//! semihosting console as well under QEMU.
//!
//! The PWM node is disabled in the stock device trees: `just test-midi` boots
//! the test tree `targets/overlays/audio-test-pwm.dts` makes, where the
//! driver captures the stream by DMA copies; the board needs
//! `dtoverlay=pwm-2chan,pin=40,func=4,pin2=41,func2=4` in `config.txt`.
//!
//! The boot Thread checks what Kickstart hands the player, the synthesiser,
//! the audio driver and the log server, installs the report Notification key
//! at `REPORT_SLOT` of the player's table and waits. Under QEMU the player
//! plays a bounded number of periods, closes and signals it, and the boot
//! Thread ends the run successfully; on the board the player plays for good.
//! A failed check panics and fails the run instead.

// The bundled SoundFont is a 4 MiB resource.
#[allow(clippy::large_include_file)]
mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    audio::audio_v1::{FUNCTION_COUNT as AUDIO_V1_FUNCTIONS, TYPE_CODE as AUDIO_V1},
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
    midi_synth_server::midi_synth_v1::{
        FUNCTION_COUNT as MIDI_SYNTH_V1_FUNCTIONS, TYPE_CODE as MIDI_SYNTH_V1,
    },
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

/// Enabled nodes of the tree compatible with one of `compatible`.
fn enabled_nodes(compatible: &[&str]) -> usize {
    let tree = kickstart::device_tree::at_el1().index();
    tree.nodes()
        .filter(|node| is_enabled(node))
        .filter(|node| compatible.iter().any(|name| is_compatible(node, name)))
        .count()
}

pub fn run() -> ! {
    semi::println!("midi-test: enabled MMU and dropped to EL1");
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

    // The music needs the audio driver: the stock trees leave the PWM node
    // disabled.
    assert_eq!(
        enabled_nodes(components::AUDIO_DRIVER.compatible),
        1,
        "no enabled PWM node: boot with the test tree (just test-midi) or the pwm-2chan overlay"
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
    let started = load_drivers(
        nucleus,
        &builder,
        keys,
        &[
            &components::GPIO_DRIVER,
            &components::CPRMAN_DRIVER,
            &components::DMA_DRIVER,
            &components::AUDIO_DRIVER,
            &components::MAILBOX_DRIVER,
            &components::UART_DRIVER,
            &components::MIDI_SYNTH_DRIVER,
            &components::PLAYER_DRIVER,
            &components::LOG_SERVER_DRIVER,
            &components::DISPATCHER_DRIVER,
        ],
        &mut slots,
        |loaded: &LoadedDriver<'_>| {
            let init = loaded.init;
            let name = loaded.driver.component.name;
            let imports = init.imports();
            if name == components::PLAYER.name {
                // The player: the four functions of audio_v1, the six of
                // midi_synth_v1, `flush` of log_v1, each set on its
                // io-channel, `get_clock_rate` of mailbox_v1 (a data area, no
                // channel), and the song; the report key goes in its table
                // only.
                assert!(init.pages().is_empty());
                assert_eq!(
                    imports.len(),
                    AUDIO_V1_FUNCTIONS + MIDI_SYNTH_V1_FUNCTIONS + 1 + 1,
                    "audio_v1, midi_synth_v1, flush, get_clock_rate"
                );
                let of = |type_code| imports.iter().filter(move |record| record.type_code == type_code);
                assert!(
                    of(AUDIO_V1)
                        .map(|record| record.function)
                        .eq(0..AUDIO_V1_FUNCTIONS as u64),
                    "every function of audio_v1"
                );
                assert!(
                    of(MIDI_SYNTH_V1)
                        .map(|record| record.function)
                        .eq(0..MIDI_SYNTH_V1_FUNCTIONS as u64),
                    "every function of midi_synth_v1"
                );
                assert_eq!(
                    imports
                        .iter()
                        .filter(|record| record.channel_event == 0)
                        .count(),
                    1,
                    "every import but the mailbox's has an io-channel"
                );
                assert!(init.resource(0).is_some(), "the song resource");
                assert_eq!(init.serial.transmit_submitted, 0, "no serial channel");
                // SAFETY: the player's retained table; no invocation overlaps.
                unsafe { &mut *KeyTable::at_carve(loaded.component.table_addr) }
                    .insert(
                        KeySlot(REPORT_SLOT),
                        KeyEntry::new_carved(report_id, Rights(Rights::SEND), 1),
                        loaded.component.guard,
                    )
                    .unwrap_or_else(|failure| {
                        panic!("report grant failed: {:?}", failure.error.code())
                    });
            } else if name == components::MIDI_SYNTH.name {
                // The MIDI synthesiser: `flush` of log_v1 with its io-channel,
                // its SoundFont, and the player as its one client, on an
                // io-channel.
                assert_eq!(imports.len(), 1, "flush of log_v1");
                assert_ne!(imports[0].channel_event, 0, "log_v1 has an io-channel");
                assert!(init.resource(0).is_some(), "the SoundFont resource");
                let [area] = init.areas() else {
                    panic!(
                        "the player is the one client of midi_synth_v1, found {}",
                        init.areas().len()
                    );
                };
                assert_ne!(area.channel_event, 0, "midi_synth_v1 has an io-channel");
            } else if name == components::AUDIO.name {
                // The audio driver: the player is its one client, on an
                // io-channel; its trace goes to the log server.
                assert_eq!(init.serial.transmit_submitted, 0, "no serial channel");
                let [area] = init.areas() else {
                    panic!(
                        "the player is the one client of audio_v1, found {}",
                        init.areas().len()
                    );
                };
                assert_ne!(area.channel_event, 0, "audio_v1 has an io-channel");
            } else if name == components::LOG_SERVER.name {
                // The log server: the serial channel, and the player, the
                // synthesiser and the audio driver as its clients, each on an
                // io-channel (of the eight it serves).
                assert_ne!(init.serial.transmit_submitted, 0, "a serial channel");
                assert!(imports.is_empty());
                assert_eq!(
                    init.areas().len(),
                    3,
                    "the player, the synthesiser and the audio driver"
                );
                assert!(init.areas().iter().all(|area| area.channel_event != 0));
            }
            assert_ne!(
                init.return_key, 0,
                "{name}: exports answer with a Return key"
            );
            seen += 1;
            semi::println!(
                "midi-test: {name} on {}: {} pages, {} interrupts, {} imports",
                loaded.node,
                init.pages().len(),
                init.interrupts().len(),
                imports.len()
            );
        },
    );
    assert_eq!(
        started, 10,
        "gpio, the clock manager, the DMA controller, the audio driver, the mailbox, the PL011, the MIDI synthesiser, the player, the log server and the root dispatcher"
    );
    assert_eq!(seen, 9);

    // ── The player reports once it played and closed (under QEMU) ─────────
    let bits = NotificationKey::from_key(report)
        .await_bits(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("report Wait failed: {:?}", error.code()));
    assert_eq!(bits, 1);

    semi::println!("midi-test: ✅ the player played its song through midi_synth_v1 and audio_v1");
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

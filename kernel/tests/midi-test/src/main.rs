#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! midi-test: Wood's music path alone, loaded by Kickstart: the MIDI
//! synthesiser component with its SoundFont, a player program with one song
//! (Bach's Prelude in C, the synthesiser's test song), audio-test's drivers (the gpio component, the clock
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
//! The PWM node is disabled in the stock device trees: `just _test-midi` boots
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

// The bundled SoundFont is a 2 MiB resource.
#[allow(clippy::large_include_file)]
mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    audio::audio_v1::{FUNCTION_COUNT as AUDIO_V1_FUNCTIONS, TYPE_CODE as AUDIO_V1},
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
    midi_synth_server::midi_synth_v1::{
        FUNCTION_COUNT as MIDI_SYNTH_V1_FUNCTIONS, TYPE_CODE as MIDI_SYNTH_V1,
    },
    nucleus::{api::key_entry::KeyEntry, objects::KeyTable},
};

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

pub fn run() -> ! {
    semi::println!("midi-test: enabled MMU and dropped to EL1");
    let mut boot = BootObjects::bootstrap();
    let mut image = boot.builder_address_space(BUILDER_ARCHIVE_GUARD);

    // The music needs the audio driver: the stock trees leave the PWM node
    // disabled.
    assert_eq!(
        enabled_nodes(components::AUDIO_DRIVER.compatible),
        1,
        "no enabled PWM node: boot with the test tree (just _test-midi) or the pwm-2chan overlay"
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
            let of = |type_code| {
                imports
                    .iter()
                    .filter(move |record| record.type_code == type_code)
            };
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
    });
    // The root dispatcher is bound, not shown to the callback.
    assert_eq!(seen, started - 1);

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

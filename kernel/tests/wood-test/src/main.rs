#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! wood-test: compositor-test's components (the DWC2 host controller component,
//! the PL011 driver, the mailbox and fb components, the USB core, the USB HID
//! component, the compositor and the shell's lava-lamp window) plus Wood, the
//! Room Doom engine, and audio-test's drivers (the gpio component, the clock
//! manager, the DMA controller and the PWM audio driver), loaded by Kickstart.
//!
//! Wood asks the compositor for a 640x400 window, reads the shareware IWAD
//! from its resource and runs the engine, which plays its demos until keys
//! arrive: the window is the newest, so it has the keyboard focus, and the
//! compositor sends it the keyboard's keys on its event channel. Its log and a
//! line per power-of-two frame go to the semihosting console under QEMU. Its
//! playback Thread streams the mixed sound effects to the audio driver through
//! `audio_v1`, when the PWM node is enabled: `just test-wood` boots the test
//! tree `targets/overlays/audio-test-pwm.dts` makes, where the driver captures
//! the stream by DMA copies; the board needs
//! `dtoverlay=pwm-2chan,pin=40,func=4,pin2=41,func2=4` in `config.txt`.
//! Without the node Wood runs silent.
//!
//! As in compositor-test, the boot Thread installs the report Notification key
//! at `REPORT_SLOT` of the compositor's table and the compositor signals it once
//! it has read the keyboard. Then the boot Thread waits for good: the image
//! keeps running until `just test-wood` saves a screendump and stops QEMU. A
//! failed check panics in a component and ends the run under QEMU.

// The bundled IWAD is a 4 MiB resource.
#[allow(clippy::large_include_file)]
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
    semi::println!("wood-test: enabled MMU and dropped to EL1");
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
    // The PWM node is enabled by the test tree (`just test-wood`) or the
    // board's `pwm-2chan` overlay; without it no audio driver loads and Wood
    // runs silent.
    let audio_nodes = tree
        .nodes()
        .filter(|node| is_enabled(node))
        .filter(|node| {
            components::AUDIO_DRIVER
                .compatible
                .iter()
                .any(|compatible| is_compatible(node, compatible))
        })
        .count();
    semi::println!("wood-test: {audio_nodes} enabled PWM node for the audio driver");

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
            &components::WOOD_DRIVER,
            &components::GPIO_DRIVER,
            &components::CPRMAN_DRIVER,
            &components::DMA_DRIVER,
            &components::AUDIO_DRIVER,
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
            } else if name == components::WOOD.name {
                // Wood: six functions of compositor_v1 and its event channel,
                // the four of audio_v1 and their io-channel when the audio
                // driver loaded, a window to map its surface in, and the IWAD.
                assert_eq!(
                    imports.len(),
                    6 + 4 * audio_nodes,
                    "six functions of compositor_v1, four of audio_v1"
                );
                assert!(imports.iter().all(|record| record.channel_event != 0));
                assert_ne!(init.map_window.l2, 0, "a window to map the surface");
                assert!(init.resource(0).is_some(), "the IWAD resource");
            } else if name == components::AUDIO.name {
                // The audio driver: Wood is its one client, on an io-channel.
                let [area] = init.areas() else {
                    panic!(
                        "Wood is the one client of audio_v1, found {}",
                        init.areas().len()
                    );
                };
                assert_ne!(area.channel_event, 0, "audio_v1 has an io-channel");
            }
            assert_ne!(
                init.return_key, 0,
                "{name}: exports answer with a Return key"
            );
            seen += 1;
            semi::println!(
                "wood-test: {name} on {}: {} pages, {} interrupts, {} imports",
                loaded.node,
                init.pages().len(),
                init.interrupts().len(),
                imports.len()
            );
        },
    );
    assert_eq!(
        started,
        INSTANCES + 12 + audio_nodes,
        "the controller, the PL011, the mailbox, fb, usb-core, usb-hid, the compositor, draw-demo, wood, gpio, the clock manager, the DMA controller, the audio driver (with a PWM node) and the root dispatcher"
    );
    assert_eq!(seen, INSTANCES + 11 + audio_nodes);

    // ── The compositor reports once it has read the keyboard through input_v1 ──
    let bits = NotificationKey::from_key(report)
        .await_bits(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("report Wait failed: {:?}", error.code()));
    assert_eq!(bits, 1);

    semi::println!("wood-test: ✅ the compositor owns the input stream");
    // The engine keeps playing and the compositor keeps drawing: this Thread
    // waits for good instead of ending the run.
    loop {
        let _woken = NotificationKey::from_key(report).await_bits(NotificationKey::WAIT_INFINITE);
    }
}

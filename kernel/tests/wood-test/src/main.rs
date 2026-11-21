#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! wood-test: the platform (kernel/kickstart/platform.toml: the PL011, the
//! mailbox, power and fb components, the SD controller, the gpio component,
//! the clock manager, the DMA controller, the DWC2 or the PCIe root complex and
//! xHCI controller, the USB core, the USB HID component and the root
//! dispatcher), the compositor with the shell's lava-lamp window, Wood, the
//! Room Doom engine, the MIDI synthesiser, the PWM audio driver and the log
//! server, loaded by Kickstart where the board's device tree has their nodes.
//! One image boots on the Raspberry Pi 3, 4 and 5 and under QEMU.
//!
//! Wood asks the compositor for a 640x400 window, reads the shareware IWAD
//! from its resource and runs the engine, which plays its demos until keys
//! arrive: the window is the newest, so it has the keyboard focus, and the
//! compositor sends it the keyboard's keys on its event channel. The log
//! server holds the PL011's serial channel: Wood's log (with a line per
//! power-of-two frame and a music summary every 256 periods), the
//! compositor's, the synthesiser's, the audio driver's and the USB, SD and
//! PCIe components' trace reach the UART through it, and the semihosting
//! console as well under QEMU. Its playback Thread streams the mixed sound
//! effects to the audio driver through `audio_v1`, when the PWM node is
//! enabled: `just _test-wood` boots the test tree
//! `targets/overlays/audio-test-pwm.dts` makes, where the driver captures the
//! stream by DMA copies; the Raspberry Pi 3 needs
//! `dtoverlay=pwm-2chan,pin=40,func=4,pin2=41,func2=4` in `config.txt`.
//! Without the node Wood runs silent.
//!
//! As in compositor-test, the boot Thread installs the report Notification key
//! at `REPORT_SLOT` of the compositor's table and the compositor signals it once
//! it has read the keyboard. Then the boot Thread waits for good: the image
//! keeps running until `just _test-wood` saves a screendump and stops QEMU. A
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

#[cfg_attr(
    not(feature = "qemu"),
    expect(
        unused_variables,
        reason = "the boot report prints over semihosting only"
    )
)]
pub fn run() -> ! {
    semi::println!("wood-test: enabled MMU and dropped to EL1");
    let mut boot = BootObjects::bootstrap();
    let mut image = boot.builder_address_space(BUILDER_ARCHIVE_GUARD);

    // The PWM node is enabled by the test tree (`just _test-wood`) or the
    // board's `pwm-2chan` overlay; without it no audio driver loads and Wood
    // runs silent.
    let audio_nodes = enabled_nodes(components::AUDIO_DRIVER.compatible);
    semi::println!("wood-test: {audio_nodes} enabled PWM node for the audio driver");

    let report = image.notification();
    let report_id = image
        .builder
        .entry(report)
        .carved_id()
        .unwrap_or_else(|error| panic!("report identity: {:?}", error.code()));

    // ── Driver loading: what the device tree asks for ─────────────────────
    let started = image.start_drivers(components::DRIVERS, |loaded, _builder| {
        let name = loaded.driver.component.name;
        if name == components::COMPOSITOR.name {
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
        semi::println!(
            "wood-test: {name} on {}: {} pages, {} interrupts, {} imports",
            loaded.node,
            loaded.init.pages().len(),
            loaded.init.interrupts().len(),
            loaded.init.imports().len()
        );
    });
    semi::println!("wood-test: {started} components started");

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

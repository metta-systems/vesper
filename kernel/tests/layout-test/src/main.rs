#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! layout-test: `display_layout_v1`, the compositor's layout.
//!
//! Kickstart loads the mailbox and fb components, the compositor with the
//! framebuffer as its one output, and the layout client twice: `arranger`,
//! whose manifest entry says `layout = true`, and `reader`. Both get the keys
//! of `display_layout_v1` from the compositor; the reader only reads the
//! layout, and the arranger turns and moves the output and reads it back.
//! This Thread installs a report key in each client's table, the arranger's
//! signalling bit 0 and the reader's bit 1, and ends the run once both have.

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

/// The bits each client's report signals.
const ARRANGER_BIT: u64 = 1;
const READER_BIT: u64 = 2;

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
    semi::println!("layout-test: enabled MMU and dropped to EL1");
    let mut boot = BootObjects::bootstrap();
    let mut image = boot.builder_address_space(BUILDER_ARCHIVE_GUARD);

    let report = image.notification();
    let report_id = image
        .builder
        .entry(report)
        .carved_id()
        .unwrap_or_else(|error| panic!("report identity: {:?}", error.code()));

    let started = image.start_drivers(components::DRIVERS, |loaded, _builder| {
        let name = loaded.driver.component.name;
        let bit = if name == components::ARRANGER.name {
            assert!(loaded.driver.layout, "the arranger may arrange");
            ARRANGER_BIT
        } else if name == components::READER.name {
            assert!(!loaded.driver.layout, "the reader may not");
            READER_BIT
        } else {
            return;
        };
        // pixel_order and display_layout of compositor_v1.
        assert_eq!(loaded.init.imports().len(), 2, "{name}: two functions");
        // SAFETY: the program's retained table; no invocation overlaps.
        unsafe { &mut *KeyTable::at_carve(loaded.component.table_addr) }
            .insert(
                KeySlot(REPORT_SLOT),
                KeyEntry::new_carved(report_id, Rights(Rights::SEND), bit),
                loaded.component.guard,
            )
            .unwrap_or_else(|failure| panic!("report grant failed: {:?}", failure.error.code()));
    });
    semi::println!("layout-test: {started} components started");

    let mut reported = 0;
    while reported != ARRANGER_BIT | READER_BIT {
        reported |= NotificationKey::from_key(report)
            .await_bits(NotificationKey::WAIT_INFINITE)
            .unwrap_or_else(|error| panic!("report Wait failed: {:?}", error.code()));
    }
    semi::println!(
        "layout-test: ✅ any client reads the layout, and only the one with layout = true turns and moves outputs"
    );
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            // Give the CPU back (`endless_sleep` would starve every other
            // Thread): wait for good.
            loop {
                let _woken = NotificationKey::from_key(report)
                    .await_bits(NotificationKey::WAIT_INFINITE);
            }
        }
    }
}

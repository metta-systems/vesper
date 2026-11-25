#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! every-test: an import `via every`.
//!
//! Kickstart loads three programs exporting `echo_v1` (`echo_a`, `echo_b` and
//! `echo_c`, one binary) and a client that imports it `via every max 2`. Each
//! function of the import is linked to every exporter in load order, at most
//! two: the client gets two records of `count`, one for `echo_a` and one for
//! `echo_b`, each with a data area of its own, and `echo_c` serves no one.
//! This Thread checks the records; the client calls both instances and
//! signals the report key this Thread installs in its table.

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

#[cfg_attr(
    not(feature = "qemu"),
    expect(
        unused_variables,
        reason = "the boot report prints over semihosting only"
    )
)]
pub fn run() -> ! {
    semi::println!("every-test: enabled MMU and dropped to EL1");
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
        let init = loaded.init;
        if name == components::CLIENT.name {
            // One record of `count` per linked exporter, each pair with an
            // area of its own.
            let [first, second] = init.imports() else {
                panic!(
                    "two records of count (echo_a and echo_b), found {}",
                    init.imports().len()
                );
            };
            assert_eq!(first.type_code, second.type_code, "one interface");
            assert_eq!((first.function, second.function), (0, 0), "count, twice");
            assert_ne!(first.invocation, second.invocation, "two exporters");
            assert_ne!(first.area_base, second.area_base, "an area for each pair");
            assert_eq!((first.area_len, second.area_len), (4096, 4096));
            // SAFETY: the program's retained table; no invocation overlaps.
            unsafe { &mut *KeyTable::at_carve(loaded.component.table_addr) }
                .insert(
                    KeySlot(REPORT_SLOT),
                    KeyEntry::new_carved(report_id, Rights(Rights::SEND), 1),
                    loaded.component.guard,
                )
                .unwrap_or_else(|failure| {
                    panic!("report grant failed: {:?}", failure.error.code())
                });
        } else if name == components::ECHO_A.name || name == components::ECHO_B.name {
            assert_eq!(init.areas().len(), 1, "{name} serves the client");
        } else if name == components::ECHO_C.name {
            assert!(init.areas().is_empty(), "echo_c is past the client's max");
        }
        semi::println!(
            "every-test: {name}: {} imports, {} areas",
            init.imports().len(),
            init.areas().len()
        );
    });
    semi::println!("every-test: {started} components started");

    let bits = NotificationKey::from_key(report)
        .await_bits(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("report Wait failed: {:?}", error.code()));
    assert_eq!(bits, 1);
    semi::println!("every-test: ✅ the client imports echo_v1 from two of three exporters");
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

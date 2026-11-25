#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! display-test: the fb component as a scanout output of `display_v1`.
//!
//! Kickstart loads the mailbox and fb components for their nodes and a probe
//! program that imports `display_v1` from the fb component, found by the
//! framebuffer node's compatible string. At start the fb component reads the
//! display's EDID through the mailbox and configures its preferred mode; under
//! QEMU, which gives no EDID, that is the one fallback mode. The probe checks
//! the description, the modes and the refusals, then signals the report key
//! this Thread installs in its table.

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
    semi::println!("display-test: enabled MMU and dropped to EL1");
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
        if name == components::FB.name {
            // Its probe is its one display_v1 client: one data area, the
            // flush screen's, which a scanout output leaves unused.
            assert_eq!(init.areas().len(), 1, "the probe's display_v1 area");
        } else if name == components::PROBE.name {
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
        }
        semi::println!(
            "display-test: {name} on {}: {} pages, {} imports",
            loaded.node,
            init.pages().len(),
            init.imports().len()
        );
    });
    semi::println!("display-test: {started} components started");

    let bits = NotificationKey::from_key(report)
        .await_bits(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("report Wait failed: {:?}", error.code()));
    assert_eq!(bits, 1);
    semi::println!("display-test: ✅ the fb component serves display_v1 as a scanout output");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            // On a board the components go on: the log server still has the
            // probe's last lines to write. This Thread must give the CPU back
            // rather than spin in `endless_sleep` (a `wfe` loop at EL1 that
            // starves every other Thread), so it waits for good.
            loop {
                let _woken = NotificationKey::from_key(report)
                    .await_bits(NotificationKey::WAIT_INFINITE);
            }
        }
    }
}

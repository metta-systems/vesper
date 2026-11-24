#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! panel-test: the SPI panel on a board, and nothing else. Kickstart loads the
//! PL011 driver, the log server (holding its serial channel), the mailbox, the
//! gpio component, the spi component for SPI0, the ILI9341 panel driver for
//! the panel node on it and the panel probe. The panel driver initializes the
//! panel, clears it and turns its backlight on, then serves `display_v1`; the
//! probe checks the panel's description and draws on it through that
//! interface. Their lines (with the spi component's) reach the UART through
//! the log server.
//!
//! Kickstart prints on semihosting only once it drops to EL1, so this Thread
//! then only waits.

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    kickstart::{
        image::{BUILDER_ARCHIVE_GUARD, BootObjects},
        kickstart_init_el2,
    },
    // `libkicktest` supplies the panic handler.
    libkicktest as _,
    libobject::NotificationKey,
    libqemu::semihosting as semi,
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
    semi::println!("panel-test: enabled MMU and dropped to EL1");
    let mut boot = BootObjects::bootstrap();
    let mut image = boot.builder_address_space(BUILDER_ARCHIVE_GUARD);
    let started = image.start_drivers(components::DRIVERS, |loaded, _builder| {
        semi::println!(
            "panel-test: {} on {}: {} pages, {} imports",
            loaded.driver.component.name,
            loaded.node,
            loaded.init.pages().len(),
            loaded.init.imports().len()
        );
    });
    semi::println!("panel-test: {started} components started");

    // The drivers log on their own; wait for good.
    let park = NotificationKey::from_key(image.notification());
    loop {
        let _woken = park.await_bits(NotificationKey::WAIT_INFINITE);
    }
}

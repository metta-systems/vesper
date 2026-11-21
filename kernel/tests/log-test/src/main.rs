#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! log-test: a component's last words reach the UART through the log server.
//!
//! Kickstart loads the PL011 driver, the log server (holding the PL011's
//! serial channel), `log_probe` and the root dispatcher. The probe links
//! `liblog_client`, so its image exports the fault handler and Kickstart
//! installs it at its table's `FAULT_HANDLER` slot (checked here before the
//! probe starts). The probe's second Thread overflows its stack; its first
//! Thread then panics. `just _test-log` checks that the UART file holds the
//! `FAULT:` and `PANICKED:` lines. Under QEMU the panic ends the run; this
//! Thread only waits.

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    kickstart::{image::BootObjects, kickstart_init_el2},
    libdevice_driver::FAULT_HANDLER_EXPORT,
    // `libkicktest` supplies the panic handler.
    libkicktest as _,
    libobject::NotificationKey,
    libqemu::semihosting as semi,
    nucleus::objects::KeyTable,
};

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

/// Guard of the `KeyTable` archiving the builder's image Frames.
const BUILDER_ARCHIVE_GUARD: u32 = 0x0A_4C02;

pub fn run() -> ! {
    semi::println!("log-test: enabled MMU and dropped to EL1");
    let mut boot = BootObjects::bootstrap();
    let mut image = boot.builder_address_space(BUILDER_ARCHIVE_GUARD);

    let mut seen = 0;
    let started = image.start_drivers(components::DRIVERS, |loaded, _builder| {
        let init = loaded.init;
        let name = loaded.driver.component.name;
        assert_eq!(init.name(), name, "the init record names the component");
        assert_ne!(init.thread_stacks[0].top, 0, "the first Thread's stack");
        // SAFETY: the component's table was carved at `table_addr` and
        // nothing else uses it before its Threads start.
        let table = unsafe { &*KeyTable::at_carve(loaded.component.table_addr) };
        let exports_handler = loaded
            .driver
            .component
            .export(FAULT_HANDLER_EXPORT)
            .is_some();
        assert_eq!(
            table.fault_handler().is_some(),
            exports_handler,
            "{name}: a fault handler exactly when the image exports one"
        );
        if name == components::LOG_PROBE.name {
            assert!(exports_handler, "liblog_client exports the fault handler");
            assert_ne!(
                init.thread_stacks[1].top, 0,
                "the overflowing Thread's stack"
            );
            assert_eq!(init.imports().len(), 1, "flush of log_v1");
        } else if name == components::LOG_SERVER.name {
            assert_ne!(init.serial.transmit_submitted, 0, "a serial channel");
            assert_eq!(init.areas().len(), 1, "the probe is its client");
        }
        seen += 1;
        semi::println!("log-test: {name} loaded, fault handler {exports_handler}");
    });
    // The root dispatcher is bound, not shown to the callback.
    assert_eq!(seen, started - 1);
    semi::println!("log-test: ✅ the probe has its fault handler");

    // The probe's panic ends the run under QEMU; until then, wait.
    let park = image.notification();
    loop {
        let _woken = NotificationKey::from_key(park).await_bits(NotificationKey::WAIT_INFINITE);
    }
}

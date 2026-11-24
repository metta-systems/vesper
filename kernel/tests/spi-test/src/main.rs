#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! spi-test: the spi component on SPI0 and a probe driver for the panel node
//! on it (`targets/spi-panel-test.dtb`, the Raspberry Pi 4 tree with the
//! MSP2202 panel on SPI0 CE0), loaded by Kickstart.
//!
//! Kickstart loads the probe for `panel@0`, a node on the SPI bus with no
//! registers of its own, and links its `spi_v1` through its parent node (`via
//! device parent`) to the spi component. Both read their nodes from their
//! device tree copies: the spi component its chip selects (`cs-gpios`), the
//! probe its chip select and clock. The probe calls the spi component and
//! signals the report key this Thread installs in its table; QEMU has no SPI0
//! block, so no transaction reaches the wire.

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
    semi::println!("spi-test: enabled MMU and dropped to EL1");
    let mut boot = BootObjects::bootstrap();
    let mut image = boot.builder_address_space(BUILDER_ARCHIVE_GUARD);
    assert_eq!(
        enabled_nodes(components::SPI_DRIVER.compatible),
        1,
        "SPI0 is enabled: boot with targets/spi-panel-test.dtb (just _test-spi)"
    );
    assert_eq!(
        enabled_nodes(components::PROBE_DRIVER.compatible),
        1,
        "the panel node on SPI0"
    );

    let report = image.notification();
    let report_id = image
        .builder
        .entry(report)
        .carved_id()
        .unwrap_or_else(|error| panic!("report identity: {:?}", error.code()));

    let started = image.start_drivers(components::DRIVERS, |loaded, _builder| {
        let name = loaded.driver.component.name;
        let init = loaded.init;
        if name == components::SPI.name {
            // The bus: the block's registers, the gpio and mailbox imports,
            // the probe as its one client, and a device tree copy.
            assert!(!init.pages().is_empty(), "the SPI0 registers");
            assert_eq!(init.areas().len(), 1, "the probe is the bus's one client");
            assert!(init.device_tree().is_some(), "a device tree copy");
            assert_eq!(init.node_path(), Some("/soc/spi@7e204000"));
        } else if name == components::PROBE.name {
            // The panel: no registers (its `reg` is a chip select), `write`
            // of spi_v1 through its parent, and a device tree copy.
            assert!(
                init.pages().is_empty(),
                "a node on the bus has no registers"
            );
            assert_eq!(init.imports().len(), 1, "write of spi_v1");
            assert!(init.device_tree().is_some(), "a device tree copy");
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
            "spi-test: {name} on {}: {} pages, {} imports",
            loaded.node,
            init.pages().len(),
            init.imports().len()
        );
    });
    semi::println!("spi-test: {started} components started");

    let bits = NotificationKey::from_key(report)
        .await_bits(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("report Wait failed: {:?}", error.code()));
    assert_eq!(bits, 1);
    semi::println!("spi-test: ✅ the panel's probe called the spi component through its parent");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            libcpu::endless_sleep()
        }
    }
}

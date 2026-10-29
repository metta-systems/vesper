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
//! Thread then panics. `just test-log` checks that the UART file holds the
//! `FAULT:` and `PANICKED:` lines. Under QEMU the panic ends the run; this
//! Thread only waits.

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
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
    libdevice_driver::FAULT_HANDLER_EXPORT,
    // `libkicktest` supplies the panic handler.
    libkicktest as _,
    libobject::{
        KeySlot, KeyTableKey, NotificationKey, ObjectType, Rights, UntypedKey,
        address_space::AddressSpaceKey,
    },
    libqemu::semihosting as semi,
    nucleus::objects::KeyTable,
};

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

/// First boot `KeyTable` slot the builder carves fixtures into.
const FIRST_FIXTURE_SLOT: u32 = 64;

/// Guard of the `KeyTable` archiving the builder's image Frames.
const BUILDER_ARCHIVE_GUARD: u32 = 0x0A_4C02;

pub fn run() -> ! {
    semi::println!("log-test: enabled MMU and dropped to EL1");
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

    // The builder's own address space, as in the other driver test kernels.
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

    let mut seen = 0;
    let started = load_drivers(
        nucleus,
        &builder,
        keys,
        &[
            &components::UART_DRIVER,
            &components::LOG_SERVER_DRIVER,
            &components::LOG_PROBE_DRIVER,
            &components::DISPATCHER_DRIVER,
        ],
        &mut slots,
        |loaded: &LoadedDriver<'_>| {
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
        },
    );
    assert_eq!(
        started, 4,
        "the PL011, the log server, the probe and the root dispatcher"
    );
    assert_eq!(seen, 3);
    semi::println!("log-test: ✅ the probe has its fault handler");

    // The probe's panic ends the run under QEMU; until then, wait.
    let park = untyped
        .retype(
            ObjectType::NOTIFICATION,
            0,
            0,
            1,
            &self_table,
            slots.take(1),
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("park Notification Retype failed: {:?}", error.code()));
    loop {
        let _woken = NotificationKey::from_key(park).await_bits(NotificationKey::WAIT_INFINITE);
    }
}

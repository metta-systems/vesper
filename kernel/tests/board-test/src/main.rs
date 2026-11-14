#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! board-test: the drivers being brought up on new boards, with their lines on
//! the UART.
//!
//! Kickstart loads the PL011 driver, the log server (holding the PL011's
//! serial channel), the mailbox, the SD card controller and the root
//! dispatcher. The controller logs its bring-up (base clock, ADMA2 or the data
//! port, the card's size) and what block 0 holds through the log server, so a
//! board shows them on its UART: the Raspberry Pi 4's GPIO 14 and 15, the
//! Raspberry Pi 5's debug connector. Nothing writes to the card. Kickstart
//! prints on semihosting only once it drops to EL1, so this Thread then only
//! waits.

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
    // `libkicktest` supplies the panic handler.
    libkicktest as _,
    libobject::{
        KeySlot, KeyTableKey, NotificationKey, ObjectType, Rights, UntypedKey,
        address_space::AddressSpaceKey,
    },
    libqemu::semihosting as semi,
};

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

/// First boot `KeyTable` slot the builder carves fixtures into.
const FIRST_FIXTURE_SLOT: u32 = 64;

/// Guard of the `KeyTable` archiving the builder's image Frames.
const BUILDER_ARCHIVE_GUARD: u32 = 0x0A_4C03;

#[cfg_attr(
    not(feature = "qemu"),
    expect(
        unused_variables,
        reason = "the boot report prints over semihosting only"
    )
)]
pub fn run() -> ! {
    semi::println!("board-test: enabled MMU and dropped to EL1");
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

    let started = load_drivers(
        nucleus,
        &builder,
        keys,
        &[
            &components::UART_DRIVER,
            &components::LOG_SERVER_DRIVER,
            &components::MAILBOX_DRIVER,
            &components::SD_DRIVER,
            &components::DISPATCHER_DRIVER,
        ],
        &mut slots,
        |loaded: &LoadedDriver<'_>| {
            semi::println!(
                "board-test: {} on {}: {} pages, {} interrupts, {} imports",
                loaded.driver.component.name,
                loaded.node,
                loaded.init.pages().len(),
                loaded.init.interrupts().len(),
                loaded.init.imports().len()
            );
        },
    );
    semi::println!("board-test: {started} components started");

    // The drivers log on their own; wait for good.
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

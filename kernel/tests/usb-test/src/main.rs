#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! usb-test: the DWC2 host controller component, loaded by Kickstart.
//!
//! The image bundles the `usb-hcd-dwc2` component. Kickstart loads one
//! instance per enabled `brcm,bcm2708-usb` node, which is the Raspberry Pi 3's
//! controller. The component resets the root port and enumerates the device
//! behind it. On raspi3b, QEMU attaches a hub there, so the root device must
//! be a hub.
//!
//! The boot Thread installs the report Notification key at `REPORT_SLOT` of
//! the instance's table, as driver-test does. The component signals it once
//! the hub is enumerated, and then the boot Thread exits the run successfully.
//! A failed check panics in the component and fails the run instead.
//!
//! In-guest assertions and the QEMU semihosting exit status are the result
//! (`just test-usb`).

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    kickstart::{
        bootstrap::{bootstrap_nucleus, device_untypeds, retained_init_memory, spawn_idle},
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
        KeySlot, KeyTableKey, NotificationKey, ObjectType, RawKey, Rights, UntypedKey,
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

/// The slot the component signals its report at. The component names the
/// same slot under the `qemu` feature.
const REPORT_SLOT: u32 = 250;

/// Instances the board device tree produces: the Raspberry Pi 3's DWC2 node.
const INSTANCES: usize = 1;

pub fn run() -> ! {
    semi::println!("usb-test: enabled MMU and dropped to EL1");
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

    let _idle = spawn_idle(
        nucleus,
        keytable_addr,
        RawKey::from_wire(untyped.to_wire()),
        boot_as_id,
    );

    // ── Driver matching ───────────────────────────────────────────────────
    let mut count = 0;
    let started = load_drivers(
        nucleus,
        &builder,
        keys,
        &[&components::USB_DRIVER],
        &mut slots,
        |loaded: &LoadedDriver<'_>| {
            semi::println!(
                "usb-test: {} on {}: {} pages, {} interrupts",
                loaded.driver.component.name,
                loaded.node,
                loaded.init.pages().len(),
                loaded.init.interrupts().len()
            );
            // The report key, badged with this instance's bit.
            // SAFETY: the driver's retained table; no invocation overlaps.
            unsafe { &mut *KeyTable::at_carve(loaded.component.table_addr) }
                .insert(
                    KeySlot(REPORT_SLOT),
                    KeyEntry::new_carved(report_id, Rights(Rights::SEND), 1 << count),
                    loaded.component.guard,
                )
                .unwrap_or_else(|failure| {
                    panic!("report grant failed: {:?}", failure.error.code())
                });
            count += 1;
        },
    );
    assert_eq!(started, INSTANCES);

    // ── The component reports once the hub is enumerated ─────────────────
    let report = NotificationKey::from_key(report);
    let all = (1_u64 << INSTANCES) - 1;
    let mut reported = 0;
    while reported != all {
        reported |= report
            .wait(NotificationKey::WAIT_INFINITE)
            .unwrap_or_else(|error| panic!("report Wait failed: {:?}", error.code()));
    }

    semi::println!("usb-test: ✅ DWC2 root hub enumerated");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            libcpu::endless_sleep()
        }
    }
}

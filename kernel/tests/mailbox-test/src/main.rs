#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! mailbox-test: the mailbox component and its clients, end to end.
//!
//! The image bundles the `mailbox` component, a driver for the
//! `brcm,bcm2835-mbox` node, and `mailbox-test-client`, a driver for the
//! `raspberrypi,bcm2835-power` node whose `imports.if` uses `mailbox_v1` via
//! `firmware, mboxes`: the power node links to the firmware node, which links
//! to the mailbox.
//!
//! Before each instance starts, the boot Thread checks its init record: on
//! raspi3b the mailbox registers share the BCM interrupt controller's
//! kernel-owned page and arrive as a minted device Frame, on raspi4b as a
//! page-sized device Untyped; the client holds exactly one `mailbox_v1`
//! import. The client calls the component and reports; the boot Thread waits
//! for the report.
//!
//! In-guest assertions and the QEMU semihosting exit status are the result
//! (`just test-mailbox`, `just test-mailbox-rpi4`).

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    driver_protocol::REPORT_SLOT,
    kickstart::{
        bootstrap::{
            PoolCapacities, bootstrap_nucleus, device_untypeds, retained_init_memory, spawn_idle,
        },
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
        KeySlot, KeyTableKey, NotificationKey, ObjectType, RawKey, Rights, UntypedKey,
        address_space::AddressSpaceKey,
    },
    libqemu::semihosting as semi,
    mailbox::mailbox_v1::TYPE_CODE as MAILBOX_V1,
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

/// Instances the board device trees produce: the mailbox and the power node.
const INSTANCES: usize = 2;

/// The mailbox's register page: in the BCM interrupt controller's page on the
/// Raspberry Pi 3, an ordinary device page on the Raspberry Pi 4.
#[cfg(board_rpi3)]
const MAILBOX_PAGE: (u64, u64) = (0x3f00_b000, libdevice_driver::PAGE_FRAME);
#[cfg(board_rpi4)]
const MAILBOX_PAGE: (u64, u64) = (0xfe00_b000, libdevice_driver::PAGE_UNTYPED);

pub fn run() -> ! {
    semi::println!("mailbox-test: enabled MMU and dropped to EL1");
    let retained = retained_init_memory();
    let image_tables = image_table_count(&retained);
    let boot = bootstrap_nucleus(&PoolCapacities {
        // builder and one per driver instance
        address_spaces: 1 + INSTANCES,
        // builder: root/L1/L2 and image L3s; each driver: root/L1/L2, image
        // span, stack region, init page, device window
        page_tables: 3 + image_tables + INSTANCES * 8,
        asid_pools: 1,
    });
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

    // ── Driver loading ────────────────────────────────────────────────────
    let mut seen_mailbox = false;
    let mut seen_client = false;
    let started = load_drivers(
        nucleus,
        &builder,
        keys,
        &[&components::MAILBOX_DRIVER, &components::CLIENT_DRIVER],
        &mut slots,
        |loaded: &LoadedDriver<'_>| {
            let init = loaded.init;
            if loaded.driver.component.name == components::MAILBOX.name {
                let page = init
                    .pages()
                    .first()
                    .expect("the mailbox has a register page");
                assert_eq!(
                    (page.paddr, page.kind),
                    MAILBOX_PAGE,
                    "the mailbox register page and how it is handed over"
                );
                assert!(init.imports().is_empty());
                seen_mailbox = true;
            } else {
                // The power node has no registers; its import reaches the
                // mailbox through the firmware node.
                assert!(init.pages().is_empty());
                let imports = init.imports();
                assert_eq!(imports.len(), 4, "the client imports four functions");
                assert!(imports.iter().all(|record| record.type_code == MAILBOX_V1));
                let functions: [u64; 4] = core::array::from_fn(|index| imports[index].function);
                assert_eq!(
                    functions,
                    [0, 1, 2, 3],
                    "declaration indices of the imported functions"
                );
                // The report key, in the client's table only.
                // SAFETY: the driver's retained table; no invocation overlaps.
                unsafe { &mut *(loaded.component.table_addr as *mut KeyTable) }
                    .insert(
                        KeySlot(REPORT_SLOT),
                        KeyEntry::new_carved(report_id, Rights(Rights::SEND), 1),
                        loaded.component.guard,
                    )
                    .unwrap_or_else(|failure| {
                        panic!("report grant failed: {:?}", failure.error.code())
                    });
                seen_client = true;
            }
            semi::println!(
                "mailbox-test: {} on {}: {} pages, {} interrupts, {} imports",
                loaded.driver.component.name,
                loaded.node,
                init.pages().len(),
                init.interrupts().len(),
                init.imports().len()
            );
        },
    );
    assert_eq!(started, INSTANCES);
    assert!(seen_mailbox && seen_client);

    // ── The client reports ────────────────────────────────────────────────
    let bits = NotificationKey::from_key(report)
        .wait(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("report Wait failed: {:?}", error.code()));
    assert_eq!(bits, 1);

    semi::println!("mailbox-test: ✅ mailbox loading, firmware link and client call passed");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            libcpu::endless_sleep()
        }
    }
}

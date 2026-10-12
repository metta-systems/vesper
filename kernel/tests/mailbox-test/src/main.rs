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
    libdevice_driver::DATA_WINDOW_VA,
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

/// Instances the board device trees produce: the mailbox, the power management
/// node, the framebuffer node and the vcsm client.
const INSTANCES: usize = 4;

/// The mailbox's register page: in the BCM interrupt controller's page on the
/// Raspberry Pi 3, an ordinary device page on the Raspberry Pi 4.
#[cfg(board_rpi3)]
const MAILBOX_PAGE: (u64, u64) = (0x3f00_b000, libdevice_driver::PAGE_FRAME);
#[cfg(board_rpi4)]
const MAILBOX_PAGE: (u64, u64) = (0xfe00_b000, libdevice_driver::PAGE_UNTYPED);

/// The `VideoCore` memory the firmware reports on the emulated boards: from the
/// top of RAM up to the peripheral window on the Raspberry Pi 3, up to the
/// first gigabyte on the Raspberry Pi 4.
#[cfg(board_rpi3)]
const VIDEO_MEMORY: (u64, u64) = (0x3c00_0000, 0x3f00_0000);
#[cfg(board_rpi4)]
const VIDEO_MEMORY: (u64, u64) = (0x3c00_0000, 0x4000_0000);

/// Four words, a place to note the mailbox's data areas in.
#[derive(Default)]
struct Vec4([u64; 4]);

pub fn run() -> ! {
    semi::println!("mailbox-test: enabled MMU and dropped to EL1");
    let retained = retained_init_memory();
    let image_tables = image_table_count(&retained);
    let boot = bootstrap_nucleus(&PoolCapacities {
        // builder and one per driver instance
        address_spaces: 1 + INSTANCES,
        // builder: root/L1/L2 and image L3s; each driver: root/L1/L2, image
        // span, stack region, init page, device window, data window
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
    // What each driver was handed, noted as its record is checked.
    let mut mailbox_areas: Vec4 = Vec4::default();
    let mut client_area = 0;
    let mut power_area = 0;
    let mut fb_area = 0;
    let mut seen = 0;
    let started = load_drivers(
        nucleus,
        &builder,
        keys,
        &[
            &components::MAILBOX_DRIVER,
            &components::POWER_DRIVER,
            &components::FB_DRIVER,
            &components::CLIENT_DRIVER,
        ],
        &mut slots,
        |loaded: &LoadedDriver<'_>| {
            let init = loaded.init;
            let name = loaded.driver.component.name;
            let imports = init.imports();
            // Every import of the mailbox interface carries the area the
            // client shares with the mailbox: one page, in the data window.
            let mailbox_imports = imports
                .iter()
                .filter(|record| record.type_code == MAILBOX_V1)
                .count();
            let area_of_mailbox_imports = imports
                .iter()
                .find(|record| record.type_code == MAILBOX_V1)
                .map_or(0, |record| record.area_base);
            for record in imports
                .iter()
                .filter(|record| record.type_code == MAILBOX_V1)
            {
                assert_eq!(
                    (record.area_base, record.area_len),
                    (area_of_mailbox_imports, 4096),
                    "{name}: the functions of one pair share one area"
                );
                assert!(record.area_base >= DATA_WINDOW_VA);
            }
            if name == components::MAILBOX.name {
                let page = init
                    .pages()
                    .first()
                    .expect("the mailbox has a register page");
                assert_eq!(
                    (page.paddr, page.kind),
                    MAILBOX_PAGE,
                    "the mailbox register page and how it is handed over"
                );
                assert!(imports.is_empty());
                // Three clients call it (the vcsm client, power and fb), each
                // with an area of one page of its own, and a stack of its own.
                let areas = init.areas();
                assert_eq!(areas.len(), 3, "three clients, three data areas");
                for (index, area) in areas.iter().enumerate() {
                    assert_eq!(area.len, 4096);
                    assert!(area.stack_bottom < area.stack_end);
                    mailbox_areas.0[index] = area.base;
                }
                mailbox_areas.0[..3].sort_unstable();
                assert_eq!(
                    mailbox_areas.0[..3],
                    [DATA_WINDOW_VA, DATA_WINDOW_VA + 4096, DATA_WINDOW_VA + 8192],
                    "areas follow each other in the window"
                );
            } else if name == components::POWER.name {
                // The power management registers, and `set_power_state` of the
                // mailbox, found through the firmware node by compatible string.
                assert!(init.pages().len() >= 2);
                let [set_power_state] = imports else {
                    panic!("power imports one function, found {}", imports.len());
                };
                assert_eq!(mailbox_imports, 1);
                assert_eq!(set_power_state.function, 4, "set_power_state");
                assert!(init.areas().is_empty());
                power_area = area_of_mailbox_imports;
            } else if name == components::FB.name {
                // No registers; `property` of the mailbox through the firmware
                // link of its own node.
                assert!(init.pages().is_empty());
                let [property] = imports else {
                    panic!("fb imports one function, found {}", imports.len());
                };
                assert_eq!(mailbox_imports, 1);
                assert_eq!(property.function, 5, "property");
                // The video memory, in aligned blocks that follow each other
                // from the top of RAM to the end of the gap.
                let video = init.video();
                assert!(!video.is_empty(), "fb holds the video memory");
                let mut cursor = VIDEO_MEMORY.0;
                for block in video {
                    assert_eq!(block.base, cursor, "blocks follow each other");
                    assert_eq!(block.base % (1 << block.size_bits), 0, "aligned");
                    assert_ne!(block.key, 0);
                    cursor = block.end();
                }
                assert_eq!(cursor, VIDEO_MEMORY.1, "up to the end of the gap");
                fb_area = area_of_mailbox_imports;
            } else {
                // The vcsm node has no registers; its imports reach the
                // mailbox through the firmware node, and the power and
                // framebuffer components by compatible string.
                assert!(init.pages().is_empty());
                let imports: &[_; 10] = imports.try_into().unwrap_or_else(|_| {
                    panic!(
                        "five mailbox, one power, four fb functions, found {}",
                        imports.len()
                    )
                });
                assert_eq!(mailbox_imports, 5);
                let functions: [u64; 5] = core::array::from_fn(|index| imports[index].function);
                assert_eq!(
                    functions,
                    [0, 1, 2, 3, 5],
                    "declaration indices of the imported mailbox functions"
                );
                assert_eq!(imports[5].function, 0, "devices_off");
                assert!(imports[5..].iter().all(|record| record.area_len == 0));
                assert!(imports[6..].iter().map(|record| record.function).eq(0..4));
                assert_ne!(imports[5].invocation, imports[6].invocation);
                client_area = area_of_mailbox_imports;
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
            }
            assert_ne!(
                init.return_key, 0,
                "{name}: exports answer with a Return key"
            );
            seen += 1;
            semi::println!(
                "mailbox-test: {name} on {}: {} pages, {} interrupts, {} imports",
                loaded.node,
                init.pages().len(),
                init.interrupts().len(),
                imports.len()
            );
        },
    );
    assert_eq!(started, INSTANCES);
    assert_eq!(seen, INSTANCES);
    // The areas the clients were handed are the ones the mailbox serves.
    let mut handed_out = [client_area, power_area, fb_area];
    handed_out.sort_unstable();
    assert_eq!(handed_out, mailbox_areas.0[..3]);

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

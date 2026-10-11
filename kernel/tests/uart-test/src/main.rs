#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! uart-test: the PL011 driver component serving a client in loopback.
//!
//! The image bundles the `uart-pl011` driver. Kickstart loads one instance,
//! for the PL011 `uart0` of raspi3b and raspi4b. Before it starts, the boot
//! Thread attaches to it as a client (see `libserial-channel`): it installs
//! a channel page asking for loopback and a Notification for the driver to
//! signal, and keeps a doorbell key to the driver's own Notification.
//!
//! The boot Thread then streams [`MESSAGE_BYTES`] bytes of text through the
//! channel's transmit ring, several times the ring and far more than the
//! UART's FIFOs, ringing the doorbell as it queues them, and reads every
//! byte back from the receive ring as the driver moves the looped-back bytes
//! there from its receive interrupts. Nothing may be dropped or reordered.
//!
//! In-guest assertions and the QEMU semihosting exit status are the result
//! (`just test-uart`, `just test-uart-rpi4`).

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    core::sync::atomic::Ordering,
    kickstart::{
        bootstrap::{
            PoolCapacities, bootstrap_nucleus, device_untypeds, retained_init_memory, spawn_idle,
        },
        drivers::{BootKeys, LoadedDriver, instance_count, load_drivers},
        kickstart_init_el2,
    },
    libaddress::PhysAddr,
    libcomponent_loader::{
        builder::{Builder, ImageArchive, ImageTarget, verify_retained_image},
        component::ttbr,
        keys::{BOOT_TABLE_GUARD, SlotCursor, boot_key},
        paging::image_table_count,
    },
    libdevice_driver::local_key,
    // `libkicktest` supplies the panic handler.
    libkicktest as _,
    libobject::{
        KeySlot, KeyTableKey, NotificationKey, ObjectType, RawKey, Rights, UntypedKey,
        address_space::AddressSpaceKey,
    },
    libqemu::semihosting as semi,
    libserial_channel::{CHANNEL_FRAME_SLOT, CLIENT_SLOT, DOORBELL_BIT, LOOPBACK, SerialChannel},
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

/// Bytes streamed through the loopback.
const MESSAGE_BYTES: usize = 3000;

/// Characters per line of the stream.
const LINE_BYTES: usize = 64;

/// Byte `index` of the stream: lines of printable ASCII in an order a
/// dropped or repeated byte shows up in. QEMU's PL011 also sends looped-back
/// bytes to its console, so the stream stays readable there.
fn message_byte(index: usize) -> u8 {
    if index % LINE_BYTES == LINE_BYTES - 1 {
        b'\n'
    } else {
        // Below 95, so the conversion never fails.
        b' ' + u8::try_from(index.wrapping_mul(31).wrapping_add(7) % 95).unwrap_or_default()
    }
}

pub fn run() -> ! {
    semi::println!("uart-test: enabled MMU and dropped to EL1");
    let drivers = [&components::UART_DRIVER];
    let instances = instance_count(&drivers);
    assert_eq!(instances, 1, "the board device tree has one enabled PL011");

    let retained = retained_init_memory();
    let image_tables = image_table_count(&retained);
    let boot = bootstrap_nucleus(&PoolCapacities {
        // builder and the driver
        address_spaces: 1 + instances,
        // builder: root/L1/L2 and image L3s; the driver: root/L1/L2, image
        // span, stack region, init page, device window
        page_tables: 3 + image_tables + instances * 8,
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

    // ── The client's side of the channel ──────────────────────────────────
    let channel_frame = builder.retype_frames(slots.take(1), 1);
    let channel_paddr = Builder::frame_paddr(channel_frame);
    // SAFETY: a fresh, zeroed Frame the boot Thread holds; the direct map
    // covers it for the whole test, and the driver maps the same page.
    let channel =
        unsafe { SerialChannel::at(PhysAddr::new(channel_paddr).user_to_kernel().as_u64()) };
    channel.config.store(LOOPBACK, Ordering::Release);
    let client = untyped
        .retype(
            ObjectType::NOTIFICATION,
            0,
            0,
            1,
            &self_table,
            slots.take(1),
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("client Notification Retype failed: {:?}", error.code()));
    let client_id = builder
        .entry(client)
        .carved_id()
        .unwrap_or_else(|error| panic!("client identity: {:?}", error.code()));
    let doorbell_slot = slots.take(1);

    let _idle = spawn_idle(
        nucleus,
        keytable_addr,
        RawKey::from_wire(untyped.to_wire()),
        boot_as_id,
    );

    // ── Load the driver and attach before it starts ───────────────────────
    let mut doorbell = None;
    let started = load_drivers(
        nucleus,
        &builder,
        keys,
        &drivers,
        &mut slots,
        |loaded: &LoadedDriver<'_>| {
            let init = loaded.init;
            assert_eq!(init.interrupts().len(), 1, "{}: one interrupt", loaded.node);
            let frame = self_table
                .copy_derive(
                    channel_frame,
                    &loaded.component.table(),
                    CHANNEL_FRAME_SLOT,
                    Rights(Rights::READ | Rights::WRITE),
                )
                .unwrap_or_else(|error| panic!("channel Frame copy failed: {:?}", error.code()));
            assert_eq!(frame, local_key(init.table, CHANNEL_FRAME_SLOT));
            // SAFETY: the driver's retained table, before its Thread runs;
            // no invocation overlaps these bootstrap grants.
            let driver_table = unsafe { &mut *(loaded.component.table_addr as *mut KeyTable) };
            let signal = driver_table
                .insert(
                    KeySlot(CLIENT_SLOT),
                    KeyEntry::new_carved(client_id, Rights(Rights::SEND), 1),
                    loaded.component.guard,
                )
                .unwrap_or_else(|failure| {
                    panic!("client key grant failed: {:?}", failure.error.code())
                });
            assert_eq!(signal, local_key(init.table, CLIENT_SLOT));
            let wakeups = driver_table
                .lookup(RawKey::from_wire(init.notification), loaded.component.guard)
                .and_then(KeyEntry::carved_id)
                .unwrap_or_else(|error| panic!("driver Notification: {:?}", error.code()));
            // SAFETY: the live boot table, borrowed for this grant only.
            doorbell = Some(
                unsafe { &mut *(keytable_addr as *mut KeyTable) }
                    .insert(
                        KeySlot(doorbell_slot),
                        KeyEntry::new_carved(wakeups, Rights(Rights::SEND), DOORBELL_BIT),
                        BOOT_TABLE_GUARD,
                    )
                    .unwrap_or_else(|failure| {
                        panic!("doorbell grant failed: {:?}", failure.error.code())
                    }),
            );
        },
    );
    assert_eq!(started, 1);
    let doorbell = NotificationKey::from_key(doorbell.expect("the driver was loaded"));
    let client = NotificationKey::from_key(client);

    // ── Stream through the loopback ───────────────────────────────────────
    let (mut queued, mut received, mut wakeups) = (0, 0, 0_u32);
    while received < MESSAGE_BYTES {
        let before = queued;
        while queued < MESSAGE_BYTES && channel.transmit.push(message_byte(queued)) {
            queued += 1;
        }
        if queued != before {
            doorbell
                .signal(0)
                .unwrap_or_else(|error| panic!("doorbell Signal failed: {:?}", error.code()));
        }
        while let Some(byte) = channel.receive.pop() {
            assert_eq!(
                byte,
                message_byte(received),
                "byte {received} came back changed"
            );
            received += 1;
        }
        if received < MESSAGE_BYTES {
            client
                .wait(NotificationKey::WAIT_INFINITE)
                .unwrap_or_else(|error| panic!("client Wait failed: {:?}", error.code()));
            wakeups += 1;
        }
    }
    assert_eq!(
        channel.dropped.load(Ordering::Acquire),
        0,
        "no byte dropped"
    );
    assert!(channel.transmit.is_empty() && channel.receive.is_empty());
    // More bytes than the rings hold: the client had to wait for the driver.
    assert!(wakeups > 0, "the client never waited for the driver");
    semi::println!("uart-test: {MESSAGE_BYTES} bytes looped back, {wakeups} client wakeups");

    semi::println!("uart-test: ✅ PL011 driver served transmit and receive in loopback");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            libcpu::endless_sleep()
        }
    }
}

#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! uart-test: the PL011 driver component serving a client in loopback.
//!
//! The image bundles the `uart-pl011` driver. Kickstart loads one instance,
//! for the PL011 `uart0` of raspi3b and raspi4b. Before its Threads start,
//! the boot Thread attaches to it as a client (see `libserial-channel`): it
//! installs a control page asking for loopback, the data area's Frames, and
//! the channels' `EventCount`s.
//!
//! The boot Thread then streams [`MESSAGE_BYTES`] bytes of text through the
//! transmit channel, in packets of two records from a few buffers it reuses
//! as they complete, far more than the UART's FIFOs hold. It keeps receive
//! buffers submitted, checks every byte the driver stores in them from its
//! receive interrupts, and resubmits each as it comes back. Nothing may be
//! dropped or reordered, and every transmit packet must come back whole.
//!
//! The boot Thread reaches the data area through the kernel's view of
//! physical memory, while every record it hands the driver names the
//! agreed `DATA_AREA_VA` addresses the driver maps.
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
        serial::SerialChannelParts,
    },
    libaddress::PhysAddr,
    libcomponent_loader::{
        builder::{Builder, ImageArchive, ImageTarget, verify_retained_image},
        component::ttbr,
        keys::{SlotCursor, boot_key},
        paging::image_table_count,
    },
    libio_channel::{Event, IoRec, NoEvent, Packet, Producer, WaitError},
    // `libkicktest` supplies the panic handler.
    libkicktest as _,
    libobject::{
        CapError, EventCountKey, KeySlot, KeyTableKey, RawKey, UntypedKey,
        address_space::AddressSpaceKey,
    },
    libqemu::semihosting as semi,
    libserial_channel::{DATA_AREA, DATA_AREA_VA, DATA_PAGES, LOOPBACK, SLOTS, SerialChannel},
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

/// Transmit buffers, each two records of [`TRANSMIT_RECORD_BYTES`] at the
/// start of the data area, [`TRANSMIT_BUFFER_STRIDE`] apart.
const TRANSMIT_BUFFERS: usize = 4;
const TRANSMIT_RECORD_BYTES: u64 = 120;
const TRANSMIT_BUFFER_STRIDE: u64 = 256;

/// Receive buffers of one record of [`RECEIVE_BUFFER_BYTES`] each, from the
/// middle of the data area.
const RECEIVE_BUFFERS: usize = SLOTS / 2;
const RECEIVE_BUFFER_BYTES: u64 = 64;
const RECEIVE_AREA_VA: u64 = DATA_AREA_VA + DATA_PAGES as u64 * 4096 / 2;

const _: () =
    assert!(TRANSMIT_BUFFERS as u64 * TRANSMIT_BUFFER_STRIDE <= RECEIVE_AREA_VA - DATA_AREA_VA);
const _: () = assert!(TRANSMIT_RECORD_BYTES * 2 <= TRANSMIT_BUFFER_STRIDE);
// Every buffer of either kind can be out at once.
const _: () = assert!(TRANSMIT_BUFFERS * 3 <= SLOTS && RECEIVE_BUFFERS * 2 <= SLOTS);

/// An empty receive buffer, tagged with its index.
fn receive_buffer(index: usize) -> Packet {
    Packet::new(
        index as u64,
        &[IoRec {
            base: RECEIVE_AREA_VA + index as u64 * RECEIVE_BUFFER_BYTES,
            len: RECEIVE_BUFFER_BYTES,
        }],
    )
    .expect("one record")
}

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

/// The client's side of the stream: which bytes went out and came back.
struct Stream<'data, Data: Fn(u64) -> *mut u8> {
    /// The boot Thread's view of a data area address
    data: &'data Data,
    /// Bytes put in transmit packets
    queued: usize,
    /// Bytes checked from receive buffers
    received: usize,
    free_transmit: [bool; TRANSMIT_BUFFERS],
    packets_sent: usize,
    buffers_filled: usize,
}

impl<Data: Fn(u64) -> *mut u8> Stream<'_, Data> {
    /// Whether a transmit packet is still out.
    fn transmitting(&self) -> bool {
        self.free_transmit.contains(&false)
    }

    /// Fill every free transmit buffer with the next bytes of the stream
    /// and submit them.
    fn queue(&mut self, submit: &mut Producer<'_, SLOTS, EventCountKey>) {
        for (index, free) in self.free_transmit.iter_mut().enumerate() {
            if !*free || self.queued >= MESSAGE_BYTES {
                continue;
            }
            let base = DATA_AREA_VA + index as u64 * TRANSMIT_BUFFER_STRIDE;
            let mut recs = [IoRec::default(); 2];
            let mut count = 0;
            for (rec, offset) in recs.iter_mut().zip([0, TRANSMIT_RECORD_BYTES]) {
                let len = TRANSMIT_RECORD_BYTES.min((MESSAGE_BYTES - self.queued) as u64);
                if len == 0 {
                    break;
                }
                *rec = IoRec {
                    base: base + offset,
                    len,
                };
                for address in rec.base..rec.base + len {
                    // SAFETY: the data area page is the boot Thread's, and
                    // the driver holds no packet in this buffer.
                    unsafe {
                        (self.data)(address).write_volatile(message_byte(self.queued));
                    }
                    self.queued += 1;
                }
                count += 1;
            }
            let packet = Packet::new(index as u64, &recs[..count]).expect("one or two records");
            submit.put(&packet).expect("room for every transmit buffer");
            *free = false;
        }
        submit
            .signal()
            .unwrap_or_else(|error| panic!("transmit Advance failed: {:?}", error.code()));
    }

    /// A transmit packet came back: it must be whole.
    fn sent(&mut self, packet: &Packet) {
        let index = usize::try_from(packet.value()).expect("a buffer index");
        assert!(
            !self.free_transmit[index],
            "transmit buffer {index} came back twice"
        );
        let base = DATA_AREA_VA + index as u64 * TRANSMIT_BUFFER_STRIDE;
        for (rec, offset) in packet.recs().iter().zip([0, TRANSMIT_RECORD_BYTES]) {
            assert_eq!(rec.base, base + offset, "a transmit record moved");
            assert!(rec.len > 0, "a transmit record came back cut");
        }
        self.free_transmit[index] = true;
        self.packets_sent += 1;
    }

    /// A receive buffer came back: check its bytes and resubmit it.
    fn filled<Submitted: Event>(
        &mut self,
        packet: &Packet,
        give_buffer: &mut Producer<'_, SLOTS, Submitted>,
    ) {
        let index = usize::try_from(packet.value()).expect("a buffer index");
        let [rec] = packet.recs() else {
            panic!("receive buffer {index} came back with other records");
        };
        let given = receive_buffer(index).recs()[0];
        assert_eq!(rec.base, given.base, "receive buffer {index} moved");
        assert!(rec.len <= given.len, "receive buffer {index} grew");
        for offset in 0..rec.len {
            // SAFETY: the driver gave the buffer back.
            let byte = unsafe { (self.data)(rec.base + offset).read_volatile() };
            assert_eq!(
                byte,
                message_byte(self.received),
                "byte {} came back changed",
                self.received
            );
            self.received += 1;
        }
        self.buffers_filled += 1;
        give_buffer
            .put(&receive_buffer(index))
            .expect("room for every receive buffer");
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
    // The same parts Kickstart gives the shell; this client is the boot
    // Thread, which reaches them through the kernel's direct map.
    let parts = SerialChannelParts::create(&builder, &mut slots);
    let control_paddr = Builder::frame_paddr(parts.control);
    // SAFETY: a fresh, zeroed Frame the boot Thread holds; the direct map
    // covers it for the whole test, and the driver maps the same page.
    let channel =
        unsafe { SerialChannel::at(PhysAddr::new(control_paddr).user_to_kernel().as_u64()) };
    channel.config.store(LOOPBACK, Ordering::Release);
    let data_paddr =
        Builder::contiguous_frames(parts.data_first, parts.data_incarnation, DATA_PAGES);
    // The boot Thread's view of the data area byte at `address`.
    let data = |address: u64| {
        assert!(DATA_AREA.contains(&IoRec {
            base: address,
            len: 1
        }));
        PhysAddr::new(data_paddr + (address - DATA_AREA_VA))
            .user_to_kernel()
            .as_u64() as *mut u8
    };
    let (transmit_submitted, transmit_completed, receive_completed) = (
        parts.transmit_submitted,
        parts.transmit_completed,
        parts.receive_completed,
    );

    let _idle = spawn_idle(
        nucleus,
        keytable_addr,
        RawKey::from_wire(untyped.to_wire()),
        boot_as_id,
    );

    // ── Load the driver and attach before it starts ───────────────────────
    let started = load_drivers(
        nucleus,
        &builder,
        keys,
        &drivers,
        &mut slots,
        |loaded: &LoadedDriver<'_>| {
            let init = loaded.init;
            assert_eq!(init.interrupts().len(), 1, "{}: one interrupt", loaded.node);
            // Attach before the driver's Threads start, as Kickstart does for
            // a program with `serial = true`.
            parts.attach_driver(&builder, loaded.component);
        },
    );
    assert_eq!(started, 1);

    // ── Stream through the loopback ───────────────────────────────────────
    let (mut submit, mut sent) = channel.transmit.client(
        EventCountKey::from_key(transmit_submitted),
        EventCountKey::from_key(transmit_completed),
    );
    let (mut give_buffer, mut filled) = channel
        .receive
        .client(NoEvent, EventCountKey::from_key(receive_completed));
    for index in 0..RECEIVE_BUFFERS {
        give_buffer
            .put(&receive_buffer(index))
            .expect("room for every buffer");
    }

    let mut stream = Stream {
        data: &data,
        queued: 0,
        received: 0,
        free_transmit: [true; TRANSMIT_BUFFERS],
        packets_sent: 0,
        buffers_filled: 0,
    };
    let mut wakeups = 0_u32;
    while stream.received < MESSAGE_BYTES || stream.transmitting() {
        stream.queue(&mut submit);
        let mut progressed = false;
        while let Some(packet) = sent.get().expect("a consistent completion FIFO") {
            stream.sent(&packet);
            progressed = true;
        }
        while let Some(packet) = filled.get().expect("a consistent completion FIFO") {
            stream.filled(&packet, &mut give_buffer);
            progressed = true;
        }
        if progressed {
            continue;
        }
        assert_eq!(
            channel.dropped.load(Ordering::Acquire),
            0,
            "no byte dropped"
        );
        // Bytes still in flight come back on the receive channel; otherwise
        // the transmit buffers are what the stream waits for.
        wakeups += 1;
        let wait_failed = |error: WaitError<CapError>| match error {
            WaitError::Corrupt => panic!("a garbled completion FIFO"),
            WaitError::Event(error) => panic!("completion Await failed: {:?}", error.code()),
        };
        if stream.received < stream.queued {
            let packet = filled.wait().unwrap_or_else(wait_failed);
            stream.filled(&packet, &mut give_buffer);
        } else {
            let packet = sent.wait().unwrap_or_else(wait_failed);
            stream.sent(&packet);
        }
    }
    assert_eq!(
        channel.dropped.load(Ordering::Acquire),
        0,
        "no byte dropped"
    );
    assert_eq!(stream.received, MESSAGE_BYTES);
    assert!(channel.transmit.submit.is_empty() && channel.transmit.complete.is_empty());
    assert!(channel.receive.complete.is_empty());
    // More bytes than the buffers hold: the client had to wait for the driver.
    assert!(wakeups > 0, "the client never waited for the driver");
    semi::println!(
        "uart-test: {MESSAGE_BYTES} bytes looped back in {} transmit packets and {} receive buffers, {wakeups} client waits",
        stream.packets_sent,
        stream.buffers_filled
    );

    semi::println!("uart-test: ✅ PL011 driver served transmit and receive in loopback");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            libcpu::endless_sleep()
        }
    }
}

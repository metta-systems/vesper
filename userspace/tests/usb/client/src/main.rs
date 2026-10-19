#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! usb-test's program. It calls the DWC2 host controller component through the
//! `usb_hcd_v1` import Kickstart wired along the controller's compatible
//! string. It resets the root port, enumerates the device behind it with
//! `vesper_usb_protocol` over a control pipe (the work `usb-core` takes over in
//! USB3), checks that the root device is a hub, then checks the rules of pipes
//! and control stages, and reports on the key usb-test installed
//! (`driver_protocol::REPORT_SLOT`).
//!
//! A pipe is the `pipe_v1` keys `open_pipe` installs in this program's
//! table: the program lends its table, names the first of four vacant slots and
//! reads the keys' values from the data area.

use {
    driver_protocol::report_key,
    libdevice_driver::{driver_init, driver_threads},
    libio_channel::{NoEvent, Packet},
    libmeddle_runtime::IoRec,
    libobject::{EventCountKey, KeyTableKey, NotificationKey, RawKey, Rights},
    usb_hcd_client::{
        pipe_v1::{self, Exception as PipeException, TransferKind},
        usb_hcd_v1::{Bindings, Client, Error, Exception, Speed},
    },
    usb_host::{
        enumerate::{EnumerateError, Enumerated, enumerate},
        pipes::{OpenError, PIPE_KEYS, Pipe, Pipes},
    },
    vesper_usb_protocol::{
        descriptor::{self, Descriptor},
        enumeration::Speed as UsbSpeed,
        hub::{PORT_POWER, set_port_feature},
        report_channel::{ReportChannel, data_area},
        setup::{DESCRIPTOR_CONFIGURATION, DESCRIPTOR_DEVICE, SetupPacket},
    },
};

libuser::entry!(main);

/// Address the root device is enumerated to.
const ROOT_ADDRESS: u8 = 1;
/// Hub device class code (USB 2.0 §11.23).
const CLASS_HUB: u8 = 9;
/// Pipes the controller keeps open at once.
const MAX_PIPES: usize = 16;

driver_threads!(background);

/// The program's second Thread: it reports in on the component's own
/// `EventCount`, which the first Thread waits for before it starts calling.
fn background(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    EventCountKey::from_key(RawKey::from_wire(init.event_count))
        .advance(1)
        .unwrap_or_else(|error| panic!("background advance failed: {:?}", error.code()));
    let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
    }
}

/// Pipe and stage rules the controller component enforces.
#[allow(clippy::too_many_lines, reason = "one scenario, step by step")]
/// Spin for `milliseconds`.
fn pause_ms(milliseconds: u64) {
    let end = libuser::clock::now_ns() + milliseconds * 1_000_000;
    while libuser::clock::now_ns() < end {
        core::hint::spin_loop();
    }
}

/// The hub's status-change endpoint is an interrupt-IN pipe: the controller
/// polls it while the program has an empty buffer posted on the report
/// channel, and returns each report in a buffer tagged with the pipe's handle.
fn check_polling(hcd: &mut Pipes<'_>, speed: Speed) {
    use pipe_v1::Error as PipeError;

    let client = hcd.client();
    let channel = client
        .channel()
        .expect("the client shares an io-channel with the controller");
    assert_ne!(channel.event, 0, "the channel has an EventCount");
    // SAFETY: Kickstart mapped the control page read/write at the channel's base.
    let reports = unsafe { ReportChannel::at(channel.control) };
    let data = data_area(channel.control);
    let reader = libmeddle_runtime::DataArea {
        base: data.base,
        len: data.len,
    };
    let (mut buffers, mut completed) = reports.reports.client(
        NoEvent,
        EventCountKey::from_key(RawKey::from_wire(channel.event)),
    );

    // Interrupt-IN endpoints are polled; an OUT endpoint or an interval out of
    // range is refused, and the pipe is closed again.
    for (endpoint, interval) in [(0x01, 8), (0x81, 0), (0x81, 256)] {
        assert!(
            matches!(
                hcd.open_polled(1, endpoint, speed, TransferKind::Interrupt, 1, interval),
                Err(OpenError::Configure(PipeError::Exception(
                    PipeException::NotPermitted
                )))
            ),
            "{endpoint:#x} {interval}"
        );
    }
    // The periodic schedule has room for seven low-speed pipes of eight bytes
    // every millisecond, and no more.
    let mut crowd: [Option<Pipe>; 7] = [const { None }; 7];
    for slot in &mut crowd {
        *slot = Some(
            hcd.open_polled(2, 0x81, Speed::Low, TransferKind::Interrupt, 8, 1)
                .expect("room in the schedule"),
        );
    }
    assert!(
        matches!(
            hcd.open_polled(2, 0x81, Speed::Low, TransferKind::Interrupt, 8, 1),
            Err(OpenError::Configure(PipeError::Exception(
                PipeException::NoBandwidth
            )))
        ),
        "the schedule is full"
    );
    for slot in &mut crowd {
        hcd.close(&slot.take().expect("an open pipe"));
    }

    let control = hcd
        .open(1, 0, speed, TransferKind::Control, 64)
        .expect("a control pipe to the hub");
    let status = hcd
        .open_polled(1, 0x81, speed, TransferKind::Interrupt, 1, 8)
        .expect("the hub's status-change pipe");
    // Nothing is reported while no buffer waits.
    pause_ms(40);
    assert_eq!(completed.get(), Ok(None), "no buffer, no report");

    // Power the hub's first port: the keyboard behind it connects, and the hub
    // reports the change on its status-change endpoint until it is cleared.
    hcd.control(&control, set_port_feature(1, PORT_POWER), &[], &mut [])
        .expect("power port 1");
    let mut handle = None;
    for round in 0..2_u64 {
        let buffer = IoRec {
            base: data.base,
            len: 64,
        };
        let packet = Packet::new(0x77 + round, &[buffer]).expect("a packet");
        assert!(reader.write_bytes(0, &[0xAA; 8]));
        buffers.put(&packet).expect("room for the buffer");
        let end = libuser::clock::now_ns() + 3_000_000_000;
        let report = loop {
            match completed.get() {
                Ok(Some(report)) => break report,
                Ok(None) if libuser::clock::now_ns() < end => core::hint::spin_loop(),
                other => panic!("no report arrived: {other:?}"),
            }
        };
        assert_eq!(report.recs().len(), 1);
        assert_eq!(report.recs()[0].base, data.base);
        assert_eq!(report.recs()[0].len, 1, "a hub reports one bitmap byte");
        let mut bitmap = [0];
        assert!(reader.read_bytes(0, &mut bitmap));
        assert_eq!(bitmap[0] & 0b10, 0b10, "port 1 changed: {bitmap:?}");
        let mut untouched = [0];
        assert!(reader.read_bytes(1, &mut untouched));
        assert_eq!(untouched, [0xAA], "bytes past the report are untouched");
        let tag = report.value();
        assert_ne!(tag, 0x77 + round, "the report is tagged with the pipe");
        assert_eq!(*handle.get_or_insert(tag), tag, "the same pipe each time");
    }
    #[cfg(feature = "qemu")]
    libuser::semihosting::println!(
        "usb-test-client: the hub's status-change pipe {:#x} reported through the channel",
        handle.unwrap_or_default()
    );
    // Without a buffer the polls stop again.
    pause_ms(40);
    assert_eq!(completed.get(), Ok(None));

    status.client.close_pipe().expect("close");
    hcd.release(&status);
    // A closed pipe is not polled: a buffer stays where it is.
    let packet = Packet::new(
        1,
        &[IoRec {
            base: data.base,
            len: 64,
        }],
    )
    .expect("a packet");
    buffers.put(&packet).expect("room for the buffer");
    pause_ms(60);
    assert_eq!(completed.get(), Ok(None), "a closed pipe is not polled");
    hcd.close(&control);
}

fn check_rules(hcd: &mut Pipes<'_>, speed: Speed) {
    let client = hcd.client();

    // Status of the ports: the root port is connected and enabled, and no
    // other port exists.
    let status = client.port_status(0).expect("port status");
    assert_eq!(status & 0b11, 0b11, "connected and enabled: {status:#x}");
    assert_eq!(client.port_status(1).expect("port status"), 0);
    assert_eq!(
        client.reset_port(1),
        Err(Error::Exception(Exception::NoDevice)),
        "only the root port exists"
    );

    // A usable pipe to the enumerated device.
    let pipe = hcd
        .open(u32::from(ROOT_ADDRESS), 0, speed, TransferKind::Control, 64)
        .unwrap_or_else(|error| panic!("open_pipe failed: {error:?}"));
    assert!(
        pipe.keys.iter().all(|&key| key != 0),
        "every function has a key"
    );
    let device = SetupPacket::get_descriptor(DESCRIPTOR_DEVICE, 0, 18);
    let mut reply = [0; 18];
    assert_eq!(
        hcd.control(&pipe, device, &[], &mut reply).expect("works"),
        18
    );
    assert_eq!(reply[1], 1, "a device descriptor");
    pipe.client
        .clear_halt()
        .expect("clear_halt on an open pipe");

    // Stages the component refuses: too short for the setup packet, shorter
    // than the data the setup packet announces, outside the area, misplaced.
    let area = hcd.area();
    let stage = |len: u64| area.reference(0, len).expect("in the area");
    assert!(hcd.area().write_bytes(0, &device.to_bytes()));
    for refused in [stage(4), stage(8 + 17), stage(0)] {
        assert_eq!(
            pipe.client.control(refused),
            Err(pipe_v1::Error::Exception(PipeException::NotPermitted)),
            "{refused:?}"
        );
    }
    for outside in [
        IoRec { base: 0, len: 26 },
        IoRec {
            base: hcd.area().base - 8,
            len: 26,
        },
        IoRec {
            base: hcd.area().base + hcd.area().len - 8,
            len: 26,
        },
        IoRec {
            base: hcd.area().base,
            len: u64::MAX,
        },
    ] {
        assert_eq!(
            pipe.client.control(outside),
            Err(pipe_v1::Error::Exception(PipeException::NotPermitted)),
            "{outside:?}"
        );
    }
    // A data stage the controller's page cannot hold.
    let huge = SetupPacket::get_descriptor(DESCRIPTOR_CONFIGURATION, 0, 4033);
    assert!(hcd.area().write_bytes(0, &huge.to_bytes()));
    let big = IoRec {
        base: hcd.area().base,
        len: 8 + 4033,
    };
    assert_eq!(
        pipe.client.control(big),
        Err(pipe_v1::Error::Exception(PipeException::NotPermitted)),
        "the area holds the stage, the controller's page does not hold the data"
    );
    // A refused stage leaves the pipe usable.
    assert_eq!(
        hcd.control(&pipe, device, &[], &mut reply).expect("works"),
        18
    );

    // Pipes the component refuses to open.
    // An address or endpoint address that does not exist is refused at open.
    for (address, endpoint) in [(128, 0), (1, 0x10), (1, 0x90), (1, 0x100)] {
        assert!(
            matches!(
                hcd.open(address, endpoint, Speed::Full, TransferKind::Control, 8),
                Err(OpenError::Open(Error::Exception(Exception::NotPermitted)))
            ),
            "{address} {endpoint:#x}"
        );
    }
    // A packet size or kind the controller does not serve is refused by
    // `configure`, and the pipe is closed again.
    for (endpoint, kind, max_packet) in [
        (0, TransferKind::Control, 7),
        (0, TransferKind::Control, 0),
        (0, TransferKind::Control, 128),
        (0x81, TransferKind::Interrupt, 8),
        (2, TransferKind::Bulk, 64),
    ] {
        assert!(
            matches!(
                hcd.open(1, endpoint, Speed::Full, kind, max_packet),
                Err(OpenError::Configure(pipe_v1::Error::Exception(
                    PipeException::NotPermitted
                )))
            ),
            "{endpoint:#x} {max_packet}"
        );
    }
    // A pipe is configured again when a device's real packet size is learned,
    // and an unconfigured pipe carries no control transfer.
    let adjustable = hcd
        .open_unconfigured(2, 0, Speed::Full)
        .expect("an unconfigured pipe");
    assert_eq!(
        adjustable.client.control(stage(8)),
        Err(pipe_v1::Error::Exception(PipeException::NotPermitted))
    );
    for max_packet in [8, 64] {
        adjustable
            .client
            .configure(TransferKind::Control, max_packet, 0)
            .expect("configure");
    }
    hcd.close(&adjustable);
    // Slots that are not vacant, or outside the table, refuse the pipe, and the
    // controller takes the half-made pipe back.
    let taken = hcd.slot_of(pipe.group);
    for first_slot in [taken, taken + 1, u32::MAX - 1, 100_000] {
        assert_eq!(
            client.open_pipe(hcd.table(), first_slot, 2, 0, Speed::Full),
            Err(Error::Exception(Exception::NotPermitted)),
            "{first_slot:#x}"
        );
    }

    // Routes: validated, kept, and not served yet.
    let routed = hcd
        .open(2, 0, Speed::Full, TransferKind::Control, 8)
        .expect("a second pipe");
    assert_ne!(routed.keys, pipe.keys);
    for (hub, port) in [(0, 1), (1, 0), (128, 1)] {
        assert_eq!(
            routed.client.set_route(hub, port),
            Err(pipe_v1::Error::Exception(PipeException::NotPermitted)),
            "{hub} {port}"
        );
    }
    routed
        .client
        .set_route(1, 2)
        .expect("a full-speed device can sit behind a hub");
    // A routed pipe runs split transactions. QEMU's controller has no
    // transaction translator behind its hub, so the start split gets no answer:
    // the transfer times out and fails, and the pipe stays usable.
    assert!(
        matches!(
            routed.client.control(stage(26)),
            Err(pipe_v1::Error::Exception(
                PipeException::TransferFailed { .. }
            ))
        ),
        "a split transaction needs a high-speed hub"
    );
    let fast = hcd
        .open(3, 0, Speed::High, TransferKind::Control, 64)
        .expect("a high-speed pipe");
    assert_eq!(
        fast.client.set_route(1, 2),
        Err(pipe_v1::Error::Exception(PipeException::NotPermitted))
    );

    // Holding a key is the right to use the pipe: a copy of the keys in other
    // slots works, closing through the copy closes the pipe for every holder,
    // and the keys left behind fail.
    let copy_group = hcd.take_group();
    let copy_slot = hcd.slot_of(copy_group);
    let mut copies = [0; PIPE_KEYS];
    for (index, (copy, &key)) in copies.iter_mut().zip(&fast.keys).enumerate() {
        let slot = copy_slot + u32::try_from(index).expect("a small index");
        *copy = hcd
            .table()
            .copy_derive(
                RawKey::from_wire(key),
                hcd.table(),
                slot,
                Rights(Rights::CALL),
            )
            .unwrap_or_else(|error| panic!("copying a pipe key failed: {:?}", error.code()))
            .to_wire();
    }
    let delegated = Pipe {
        client: hcd.pipe_client(&copies),
        group: copy_group,
        keys: copies,
    };
    delegated.client.clear_halt().expect("a copy works");
    delegated.client.close_pipe().expect("a copy closes");
    // The copy and the original name a pipe that is gone now.
    for gone in [&delegated, &fast] {
        assert_eq!(
            gone.client.clear_halt(),
            Err(pipe_v1::Error::Exception(PipeException::InvalidPipe))
        );
    }
    hcd.release(&delegated);
    hcd.release(&fast);

    // Keys of a closed pipe keep failing, even when its table slot is reused by
    // a new pipe.
    routed.client.close_pipe().expect("close");
    assert_eq!(
        routed.client.control(stage(26)),
        Err(pipe_v1::Error::Exception(PipeException::InvalidPipe))
    );
    let reused = hcd
        .open(2, 0, Speed::Full, TransferKind::Control, 8)
        .expect("the slot is free again");
    assert_ne!(
        reused.keys, routed.keys,
        "a reused pipe slot has a new badge"
    );
    for result in [
        routed.client.clear_halt(),
        routed.client.set_route(1, 2),
        routed.client.close_pipe(),
    ] {
        assert_eq!(
            result,
            Err(pipe_v1::Error::Exception(PipeException::InvalidPipe))
        );
    }
    reused.client.clear_halt().expect("the new pipe is fine");
    hcd.close(&reused);
    hcd.release(&routed);

    // The table has room for MAX_PIPES pipes, and no more.
    hcd.close(&pipe);
    let mut pipes: [Option<Pipe>; MAX_PIPES] = [const { None }; MAX_PIPES];
    for slot in &mut pipes {
        *slot = Some(
            hcd.open(1, 0, Speed::Full, TransferKind::Control, 8)
                .expect("room for the pipe"),
        );
    }
    assert!(
        matches!(
            hcd.open_unconfigured(1, 0, Speed::Full),
            Err(Error::Exception(Exception::Busy))
        ),
        "the table is full"
    );
    for slot in &mut pipes {
        hcd.close(&slot.take().expect("an open pipe"));
    }
    let last = hcd
        .open(1, 0, Speed::Full, TransferKind::Control, 8)
        .expect("room again");
    hcd.close(&last);
}

fn main(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    EventCountKey::from_key(RawKey::from_wire(init.event_count))
        .await_ge(1, EventCountKey::WAIT_INFINITE)
        .unwrap_or_else(|error| {
            panic!(
                "waiting for the background Thread failed: {:?}",
                error.code()
            )
        });
    let client = Client::new(Bindings::from_imports(init.imports()));
    let area = client
        .data_area()
        .expect("the client shares a data area with the controller");
    assert_eq!(area.len, 4096);
    // The io-channel follows the data area: a zeroed control page and a data
    // page, both mapped read/write.
    let channel = client
        .channel()
        .expect("the client shares an io-channel with the controller");
    assert_eq!(channel.control, area.base + 4096);
    assert_eq!(channel.data.len, 4096);
    let control = libmeddle_runtime::DataArea {
        base: channel.control,
        len: 4096,
    };
    let mut words = [u32::MAX; 4];
    assert!(control.read_words(0, &mut words));
    assert_eq!(words, [0; 4], "a fresh channel is idle");
    for page in [control, channel.data] {
        assert!(page.write_words(8, &[0xfeed_f00d]));
        let mut back = [0];
        assert!(page.read_words(8, &mut back));
        assert_eq!(back, [0xfeed_f00d]);
        assert!(page.write_words(8, &[0]));
    }
    let mut hcd = Pipes::new(
        &client,
        area,
        KeyTableKey::from_key(RawKey::from_wire(init.table)),
        u32::try_from(init.first_free_slot).expect("a slot fits in u32"),
    );

    assert_eq!(
        client.port_status(0).expect("port status") & 1,
        1,
        "a device is attached"
    );
    let Enumerated {
        device,
        configuration,
        configuration_len,
        speed,
        ..
    } = enumerate(
        &mut hcd,
        ROOT_ADDRESS,
        None,
        || {
            client
                .reset_port(0)
                .map(|reset| match reset {
                    Speed::Low => UsbSpeed::Low,
                    Speed::Full => UsbSpeed::Full,
                    Speed::High => UsbSpeed::High,
                })
                .map_err(|_error| EnumerateError::Reset)
        },
        |milliseconds| {
            let end = libuser::clock::now_ns() + u64::from(milliseconds) * 1_000_000;
            while libuser::clock::now_ns() < end {
                core::hint::spin_loop();
            }
        },
        |_setup, _result| {},
    )
    .unwrap_or_else(|error| panic!("enumeration failed: {error:?}"));
    let Some(Ok(Descriptor::Device(root))) = descriptor::walk(&device).next() else {
        panic!("the root device's descriptor is malformed");
    };
    let interfaces = descriptor::walk(configuration.get(..configuration_len).unwrap_or_default())
        .filter_map(|descriptor| match descriptor {
            Ok(Descriptor::Interface(interface)) => Some(interface.class_code),
            _ => None,
        })
        .count();
    #[cfg(feature = "qemu")]
    libuser::semihosting::println!(
        "usb-test-client: root port {speed:?}: device at address {ROOT_ADDRESS}, class {:#x}, vendor {:#06x} product {:#06x}, {interfaces} interfaces",
        root.class_code,
        root.vendor,
        root.product
    );
    assert_eq!(root.class_code, CLASS_HUB, "the root device is a hub");
    assert!(interfaces > 0, "the hub has an interface");

    let speed = match speed {
        UsbSpeed::Low => Speed::Low,
        UsbSpeed::Full => Speed::Full,
        UsbSpeed::High => Speed::High,
    };
    check_rules(&mut hcd, speed);
    check_polling(&mut hcd, speed);

    NotificationKey::from_key(report_key(init.table))
        .signal(0)
        .unwrap_or_else(|error| panic!("report failed: {:?}", error.code()));
    let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
    }
}

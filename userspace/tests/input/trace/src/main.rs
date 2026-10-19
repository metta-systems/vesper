#![no_std]
#![no_main]

//! input-trace's program: the consumer of the USB HID component's `input_v1`.
//!
//! It posts empty buffers on the interface's io-channel and traces every record
//! that comes back on the serial channel (and, under QEMU, the semihosting
//! console), so a person at a keyboard sees what the producer publishes. Since
//! a `DeviceAdded` record sent before the program posted a buffer is lost, it
//! also asks for the descriptor of device ids from 1 on, as `input_v1`
//! prescribes, and traces each device it finds. When it has found a device and
//! read its state it signals the report key `driver_protocol::REPORT_SLOT`.

use {
    driver_protocol::report_key,
    input_client::input_v1::{Bindings, Client, Error, Exception},
    libdevice_driver::{DriverInit, INIT_VA, driver_init},
    libio_channel::{IoRec, NoEvent, Packet},
    libobject::{CapError, EventCountKey, NotificationKey, RawKey},
    libserial_trace::{Trace, trace},
    usb_hid::channel::{EventChannel, data_area},
    vesper_input_event::{
        descriptor::{DESCRIPTOR_SIZE, Descriptor},
        record::{EVENT_SIZE, Event},
    },
};

libuser::entry!(main);

/// Empty buffers kept posted.
const BUFFERS: u64 = 4;
/// Bytes of each: the data page split in four.
const BUFFER_LEN: usize = 1024;
const BUFFER_BYTES: u64 = BUFFER_LEN as u64;
/// Milliseconds between two looks for new devices.
const RECHECK_MS: u64 = 250;
/// Device ids asked for.
const MAX_SUBJECT: u32 = 8;

fn init() -> &'static DriverInit {
    // SAFETY: Kickstart maps every component's init record at `INIT_VA`.
    unsafe { driver_init(INIT_VA) }
}

fn main(_init_va: u64) -> ! {
    let init = init();
    let mut trace = Trace::new(init);
    trace!(trace, "input-trace: starting");
    let client = Client::new(Bindings::from_imports(init.imports()));
    let area = client
        .data_area()
        .expect("input-trace shares a data area with the producer");
    let channel_area = client
        .channel()
        .expect("input-trace shares an io-channel with the producer");
    assert_ne!(channel_area.event, 0, "the channel has an EventCount");
    // SAFETY: Kickstart mapped the control page read/write.
    let channel = unsafe { EventChannel::at(channel_area.control) };
    let data = data_area(channel_area.control);
    let event = EventCountKey::from_key(RawKey::from_wire(channel_area.event));
    let (mut buffers, mut completed) = channel.events.client(
        NoEvent,
        EventCountKey::from_key(RawKey::from_wire(channel_area.event)),
    );
    for index in 0..BUFFERS {
        let rec = IoRec {
            base: data.base + index * BUFFER_BYTES,
            len: BUFFER_BYTES,
        };
        let packet = Packet::new(index, &[rec]).expect("one record");
        buffers.put(&packet).expect("room for the buffers");
    }

    let mut known = [false; MAX_SUBJECT as usize + 1];
    let mut reported = false;
    let mut seen = 0;
    loop {
        for subject in 1..=MAX_SUBJECT {
            if known[subject as usize] {
                continue;
            }
            let Some(stage) = area.reference(0, DESCRIPTOR_SIZE as u64) else {
                continue;
            };
            match client.descriptor(subject, stage) {
                Ok(()) => {
                    let mut bytes = [0; DESCRIPTOR_SIZE];
                    assert!(area.read_bytes(0, &mut bytes));
                    let Ok(descriptor) = Descriptor::from_bytes(&bytes) else {
                        trace!(
                            trace,
                            "input-trace: device {subject}: a malformed descriptor"
                        );
                        continue;
                    };
                    known[subject as usize] = true;
                    trace!(
                        trace,
                        "input-trace: device {subject}: {} (USB {:04x}:{:04x})",
                        core::str::from_utf8(descriptor.name()).unwrap_or("?"),
                        descriptor.vendor(),
                        descriptor.product()
                    );
                    // The state a device starts with: nothing held, a Sync.
                    let length = client
                        .snapshot(subject, stage)
                        .unwrap_or_else(|error| panic!("snapshot failed: {error:?}"));
                    trace!(
                        trace,
                        "input-trace: device {subject}: snapshot of {length} bytes"
                    );
                    if !reported {
                        reported = true;
                        NotificationKey::from_key(report_key(init.table))
                            .signal(0)
                            .unwrap_or_else(|error| panic!("report failed: {:?}", error.code()));
                    }
                }
                Err(Error::Exception(Exception::NoDevice)) => {}
                Err(error) => panic!("descriptor failed: {error:?}"),
            }
        }

        match event.await_ge(seen + 1, RECHECK_MS * 1_000_000) {
            Ok(count) => seen = count,
            Err(CapError::TimedOut { .. }) => {}
            Err(error) => panic!("waiting for events failed: {:?}", error.code()),
        }
        while let Ok(Some(packet)) = completed.get() {
            let Some(rec) = packet.recs().first().copied() else {
                continue;
            };
            let length = usize::try_from(rec.len).unwrap_or(0).min(BUFFER_LEN);
            let mut bytes = [0_u8; BUFFER_LEN];
            for (offset, byte) in bytes[..length].iter_mut().enumerate() {
                // SAFETY: the buffer lies in the channel's data page, mapped
                // read/write; the producer gave it back.
                *byte = unsafe {
                    core::ptr::with_exposed_provenance::<u8>(
                        usize::try_from(rec.base).unwrap_or(0) + offset,
                    )
                    .read_volatile()
                };
            }
            for record in bytes[..length].chunks(EVENT_SIZE) {
                match Event::from_bytes(record) {
                    Ok(event) => trace!(
                        trace,
                        "input-trace: {:?} device {} code {:#010x} value {}",
                        event.kind,
                        event.subject,
                        event.code,
                        event.value
                    ),
                    Err(error) => trace!(trace, "input-trace: a bad record: {error:?}"),
                }
            }
            // Post the buffer again.
            let again = IoRec {
                base: rec.base,
                len: BUFFER_BYTES,
            };
            if data.contains(&again)
                && let Some(packet) = Packet::new(0, &[again])
            {
                let _posted = buffers.put(&packet);
            }
        }
    }
}

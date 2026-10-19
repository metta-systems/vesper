#![no_std]
#![no_main]

//! The compositor program. It is the only consumer of the USB HID component's
//! input stream: it posts empty buffers on the `input_v1` io-channel, folds the
//! records that come back into a [`Router`] and sends what the router decides
//! to windows.
//!
//! The windows are fixed for now (the clients and the screen come in later
//! stages), and what each window receives is traced on the serial channel, so
//! a person at a mouse and a keyboard sees the pointer, the focus, the
//! implicit grab and the keys follow the routing rules.
//!
//! A device the stream added before the compositor posted a buffer, and the
//! records a `Dropped` marker says were lost, are recovered with the calls
//! `input_v1` serves: `descriptor` finds the devices, `snapshot` gives their
//! state, and the router replaces what it knew.

use {
    driver_protocol::report_key,
    input_client::input_v1::{Bindings, Client, Error, Exception},
    libcompositor_core::{
        router::{Output, Router},
        windows::{Rect, Window},
    },
    libdevice_driver::{DriverInit, INIT_VA, driver_init},
    libio_channel::{IoRec, NoEvent, Packet},
    libmeddle_runtime::DataArea,
    libobject::{CapError, EventCountKey, NotificationKey, RawKey},
    libserial_trace::{Trace, trace},
    usb_hid::channel::{EventChannel, data_area},
    vesper_input_event::{
        descriptor::{DESCRIPTOR_SIZE, Descriptor},
        record::{EVENT_SIZE, Event, Kind},
    },
};

libuser::entry!(main);

/// The screen the pointer moves on, until the compositor owns the display.
const SCREEN_WIDTH: u32 = 1024;
const SCREEN_HEIGHT: u32 = 768;
/// Empty buffers kept posted, and the bytes of each: the data page split in four.
const BUFFERS: u64 = 4;
const BUFFER_LEN: usize = 1024;
const BUFFER_BYTES: u64 = BUFFER_LEN as u64;
/// Milliseconds between two looks for new devices.
const RECHECK_MS: u64 = 250;
/// Device ids asked for.
const MAX_SUBJECT: u32 = 8;
/// Records of one snapshot: a frame of `input_v1` holds at most 24.
const SNAPSHOT_EVENTS: usize = 24;

fn init() -> &'static DriverInit {
    // SAFETY: Kickstart maps every component's init record at `INIT_VA`.
    unsafe { driver_init(INIT_VA) }
}

/// The windows of the demonstration: three that overlap, so a drag crosses one
/// window on its way to another.
const WINDOWS: [Window; 3] = [
    Window {
        id: 1,
        client: 1,
        rect: Rect {
            x: 100,
            y: 100,
            width: 400,
            height: 300,
        },
    },
    Window {
        id: 2,
        client: 1,
        rect: Rect {
            x: 300,
            y: 250,
            width: 400,
            height: 300,
        },
    },
    Window {
        id: 3,
        client: 1,
        rect: Rect {
            x: 700,
            y: 50,
            width: 250,
            height: 200,
        },
    },
];

/// What the compositor knows of the input producer.
struct Input<'a> {
    client: &'a Client,
    area: DataArea,
    known: [bool; MAX_SUBJECT as usize + 1],
}

impl Input<'_> {
    /// The descriptor of device `subject`, if the producer serves it.
    fn descriptor(&self, subject: u32) -> Option<Descriptor> {
        let stage = self.area.reference(0, DESCRIPTOR_SIZE as u64)?;
        match self.client.descriptor(subject, stage) {
            Ok(()) => {}
            Err(Error::Exception(Exception::NoDevice)) => return None,
            Err(error) => panic!("descriptor failed: {error:?}"),
        }
        let mut bytes = [0; DESCRIPTOR_SIZE];
        self.area.read_bytes(0, &mut bytes).then_some(())?;
        Descriptor::from_bytes(&bytes).ok()
    }

    /// The records that rebuild the state of device `subject`.
    fn snapshot(&self, subject: u32, events: &mut [Event; SNAPSHOT_EVENTS]) -> usize {
        let Some(stage) = self
            .area
            .reference(0, (SNAPSHOT_EVENTS * EVENT_SIZE) as u64)
        else {
            return 0;
        };
        let Ok(length) = self.client.snapshot(subject, stage) else {
            return 0;
        };
        let length = usize::try_from(length).unwrap_or(0);
        let mut bytes = [0; SNAPSHOT_EVENTS * EVENT_SIZE];
        if !self.area.read_bytes(0, &mut bytes[..length]) {
            return 0;
        }
        let mut count = 0;
        for (slot, record) in events.iter_mut().zip(bytes[..length].chunks(EVENT_SIZE)) {
            if let Ok(event) = Event::from_bytes(record) {
                *slot = event;
                count += 1;
            }
        }
        count
    }
}

fn main(_init_va: u64) -> ! {
    let init = init();
    let mut trace = Trace::new(init);
    trace!(trace, "compositor: starting");
    let client = Client::new(Bindings::from_imports(init.imports()));
    let area = client
        .data_area()
        .expect("the compositor shares a data area with the producer");
    let channel_area = client
        .channel()
        .expect("the compositor shares an io-channel with the producer");
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

    let mut router = Router::new(SCREEN_WIDTH, SCREEN_HEIGHT);
    for window in WINDOWS {
        assert!(router.windows_mut().add(window));
    }
    let mut input = Input {
        client: &client,
        area,
        known: [false; MAX_SUBJECT as usize + 1],
    };
    let mut out = Output::new();
    let mut reported = false;
    let mut seen = 0;
    loop {
        // Devices announced before a buffer was posted, or lost since.
        for subject in 1..=MAX_SUBJECT {
            if input.known[subject as usize] {
                continue;
            }
            let Some(descriptor) = input.descriptor(subject) else {
                continue;
            };
            input.known[subject as usize] = true;
            trace!(
                trace,
                "compositor: device {subject}: {} (USB {:04x}:{:04x})",
                core::str::from_utf8(descriptor.name()).unwrap_or("?"),
                descriptor.vendor(),
                descriptor.product()
            );
            router.feed(&Event::new(0, subject, Kind::DeviceAdded, 0, 0), &mut out);
            router.feed(&Event::sync(0, subject), &mut out);
            resync(&mut router, &input, subject, &mut out);
            if !reported {
                reported = true;
                NotificationKey::from_key(report_key(init.table))
                    .signal(0)
                    .unwrap_or_else(|error| panic!("report failed: {:?}", error.code()));
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
                let Ok(event) = Event::from_bytes(record) else {
                    trace!(trace, "compositor: a bad record");
                    continue;
                };
                match event.kind {
                    // The next look for devices reads its descriptor and state.
                    Kind::DeviceAdded => {
                        trace!(trace, "compositor: device {} added", event.subject);
                    }
                    Kind::DeviceRemoved => {
                        let subject = event.subject;
                        if let Some(known) = input.known.get_mut(subject as usize) {
                            *known = false;
                        }
                        trace!(trace, "compositor: device {subject} removed");
                    }
                    Kind::Dropped => trace!(trace, "compositor: the producer dropped records"),
                    _ => {}
                }
                router.feed(&event, &mut out);
                if event.kind == Kind::Dropped {
                    for subject in 1..=MAX_SUBJECT {
                        if input.known[subject as usize] {
                            resync(&mut router, &input, subject, &mut out);
                        }
                    }
                }
                if event.kind == Kind::Sync {
                    report(&mut trace, &router, &out);
                    out.clear();
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

/// Replace what the router knows of device `subject` with the producer's
/// snapshot.
fn resync(router: &mut Router, input: &Input<'_>, subject: u32, out: &mut Output) {
    let mut events = [Event::sync(0, 0); SNAPSHOT_EVENTS];
    let count = input.snapshot(subject, &mut events);
    router.resync(subject, &events[..count], out);
}

/// Trace what the windows are to receive for one producer frame.
fn report(trace: &mut Trace, router: &Router, out: &Output) {
    for routed in out.items() {
        let event = routed.event;
        if event.kind == Kind::Sync {
            continue;
        }
        trace!(
            trace,
            "compositor: window {}: {:?} code {:#010x} value {}",
            routed.window,
            event.kind,
            event.code,
            event.value
        );
    }
    if !out.items().is_empty() {
        let (x, y) = router.pointer();
        trace!(
            trace,
            "compositor: pointer {x},{y} hover {:?} focus {:?}",
            router.hover(),
            router.focus()
        );
    }
}

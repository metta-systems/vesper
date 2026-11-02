//! The component's Thread and its `log_v1` export (see the crate
//! documentation in `main.rs`).

use {
    libdevice_driver::{DriverInit, INIT_VA, driver_init},
    libio_channel::{Consumer, DataArea, IoRec, NoEvent, Packet, Producer},
    libobject::{
        CapError, EventCountKey, RawKey, export::init_return_key, thread::ThreadReturnKey,
    },
    libserial_channel::{CONTROL_VA, DATA_AREA_VA, DATA_PAGES, SLOTS, SerialChannel},
    libuser::semihosting as semi,
    log_interface::log_v1::{CHANNEL_PAGES as DECLARED_PAGES, LogV1},
    log_server::{
        CHANNEL_PAGES, CHANNEL_SLOTS, Header, LINE_BYTES, Level, OUTPUT_LINE_BYTES, channel_at,
        format_line,
    },
};

const _: () = assert!(
    DECLARED_PAGES == CHANNEL_PAGES,
    "log_v1.if and the crate agree"
);

libuser::entry!(main);

fn init() -> &'static DriverInit {
    // SAFETY: Kickstart maps every component's init record at `INIT_VA`; it
    // is read in this component's own `AddressSpace` only.
    unsafe { driver_init(INIT_VA) }
}

/// How long the Thread sleeps when nothing wakes it: a line put without a
/// `flush` waits at most this long.
const POLL_NS: u64 = 20_000_000;

/// Bytes of one serial buffer: several lines go to the driver in one packet.
const SERIAL_BUFFER_BYTES: u64 = 1024;

/// Serial buffers in the serial channel's data area.
const SERIAL_BUFFERS: u32 = 16;

const _: () = assert!(
    SERIAL_BUFFERS as u64 * SERIAL_BUFFER_BYTES == DATA_PAGES as u64 * 4096,
    "the buffers fill the data area"
);
const _: () = assert!(
    SERIAL_BUFFERS as usize * 2 <= SLOTS,
    "every serial buffer fits the transmit FIFO at once"
);
const _: () = assert!(SERIAL_BUFFER_BYTES >= OUTPUT_LINE_BYTES as u64);

/// One client's channel, server side.
struct Client {
    lines: Consumer<'static, CHANNEL_SLOTS, NoEvent>,
    returns: Producer<'static, CHANNEL_SLOTS, NoEvent>,
    data: DataArea,
}

/// The transmit side of the PL011's serial channel, with the buffer being
/// filled.
struct Serial {
    submit: Producer<'static, SLOTS, EventCountKey>,
    sent: Consumer<'static, SLOTS, EventCountKey>,
    sent_count: EventCountKey,
    /// Buffers not in flight, one bit each
    free: u32,
    /// The buffer being filled and its bytes so far
    filling: Option<(u32, u64)>,
    /// Packets put since the last signal
    unsignalled: bool,
}

impl Serial {
    /// The serial channel Kickstart attached, if the image gives this
    /// component one (`serial = true`).
    fn attach(init: &DriverInit) -> Option<Self> {
        let keys = init.serial;
        if keys.transmit_submitted == 0 {
            return None;
        }
        // SAFETY: Kickstart mapped the control page read/write at
        // `CONTROL_VA` for this component's lifetime.
        let channel = unsafe { SerialChannel::at(CONTROL_VA) };
        let event = |wire: u64| EventCountKey::from_key(RawKey::from_wire(wire));
        let (submit, sent) = channel.transmit.client(
            event(keys.transmit_submitted),
            event(keys.transmit_completed),
        );
        Some(Self {
            submit,
            sent,
            sent_count: event(keys.transmit_completed),
            free: (1 << SERIAL_BUFFERS) - 1,
            filling: None,
            unsignalled: false,
        })
    }

    /// Take back the buffers the driver is done with.
    fn reclaim(&mut self) {
        while let Ok(Some(back)) = self.sent.get() {
            if let Ok(index) = u32::try_from(back.value())
                && index < SERIAL_BUFFERS
            {
                self.free |= 1 << index;
            }
        }
    }

    /// Room for one more output line: the buffer being filled has it, or
    /// a free one does.
    fn has_room(&self) -> bool {
        self.free != 0
            || self
                .filling
                .is_some_and(|(_index, len)| len + OUTPUT_LINE_BYTES as u64 <= SERIAL_BUFFER_BYTES)
    }

    /// Where buffer `index` lies.
    const fn buffer_base(index: u32) -> u64 {
        DATA_AREA_VA + index as u64 * SERIAL_BUFFER_BYTES
    }

    /// Format a line into the buffer being filled (starting one if needed).
    /// The caller checked [`has_room`](Self::has_room).
    fn write(&mut self, header: Header, text: &[u8]) {
        let (index, len) = match self.filling {
            Some((index, len)) if len + OUTPUT_LINE_BYTES as u64 <= SERIAL_BUFFER_BYTES => {
                (index, len)
            }
            _ => {
                self.put();
                let index = self.free.trailing_zeros();
                if index >= SERIAL_BUFFERS {
                    return;
                }
                self.free &= !(1 << index);
                (index, 0)
            }
        };
        let address = usize::try_from(Self::buffer_base(index) + len).unwrap_or(usize::MAX);
        // SAFETY: the buffer lies in the serial data area, mapped read/write
        // for good; the driver reads a buffer only while it holds the packet
        // naming it, and this one is not in flight. `has_room` checked
        // `OUTPUT_LINE_BYTES` fit after `len`.
        let out = unsafe {
            core::slice::from_raw_parts_mut(
                core::ptr::with_exposed_provenance_mut::<u8>(address),
                OUTPUT_LINE_BYTES,
            )
        };
        let written = format_line(header, text, out);
        self.filling = Some((index, len + written as u64));
    }

    /// Hand the buffer being filled to the driver.
    fn put(&mut self) {
        let Some((index, len)) = self.filling.take() else {
            return;
        };
        let record = IoRec {
            base: Self::buffer_base(index),
            len,
        };
        let packet = Packet::new(u64::from(index), &[record]).expect("one record");
        // The FIFO holds every buffer at once, so this never fails.
        if self.submit.put(&packet).is_err() {
            self.free |= 1 << index;
            return;
        }
        self.unsignalled = true;
    }

    /// Put the buffer being filled and tell the driver about what was put.
    fn send(&mut self) {
        self.put();
        if core::mem::take(&mut self.unsignalled) {
            let _signalled = self.submit.signal();
        }
    }
}

/// The server's own line: on the semihosting console under QEMU, and on the
/// UART.
fn own_line(serial: &mut Option<Serial>, text: &str) {
    semi::println!("{text}");
    if let Some(serial) = serial
        && serial.has_room()
    {
        let header = Header {
            level: Some(Level::Info),
            cut: false,
            dropped: 0,
            micros: libuser::clock::now_ns() / 1000,
        };
        serial.write(header, text.as_bytes());
        serial.send();
    }
}

fn main(_init_va: u64) -> ! {
    let init = init();
    let wake = EventCountKey::from_key(RawKey::from_wire(init.event_count));
    let mut serial = Serial::attach(init);
    let mut clients = [const { None }; libdevice_driver::MAX_AREAS];
    let mut count = 0;
    for (slot, area) in clients.iter_mut().zip(init.areas()) {
        if area.channel_base == 0 {
            continue;
        }
        // SAFETY: Kickstart mapped the client's control page read/write for
        // good, shared with that client only.
        let channel = unsafe { channel_at(area.channel_base) };
        let (lines, returns) = channel.server(NoEvent, NoEvent);
        *slot = Some(Client {
            lines,
            returns,
            data: DataArea {
                base: area.channel_base + 4096,
                len: CHANNEL_PAGES * 4096,
            },
        });
        count += 1;
    }
    let mut line = [0_u8; 64];
    let text = {
        use core::fmt::Write;
        let mut writer = log_server::LineWriter::new(&mut line);
        let _written = write!(
            writer,
            "log-server: {count} clients, serial output {}",
            if serial.is_some() {
                "attached"
            } else {
                "missing"
            }
        );
        writer.finish().0
    };
    own_line(
        &mut serial,
        core::str::from_utf8(&line[..text]).unwrap_or("log-server: started"),
    );
    let mut seen = wake.read().unwrap_or(0);
    loop {
        let moved = serve_once(&mut clients, serial.as_mut());
        if let Some(serial) = &mut serial
            && !serial.has_room()
        {
            // Every serial buffer is in flight: wait for the driver.
            let done = serial.sent_count.read().unwrap_or(0);
            let _woken = serial.sent_count.await_ge(done + 1, POLL_NS);
            continue;
        }
        if moved {
            continue;
        }
        seen = match wake.await_ge(seen + 1, POLL_NS) {
            Ok(value) | Err(CapError::TimedOut { value }) => value,
            Err(_error) => seen,
        };
    }
}

/// One pass over the clients: at most one line from each, in turn. Whether
/// any line was taken.
fn serve_once(clients: &mut [Option<Client>], mut serial: Option<&mut Serial>) -> bool {
    if let Some(serial) = serial.as_deref_mut() {
        serial.reclaim();
    }
    let mut moved = false;
    for client in clients.iter_mut().flatten() {
        if serial.as_deref().is_some_and(|serial| !serial.has_room()) {
            break;
        }
        let Ok(Some(mut packet)) = client.lines.get() else {
            continue;
        };
        moved = true;
        let record = packet.recs().first().copied();
        if let Some(record) = record
            && packet.recs().len() == 1
            && record.len <= LINE_BYTES
            && client.data.contains(&record)
            && let Some(serial) = serial.as_deref_mut()
        {
            let address = usize::try_from(record.base).unwrap_or(usize::MAX);
            let len = usize::try_from(record.len).unwrap_or(0);
            // SAFETY: the client's data area holds the record; it is mapped
            // read/write for good, and the client leaves a buffer alone while
            // its packet is here. A client that does not only garbles its own
            // line.
            let text = unsafe {
                core::slice::from_raw_parts(core::ptr::with_exposed_provenance::<u8>(address), len)
            };
            serial.write(Header::decode(packet.value()), text);
        } else if let Some(record) = packet.recs_mut().first_mut() {
            // Malformed, or no serial output: back unwritten.
            record.len = 0;
        }
        // The return FIFO holds every buffer the client has, so this never
        // fails for a client that keeps to the protocol.
        let _returned = client.returns.put(&packet);
    }
    if let Some(serial) = serial {
        serial.send();
    }
    moved
}

/// The log server, behind `log_v1`. The calls run on their callers' Threads.
struct Server;

impl LogV1 for Server {
    fn enter(_badge: u64) {
        init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
            init().return_key,
        )));
    }

    fn flush(_badge: u64) {
        // A failed advance only delays the lines until the next poll.
        let _advanced = EventCountKey::from_key(RawKey::from_wire(init().event_count)).advance(1);
    }
}

log_interface::log_v1_export!(Server);

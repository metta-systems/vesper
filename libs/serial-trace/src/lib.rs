#![no_std]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! Progress lines for the people bringing a program up, on the semihosting
//! console under QEMU and, in order of preference:
//!
//! - on the component's log channel when it imports `log_v1` and the image
//!   has a log server (`liblog-client`): never waits, a line that finds no
//!   buffer free is dropped and counted;
//! - on the serial channel when the component has one (`serial = true` in its
//!   manifest entry): waits for the driver to send each line;
//! - otherwise in a backlog that the component hands to the program holding
//!   the serial channel through an export of its own ([`drain`]).

use {
    core::{
        cell::UnsafeCell,
        fmt::{self, Write},
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    libdevice_driver::DriverInit,
    libio_channel::{Consumer, IoRec, Packet, Producer},
    liblog_client::{Level, Sent},
    libobject::{EventCountKey, RawKey},
    libserial_channel::{CONTROL_VA, DATA_AREA_VA, SLOTS, SerialChannel},
    libuser::semihosting as semi,
};

/// Bytes of one line, without its line end: a longer line is cut.
const LINE_BYTES: usize = 240;

/// The transmit side of the serial channel: the client of the PL011 driver that
/// only sends.
struct SerialLog {
    submit: Producer<'static, SLOTS, EventCountKey>,
    sent: Consumer<'static, SLOTS, EventCountKey>,
}

impl SerialLog {
    /// The log on the channel `init` has keys for, if the component has one.
    fn new(init: &DriverInit) -> Option<Self> {
        let keys = init.serial;
        if keys.transmit_submitted == 0 {
            return None;
        }
        // SAFETY: Kickstart mapped the control page read/write at `CONTROL_VA`
        // for this component's lifetime.
        let channel = unsafe { SerialChannel::at(CONTROL_VA) };
        let event = |wire: u64| EventCountKey::from_key(RawKey::from_wire(wire));
        let (submit, sent) = channel.transmit.client(
            event(keys.transmit_submitted),
            event(keys.transmit_completed),
        );
        Some(Self { submit, sent })
    }

    /// Send `bytes` as one packet and wait for the driver to be done with it.
    fn send(&mut self, bytes: &[u8]) {
        for (offset, &byte) in bytes.iter().enumerate() {
            // SAFETY: the data area is mapped read/write for this component's
            // lifetime, and the driver holds no packet in it: each packet is
            // waited for.
            unsafe {
                ((DATA_AREA_VA + offset as u64) as *mut u8).write_volatile(byte);
            }
        }
        let packet = Packet::new(
            0,
            &[IoRec {
                base: DATA_AREA_VA,
                len: bytes.len() as u64,
            }],
        )
        .expect("one record");
        self.submit
            .put(&packet)
            .expect("room for the one transmit packet");
        self.submit
            .signal()
            .unwrap_or_else(|error| panic!("transmit Advance failed: {:?}", error.code()));
        self.sent
            .wait()
            .unwrap_or_else(|_| panic!("the transmit channel failed"));
    }
}

/// Where a line is assembled before it is sent whole.
struct Line {
    bytes: [u8; LINE_BYTES + 2],
    len: usize,
}

impl Write for Line {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for &byte in text.as_bytes() {
            if self.len < LINE_BYTES {
                self.bytes[self.len] = byte;
                self.len += 1;
            }
        }
        Ok(())
    }
}

/// Bytes of trace the backlog holds; a line that does not fit is dropped.
const BACKLOG_BYTES: usize = 4096;

/// A ring of trace bytes: writers take turns through `writing` (a line that
/// finds it taken is dropped), one reader at a time drains it.
struct Backlog {
    bytes: UnsafeCell<[u8; BACKLOG_BYTES]>,
    writing: AtomicBool,
    /// Bytes written and read so far; their difference is what is waiting.
    written: AtomicUsize,
    read: AtomicUsize,
}

// SAFETY: a writer only touches bytes past `written`, the reader only bytes
// before it; `writing` admits one writer and the counters order the two sides.
unsafe impl Sync for Backlog {}

static BACKLOG: Backlog = Backlog {
    bytes: UnsafeCell::new([0; BACKLOG_BYTES]),
    writing: AtomicBool::new(false),
    written: AtomicUsize::new(0),
    read: AtomicUsize::new(0),
};

impl Backlog {
    fn push(&self, line: &[u8]) {
        if self.writing.swap(true, Ordering::Acquire) {
            return;
        }
        let written = self.written.load(Ordering::Relaxed);
        if written - self.read.load(Ordering::Acquire) + line.len() <= BACKLOG_BYTES {
            for (offset, &byte) in line.iter().enumerate() {
                // SAFETY: the reader does not look past `written`, stored below.
                unsafe {
                    (*self.bytes.get())[(written + offset) % BACKLOG_BYTES] = byte;
                }
            }
            self.written.store(written + line.len(), Ordering::Release);
        }
        self.writing.store(false, Ordering::Release);
    }
}

/// Copy the backlog's waiting bytes (whole lines, each ending in a line feed)
/// into `out`; returns how many. One caller at a time.
pub fn drain(out: &mut [u8]) -> usize {
    let read = BACKLOG.read.load(Ordering::Relaxed);
    let count = (BACKLOG.written.load(Ordering::Acquire) - read).min(out.len());
    for (offset, byte) in out[..count].iter_mut().enumerate() {
        // SAFETY: bytes before `written` are complete, and writers leave them
        // alone until `read` passes them.
        *byte = unsafe { (*BACKLOG.bytes.get())[(read + offset) % BACKLOG_BYTES] };
    }
    BACKLOG.read.store(read + count, Ordering::Release);
    count
}

/// One line on the semihosting console, and on the log channel or else in the
/// backlog, for a component without a serial channel.
pub fn note(arguments: fmt::Arguments<'_>) {
    semi::println!("{arguments}");
    if liblog_client::send(Some(Level::Info), arguments) == Sent::NoChannel {
        keep(arguments);
    }
}

fn keep(arguments: fmt::Arguments<'_>) {
    let mut line = Line {
        bytes: [0; LINE_BYTES + 2],
        len: 0,
    };
    // Writing to a `Line` cannot fail.
    let _written = line.write_fmt(arguments);
    line.bytes[line.len] = b'\n';
    BACKLOG.push(&line.bytes[..=line.len]);
}

/// Where the component's progress lines go.
pub struct Trace {
    serial: Option<SerialLog>,
}

impl Trace {
    pub fn new(init: &DriverInit) -> Self {
        Self {
            serial: SerialLog::new(init),
        }
    }

    /// Emit one line.
    pub fn line(&mut self, arguments: fmt::Arguments<'_>) {
        semi::println!("{arguments}");
        if liblog_client::send(Some(Level::Info), arguments) != Sent::NoChannel {
            return;
        }
        let Some(serial) = &mut self.serial else {
            keep(arguments);
            return;
        };
        {
            let mut line = Line {
                bytes: [0; LINE_BYTES + 2],
                len: 0,
            };
            // Writing to a `Line` cannot fail.
            let _written = line.write_fmt(arguments);
            line.bytes[line.len] = b'\r';
            line.bytes[line.len + 1] = b'\n';
            serial.send(&line.bytes[..line.len + 2]);
        }
    }
}

/// `trace!(trace, "text {value}")`: emit one line.
#[macro_export]
macro_rules! trace {
    ($trace:expr, $($argument:tt)*) => {
        $trace.line(format_args!($($argument)*))
    };
}

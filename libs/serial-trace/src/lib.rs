#![no_std]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! Progress lines for the people bringing a program up: on the serial channel
//! when the component has one (`serial = true` in its manifest entry), and on
//! the semihosting console under QEMU.

use {
    core::fmt::{self, Write},
    libdevice_driver::DriverInit,
    libio_channel::{Consumer, IoRec, Packet, Producer},
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
        if let Some(serial) = &mut self.serial {
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

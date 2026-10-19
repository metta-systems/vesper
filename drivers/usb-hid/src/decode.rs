//! Boot reports to input events.

use {
    vesper_input_event::{
        record::{EVENT_SIZE, Event, KEY_DOWN, KEY_UP, Kind},
        usage::{code, generic_desktop, keyboard, page},
    },
    vesper_usb_protocol::{
        enumeration::Speed,
        hid_boot::{
            BUTTON_LEFT, BUTTON_MIDDLE, BUTTON_RIGHT, KeyChange, KeyboardReport, MouseReport,
            ReportError, for_each_change,
        },
    },
};

/// Events one frame holds: a keyboard report changes at most eight modifiers
/// and six keys released and six pressed, and the frame ends with a `Sync`.
pub const MAX_FRAME: usize = 24;

/// Bytes of a frame in a buffer.
pub const MAX_FRAME_BYTES: usize = MAX_FRAME * EVENT_SIZE;

/// The events of one report, closed by a `Sync`.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    events: [Event; MAX_FRAME],
    len: usize,
}

impl Frame {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            events: [Event::sync(0, 0); MAX_FRAME],
            len: 0,
        }
    }

    /// Forget the events.
    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Add `event`. Returns `false` when the frame is full.
    pub fn push(&mut self, event: Event) -> bool {
        match self.events.get_mut(self.len) {
            Some(slot) => {
                *slot = event;
                self.len += 1;
                true
            }
            None => false,
        }
    }

    /// The events so far.
    #[must_use]
    pub fn events(&self) -> &[Event] {
        &self.events[..self.len]
    }

    /// Whether the frame holds nothing.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Write the records one after the other into `out`; returns the bytes
    /// written, or `None` if they do not fit.
    #[must_use]
    pub fn write_to(&self, out: &mut [u8]) -> Option<usize> {
        let total = self.len * EVENT_SIZE;
        let target = out.get_mut(..total)?;
        for (index, event) in self.events().iter().enumerate() {
            target[index * EVENT_SIZE..(index + 1) * EVENT_SIZE].copy_from_slice(&event.to_bytes());
        }
        Some(total)
    }
}

impl Default for Frame {
    fn default() -> Self {
        Self::new()
    }
}

/// The usage a boot keyboard report's modifier bit stands for.
const fn modifier_usage(mask: u8) -> u16 {
    // A single-bit mask: the bit number is below 8.
    keyboard::FIRST_MODIFIER + (mask.trailing_zeros() & 7) as u16
}

/// Follows a boot keyboard's reports and reports what changed.
#[derive(Clone, Copy, Debug)]
pub struct KeyboardDecoder {
    previous: KeyboardReport,
}

impl KeyboardDecoder {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            previous: KeyboardReport {
                modifiers: 0,
                keys: [0; 6],
            },
        }
    }

    /// Decode `report` into `frame`: a `Key` event for every modifier and key
    /// that changed, then a `Sync`. A report that changes nothing adds nothing.
    ///
    /// # Errors
    ///
    /// [`ReportError`] for a report of the wrong length or one that reports
    /// roll-over; the state stays what it was.
    pub fn decode(
        &mut self,
        report: &[u8],
        timestamp: u64,
        subject: u32,
        frame: &mut Frame,
    ) -> Result<(), ReportError> {
        let current = KeyboardReport::parse(report)?;
        let start = frame.len;
        for_each_change(&self.previous, &current, |change| {
            let (usage, pressed) = match change {
                KeyChange::Key { usage, pressed } => (u16::from(usage), pressed),
                KeyChange::Modifier { mask, pressed } => (modifier_usage(mask), pressed),
            };
            let value = if pressed { KEY_DOWN } else { KEY_UP };
            frame.push(Event::new(
                timestamp,
                subject,
                Kind::Key,
                code(page::KEYBOARD, usage),
                value,
            ));
        });
        if frame.len > start {
            frame.push(Event::sync(timestamp, subject));
        }
        self.previous = current;
        Ok(())
    }
}

impl Default for KeyboardDecoder {
    fn default() -> Self {
        Self::new()
    }
}

/// The button page usage of a boot mouse button bit.
const BUTTONS: [(u8, u16); 3] = [(BUTTON_LEFT, 1), (BUTTON_RIGHT, 2), (BUTTON_MIDDLE, 3)];

/// Follows a boot mouse's reports and reports what changed.
#[derive(Clone, Copy, Debug)]
pub struct MouseDecoder {
    buttons: u8,
}

impl MouseDecoder {
    #[must_use]
    pub const fn new() -> Self {
        Self { buttons: 0 }
    }

    /// Decode `report` into `frame`: a `Key` event for every button that
    /// changed, a `Rel` event for every axis that moved, then a `Sync`. A
    /// report that changes nothing adds nothing.
    ///
    /// # Errors
    ///
    /// [`ReportError::WrongLength`].
    pub fn decode(
        &mut self,
        report: &[u8],
        timestamp: u64,
        subject: u32,
        frame: &mut Frame,
    ) -> Result<(), ReportError> {
        let current = MouseReport::parse(report)?;
        let start = frame.len;
        for (mask, usage) in BUTTONS {
            if (self.buttons ^ current.buttons) & mask != 0 {
                let value = if current.buttons & mask != 0 {
                    KEY_DOWN
                } else {
                    KEY_UP
                };
                frame.push(Event::new(
                    timestamp,
                    subject,
                    Kind::Key,
                    code(page::BUTTON, usage),
                    value,
                ));
            }
        }
        for (usage, delta) in [
            (generic_desktop::X, current.dx),
            (generic_desktop::Y, current.dy),
            (generic_desktop::WHEEL, current.wheel),
        ] {
            if delta != 0 {
                frame.push(Event::new(
                    timestamp,
                    subject,
                    Kind::Rel,
                    code(page::GENERIC_DESKTOP, usage),
                    i32::from(delta),
                ));
            }
        }
        if frame.len > start {
            frame.push(Event::sync(timestamp, subject));
        }
        self.buttons = current.buttons;
        Ok(())
    }
}

impl Default for MouseDecoder {
    fn default() -> Self {
        Self::new()
    }
}

/// The polling interval in milliseconds an endpoint's `bInterval` asks for:
/// frames for a low- or full-speed device, `2^(bInterval - 1)` microframes for
/// a high-speed one, at least a millisecond and at most what the controller
/// polls.
#[must_use]
pub fn interval_ms(speed: Speed, b_interval: u8) -> u32 {
    let milliseconds = match speed {
        Speed::Low | Speed::Full => u32::from(b_interval),
        Speed::High => {
            let microframes = 1_u32 << u32::from(b_interval.clamp(1, 16) - 1);
            microframes.div_ceil(8)
        }
    };
    milliseconds.clamp(1, 255)
}

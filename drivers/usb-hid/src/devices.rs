//! The input devices the component serves: what each reports, its descriptor
//! and its state, found by the handle of the pipe its reports arrive on.

use {
    crate::decode::{Frame, KeyboardDecoder, MouseDecoder},
    vesper_input_event::{
        descriptor::{AxisInfo, Descriptor, KeyRange, Unit, bus},
        record::{Event, KEY_DOWN, Kind},
        state::State,
        usage::{code, generic_desktop, keyboard, page},
    },
    vesper_usb_protocol::hid_boot::ReportError,
};

/// Devices served at once: a keyboard and a mouse, with room for a second pair.
pub const MAX_DEVICES: usize = 4;

/// What a device is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    Keyboard,
    Mouse,
}

#[derive(Clone, Copy, Debug)]
enum Decoder {
    Keyboard(KeyboardDecoder),
    Mouse(MouseDecoder),
}

/// One served device.
#[derive(Clone, Copy, Debug)]
pub struct Device {
    subject: u32,
    kind: DeviceKind,
    pipe: u32,
    descriptor: Descriptor,
    state: State,
    decoder: Decoder,
}

impl Device {
    /// The device's id in the channel: its `subject`.
    #[must_use]
    pub const fn subject(&self) -> u32 {
        self.subject
    }

    #[must_use]
    pub const fn kind(&self) -> DeviceKind {
        self.kind
    }

    /// The handle of the pipe the device's reports arrive on.
    #[must_use]
    pub const fn pipe(&self) -> u32 {
        self.pipe
    }

    #[must_use]
    pub const fn descriptor(&self) -> &Descriptor {
        &self.descriptor
    }

    #[must_use]
    pub const fn state(&self) -> &State {
        &self.state
    }
}

/// The descriptor of a boot keyboard.
#[must_use]
pub fn keyboard_descriptor(vendor: u16, product: u16) -> Descriptor {
    let mut descriptor = Descriptor::new(bus::USB, vendor, product, 0);
    descriptor.set_name("USB boot keyboard");
    // The keys of the usage table, modifiers included. One range fits.
    let _pushed = descriptor.push_key_range(KeyRange {
        page: page::KEYBOARD,
        first: 0x04,
        last: keyboard::LAST_MODIFIER,
    });
    descriptor
}

/// The descriptor of a boot mouse: three buttons and the X, Y and wheel
/// movement.
#[must_use]
pub fn mouse_descriptor(vendor: u16, product: u16) -> Descriptor {
    let mut descriptor = Descriptor::new(bus::USB, vendor, product, 0);
    descriptor.set_name("USB boot mouse");
    let _pushed = descriptor.push_key_range(KeyRange {
        page: page::BUTTON,
        first: 1,
        last: 3,
    });
    for usage in [
        generic_desktop::X,
        generic_desktop::Y,
        generic_desktop::WHEEL,
    ] {
        // A boot report moves an axis by one signed byte.
        let _pushed = descriptor.push_axis(AxisInfo {
            code: code(page::GENERIC_DESKTOP, usage),
            min: -127,
            max: 127,
            resolution: 0,
            unit: Unit::None,
        });
    }
    descriptor
}

/// The served devices.
#[derive(Clone, Copy, Debug)]
pub struct Devices {
    slots: [Option<Device>; MAX_DEVICES],
    /// The id the next device gets; ids start at 1 and are never reused.
    next_subject: u32,
}

impl Devices {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [None; MAX_DEVICES],
            next_subject: 1,
        }
    }

    /// Serve a device whose reports arrive on `pipe`; returns its id, or
    /// `None` when every slot is taken.
    pub fn add(&mut self, kind: DeviceKind, pipe: u32, descriptor: Descriptor) -> Option<u32> {
        let slot = self.slots.iter_mut().find(|slot| slot.is_none())?;
        let subject = self.next_subject;
        self.next_subject += 1;
        *slot = Some(Device {
            subject,
            kind,
            pipe,
            descriptor,
            state: State::new(),
            decoder: match kind {
                DeviceKind::Keyboard => Decoder::Keyboard(KeyboardDecoder::new()),
                DeviceKind::Mouse => Decoder::Mouse(MouseDecoder::new()),
            },
        });
        Some(subject)
    }

    /// Stop serving the device `subject`.
    pub fn remove(&mut self, subject: u32) {
        for slot in &mut self.slots {
            if slot.is_some_and(|device| device.subject == subject) {
                *slot = None;
            }
        }
    }

    /// The device with id `subject`.
    #[must_use]
    pub fn get(&self, subject: u32) -> Option<&Device> {
        self.slots
            .iter()
            .flatten()
            .find(|device| device.subject == subject)
    }

    /// Decode a report that arrived on `pipe` into `frame` and fold the
    /// events into the device's state. Returns the device's id, or `None` for
    /// a pipe no device owns.
    ///
    /// # Errors
    ///
    /// [`ReportError`] for a malformed report; nothing changes.
    pub fn report(
        &mut self,
        pipe: u32,
        report: &[u8],
        timestamp: u64,
        frame: &mut Frame,
    ) -> Result<Option<u32>, ReportError> {
        let Some(device) = self
            .slots
            .iter_mut()
            .flatten()
            .find(|device| device.pipe == pipe)
        else {
            return Ok(None);
        };
        let start = frame.events().len();
        match &mut device.decoder {
            Decoder::Keyboard(decoder) => {
                decoder.decode(report, timestamp, device.subject, frame)?;
            }
            Decoder::Mouse(decoder) => {
                decoder.decode(report, timestamp, device.subject, frame)?;
            }
        }
        for event in &frame.events()[start..] {
            // Keys and buttons fit the tracker; a full tracker cannot happen
            // with six keys and eight modifiers.
            let _tracked = device.state.apply(event);
        }
        Ok(Some(device.subject))
    }

    /// The events that rebuild the state of the device `subject`: a `Key` down
    /// for every key and button held, an `Abs` for every axis seen, then a
    /// `Sync`. `None` when there is no such device or `frame` is too small.
    pub fn snapshot(&self, subject: u32, timestamp: u64, frame: &mut Frame) -> Option<()> {
        let device = self.get(subject)?;
        let mut fits = true;
        device.state.for_each_down(|held| {
            fits &= frame.push(Event::new(timestamp, subject, Kind::Key, held, KEY_DOWN));
        });
        device.state.for_each_axis(|axis, value| {
            fits &= frame.push(Event::new(timestamp, subject, Kind::Abs, axis, value));
        });
        fits &= frame.push(Event::sync(timestamp, subject));
        fits.then_some(())
    }
}

impl Default for Devices {
    fn default() -> Self {
        Self::new()
    }
}

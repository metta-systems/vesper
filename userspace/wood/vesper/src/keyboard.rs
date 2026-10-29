//! Wood's keyboard: the keys the compositor routes to Wood's window, as Doom
//! key events.
//!
//! The compositor sends the events of Wood's window on the io-channel of
//! `compositor_v1` (see `libcompositor_core::events`). The engine asks for keys
//! once a tick, through [`Platform::key`](room::engine::platform::Platform::key),
//! so the channel is only looked at then, and never waited on: the game loop
//! keeps its own pace. A keyboard key (HID usage page 7) becomes the Doom key
//! the host port gives the same key (`room`'s `platform/host/keys.rs`); keys
//! without one are ignored. Wood follows which keys are held, so that losing
//! the focus, or records the compositor dropped, release them: the engine
//! would otherwise keep a key down for good. After `Dropped` the compositor's
//! `snapshot` says which keys are still held.

use {
    crate::window::Window,
    alloc::collections::VecDeque,
    compositor_client::compositor_v1::Client as CompositorClient,
    core::cell::{Cell, RefCell},
    libcompositor_core::{
        events::{EventChannel, Receiver, data_area},
        router::MAX_ROUTED,
    },
    libobject::{EventCountKey, RawKey},
    room::engine::interface::keys::{
        KEY_BACKSPACE, KEY_DOWNARROW, KEY_ENTER, KEY_EQUALS, KEY_ESCAPE, KEY_F1, KEY_F2, KEY_F3,
        KEY_F4, KEY_F5, KEY_F6, KEY_F7, KEY_F8, KEY_F9, KEY_F10, KEY_F11, KEY_F12, KEY_FIRE,
        KEY_LALT, KEY_LEFTARROW, KEY_MINUS, KEY_PAUSE, KEY_RIGHTARROW, KEY_RSHIFT, KEY_TAB,
        KEY_UPARROW, KEY_USE,
    },
    vesper_input_event::{
        record::{Event, KEY_DOWN, KEY_UP, Kind},
        usage::{keyboard, page, page_of, usage_of},
    },
};

/// Key events traced at INFO, so a log shows that keys arrive without
/// growing with every key.
const KEYS_TRACED: u32 = 8;

/// Keyboard page usages (HID Usage Tables 1.5, section 10) that the host port
/// maps too.
mod usage {
    pub const A: u16 = 0x04;
    pub const Z: u16 = 0x1D;
    pub const DIGIT_1: u16 = 0x1E;
    pub const DIGIT_9: u16 = 0x26;
    pub const DIGIT_0: u16 = 0x27;
    pub const ENTER: u16 = 0x28;
    pub const ESCAPE: u16 = 0x29;
    pub const BACKSPACE: u16 = 0x2A;
    pub const TAB: u16 = 0x2B;
    pub const SPACE: u16 = 0x2C;
    pub const MINUS: u16 = 0x2D;
    pub const EQUAL: u16 = 0x2E;
    pub const F1: u16 = 0x3A;
    pub const F12: u16 = 0x45;
    pub const PAUSE: u16 = 0x48;
    pub const RIGHT_ARROW: u16 = 0x4F;
    pub const LEFT_ARROW: u16 = 0x50;
    pub const DOWN_ARROW: u16 = 0x51;
    pub const UP_ARROW: u16 = 0x52;
}

/// The Doom key of keyboard usage `key`, as the host port maps the same
/// physical key: arrows, Ctrl fires, Space uses, Shift runs, Alt strafes.
fn doom_key(key: u16) -> Option<u8> {
    const F_KEYS: [u8; 12] = [
        KEY_F1, KEY_F2, KEY_F3, KEY_F4, KEY_F5, KEY_F6, KEY_F7, KEY_F8, KEY_F9, KEY_F10, KEY_F11,
        KEY_F12,
    ];
    let offset = |first: u16| u8::try_from(key - first).ok();
    let doom = match key {
        usage::A..=usage::Z => b'a' + offset(usage::A)?,
        usage::DIGIT_1..=usage::DIGIT_9 => b'1' + offset(usage::DIGIT_1)?,
        usage::DIGIT_0 => b'0',
        usage::ENTER => KEY_ENTER,
        usage::ESCAPE => KEY_ESCAPE,
        usage::BACKSPACE => KEY_BACKSPACE,
        usage::TAB => KEY_TAB,
        usage::SPACE => KEY_USE,
        usage::MINUS => KEY_MINUS,
        usage::EQUAL => KEY_EQUALS,
        usage::F1..=usage::F12 => F_KEYS[usize::from(key - usage::F1)],
        usage::PAUSE => KEY_PAUSE,
        usage::RIGHT_ARROW => KEY_RIGHTARROW,
        usage::LEFT_ARROW => KEY_LEFTARROW,
        usage::DOWN_ARROW => KEY_DOWNARROW,
        usage::UP_ARROW => KEY_UPARROW,
        keyboard::LEFT_CTRL | keyboard::RIGHT_CTRL => KEY_FIRE,
        keyboard::LEFT_SHIFT | keyboard::RIGHT_SHIFT => KEY_RSHIFT,
        keyboard::LEFT_ALT | keyboard::RIGHT_ALT => KEY_LALT,
        _ => return None,
    };
    Some(doom)
}

/// The keys of Wood's window.
pub struct Keyboard {
    /// The client end of the event channel, if the compositor gave one.
    receiver: Option<RefCell<Receiver<'static, EventCountKey>>>,
    /// Doom key events not yet handed to the engine.
    pending: RefCell<VecDeque<(bool, u8)>>,
    /// The keyboard usages held, by usage.
    held: RefCell<[bool; 256]>,
    /// Key events traced so far.
    traced: Cell<u32>,
}

impl Keyboard {
    /// Post the buffers of the event channel of `compositor`, before Wood
    /// asks for its window, so the focus the new window gets arrives.
    pub fn open(compositor: &CompositorClient) -> Self {
        let receiver = compositor
            .channel()
            .filter(|area| area.event != 0)
            .map(|area| {
                // SAFETY: Kickstart mapped the channel's control page and data
                // page read/write for Wood's lifetime.
                let channel = unsafe { EventChannel::at(area.control) };
                let completed = EventCountKey::from_key(RawKey::from_wire(area.event));
                // SAFETY: as above, the data page stays mapped.
                RefCell::new(unsafe { Receiver::new(channel, data_area(area.control), completed) })
            });
        if receiver.is_none() {
            log::warn!("no event channel from the compositor: no keys");
        }
        Self {
            receiver,
            pending: RefCell::new(VecDeque::new()),
            held: RefCell::new([false; 256]),
            traced: Cell::new(0),
        }
    }

    /// The next key event for the engine, as `(pressed, doom_key)`.
    pub fn next_key(&self, window: &Window) -> Option<(bool, u8)> {
        let receiver = self.receiver.as_ref()?;
        loop {
            if let Some(key) = self.pending.borrow_mut().pop_front() {
                return Some(key);
            }
            let event = receiver.borrow_mut().next_event()?;
            self.handle(&event, window);
        }
    }

    fn handle(&self, event: &Event, window: &Window) {
        match event.kind {
            Kind::Key if page_of(event.code) == page::KEYBOARD => {
                self.key(usage_of(event.code), event.value, event.timestamp);
            }
            Kind::Focus => {
                log::info!("focus {}", if event.value == 0 { "lost" } else { "gained" });
                if event.value == 0 {
                    self.release_all();
                }
            }
            Kind::Dropped => {
                log::warn!("the compositor dropped events: resyncing");
                self.release_all();
                let mut records = [Event::sync(0, 0); MAX_ROUTED];
                let count = window.snapshot(&mut records);
                for record in &records[..count] {
                    if record.kind != Kind::Dropped {
                        self.handle(record, window);
                    }
                }
            }
            _ => {}
        }
    }

    /// Follow keyboard usage `key` going to `value`, and queue its Doom key
    /// when it changed. `stamp` is when the USB HID component decoded it (0:
    /// a release Wood or the compositor made up), for the trace.
    fn key(&self, key: u16, value: i32, stamp: u64) {
        let pressed = match value {
            KEY_DOWN => true,
            KEY_UP => false,
            _ => return,
        };
        let changed = self
            .held
            .borrow_mut()
            .get_mut(usize::from(key))
            .is_some_and(|held| {
                let changed = *held != pressed;
                *held = pressed;
                changed
            });
        if !changed {
            return;
        }
        let Some(doom) = doom_key(key) else {
            return;
        };
        let traced = self.traced.get();
        if traced < KEYS_TRACED {
            self.traced.set(traced + 1);
            let state = if pressed { "down" } else { "up" };
            if stamp == 0 {
                log::info!("key {key:#04x} {state}: doom key {doom:#04x}");
            } else {
                // The engine reads keys once a tic: this is when it got the key.
                let age_us = libuser::clock::now_ns().saturating_sub(stamp) / 1000;
                log::info!(
                    "key {key:#04x} {state}: doom key {doom:#04x}, {age_us} us after usb-hid"
                );
            }
        }
        self.pending.borrow_mut().push_back((pressed, doom));
    }

    /// Release every key held.
    fn release_all(&self) {
        let held: [bool; 256] = *self.held.borrow();
        for (key, _) in held.iter().enumerate().filter(|(_, held)| **held) {
            if let Ok(key) = u16::try_from(key) {
                self.key(key, KEY_UP, 0);
            }
        }
    }
}

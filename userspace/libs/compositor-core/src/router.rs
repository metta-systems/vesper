//! The input router: turns the frames of an input producer into events for
//! windows.
//!
//! The producer's records are folded one frame at a time (a frame ends with a
//! `Sync`). Pointer motion moves the pointer, which stays on the screen. The
//! first button pressed focuses the window under the pointer (or none) and
//! raises it, and that window holds an implicit grab: every pointer event goes
//! to it until the last button is released. A new window gets the focus when
//! the compositor gives it one ([`Router::focus_window`]). Keys go to the
//! focused window.
//! What a window receives is rewritten for it: its id in `subject`, the pointer
//! in its own coordinates, and `Enter`, `Leave` and `Focus` records when the
//! pointer or the focus moves. Each window's events of a frame end with a
//! `Sync` of their own.

use {
    crate::windows::Windows,
    vesper_input_event::{
        record::{Event, KEY_DOWN, KEY_UP, Kind},
        state::State,
        usage::{code, generic_desktop, page, page_of, usage_of},
    },
};

/// Records of one producer frame the router keeps.
pub const MAX_FRAME_EVENTS: usize = 32;

/// Events one frame can send to windows, `Sync` records included.
pub const MAX_ROUTED: usize = 64;

/// Input devices the router follows.
pub const MAX_DEVICES: usize = 8;

/// An event for a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Routed {
    pub window: u32,
    pub event: Event,
}

/// What the router sends to windows for one producer frame.
#[derive(Clone, Copy, Debug)]
pub struct Output {
    items: [Routed; MAX_ROUTED],
    len: usize,
}

impl Output {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            items: [Routed {
                window: 0,
                event: Event::sync(0, 0),
            }; MAX_ROUTED],
            len: 0,
        }
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// The events, each window's together and ending with its `Sync`.
    #[must_use]
    pub fn items(&self) -> &[Routed] {
        &self.items[..self.len]
    }

    fn push(&mut self, routed: Routed) -> bool {
        match self.items.get_mut(self.len) {
            Some(slot) => {
                *slot = routed;
                self.len += 1;
                true
            }
            None => false,
        }
    }
}

impl Default for Output {
    fn default() -> Self {
        Self::new()
    }
}

/// What the router counted that went nowhere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    /// Key events with no window to take them.
    pub unrouted: u32,
    /// Records past what a frame holds, and events past what [`Output`] holds.
    pub overflowed: u32,
}

/// The router and the state it follows.
#[derive(Clone, Copy, Debug)]
pub struct Router {
    width: u32,
    height: u32,
    windows: Windows,
    x: i32,
    y: i32,
    /// Bit `n` is the button with usage `n + 1`.
    buttons: u32,
    focus: Option<u32>,
    grab: Option<u32>,
    hover: Option<u32>,
    devices: [Option<(u32, State)>; MAX_DEVICES],
    frame: [Event; MAX_FRAME_EVENTS],
    frame_len: usize,
    stale: bool,
    counters: Counters,
}

impl Router {
    /// A router for a screen of `width` by `height` pixels, the pointer in its
    /// middle.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            windows: Windows::new(),
            x: i32::try_from(width / 2).unwrap_or(0),
            y: i32::try_from(height / 2).unwrap_or(0),
            buttons: 0,
            focus: None,
            grab: None,
            hover: None,
            devices: [None; MAX_DEVICES],
            frame: [Event::sync(0, 0); MAX_FRAME_EVENTS],
            frame_len: 0,
            stale: false,
            counters: Counters {
                unrouted: 0,
                overflowed: 0,
            },
        }
    }

    /// The windows, for painting and hit-testing.
    #[must_use]
    pub const fn windows(&self) -> &Windows {
        &self.windows
    }

    /// The windows, to add, move, raise and remove them. A window removed here
    /// must also be given to [`forget_window`](Self::forget_window).
    pub const fn windows_mut(&mut self) -> &mut Windows {
        &mut self.windows
    }

    /// What went nowhere.
    #[must_use]
    pub const fn counters(&self) -> Counters {
        self.counters
    }

    /// Where the pointer is.
    #[must_use]
    pub const fn pointer(&self) -> (i32, i32) {
        (self.x, self.y)
    }

    /// The window with keyboard focus.
    #[must_use]
    pub const fn focus(&self) -> Option<u32> {
        self.focus
    }

    /// The window the pointer's events go to: the grab, else the window under it.
    #[must_use]
    pub const fn hover(&self) -> Option<u32> {
        self.hover
    }

    /// Whether the producer dropped records since the router last resynchronised.
    #[must_use]
    pub const fn is_stale(&self) -> bool {
        self.stale
    }

    /// Forget a window that went away; nothing is sent to it.
    pub fn forget_window(&mut self, id: u32) {
        self.windows.remove(id);
        for slot in [&mut self.focus, &mut self.grab, &mut self.hover] {
            if *slot == Some(id) {
                *slot = None;
            }
        }
    }

    /// Give keyboard focus to window `id`, as a new window gets it: the window
    /// that had it receives a `Focus` with value 0 and `id` one with value 1,
    /// each closed by a `Sync`, added to `out`. Nothing is sent when `id`
    /// already has focus or is not one of the windows.
    pub fn focus_window(&mut self, id: u32, stamp: u64, out: &mut Output) {
        if self.windows.get(id).is_none() {
            return;
        }
        let mut sent = Sent::new();
        self.set_focus(Some(id), stamp, &mut sent);
        self.flush(&sent, stamp, out);
    }

    /// Write into `into` the records that rebuild what the windows of
    /// `client` were sent, for a client that lost records: for the window
    /// with focus a `Focus` with value 1 and a `Key` down for every key held,
    /// for the window the pointer's events go to an `Enter`, the pointer's
    /// position in it and a `Key` down for every button held, each window's
    /// records closed by a `Sync`. Returns how many were written; records past
    /// `into` are left out.
    pub fn snapshot(&self, client: u32, stamp: u64, into: &mut [Event]) -> usize {
        let owned = |id: Option<u32>| {
            id.filter(|&id| {
                self.windows
                    .get(id)
                    .is_some_and(|window| window.client == client)
            })
        };
        let mut sent = Sent::new();
        if let Some(focused) = owned(self.focus) {
            sent.push(focused, Event::new(stamp, focused, Kind::Focus, 0, 1));
            for (_, state) in self.devices.iter().flatten() {
                state.for_each_down(|held| {
                    if page_of(held) != page::BUTTON {
                        sent.push(
                            focused,
                            Event::new(stamp, focused, Kind::Key, held, KEY_DOWN),
                        );
                    }
                });
            }
        }
        if let Some(hovered) = owned(self.hover) {
            sent.push(hovered, Event::new(stamp, hovered, Kind::Enter, 0, 0));
            self.send_position(hovered, stamp, &mut sent);
            for bit in 0..32_u16 {
                if self.buttons & (1 << bit) != 0 {
                    sent.push(
                        hovered,
                        Event::new(
                            stamp,
                            hovered,
                            Kind::Key,
                            code(page::BUTTON, bit + 1),
                            KEY_DOWN,
                        ),
                    );
                }
            }
        }
        let mut out = Output::new();
        let _overflowed = group(&sent, stamp, &mut out);
        let mut count = 0;
        for (slot, routed) in into.iter_mut().zip(out.items()) {
            *slot = routed.event;
            count += 1;
        }
        count
    }

    /// Fold one record of the producer into the router. A `Sync` ends the
    /// frame: what the windows are to receive is added to `out`.
    pub fn feed(&mut self, event: &Event, out: &mut Output) {
        match event.kind {
            Kind::Sync => self.finish_frame(out),
            Kind::Dropped => self.stale = true,
            Kind::DeviceAdded => {
                if !self.devices.iter().flatten().any(|d| d.0 == event.subject)
                    && let Some(slot) = self.devices.iter_mut().find(|slot| slot.is_none())
                {
                    *slot = Some((event.subject, State::new()));
                }
            }
            Kind::DeviceRemoved => self.release_device(event.subject),
            Kind::Key | Kind::Rel | Kind::Abs => self.queue(*event),
            // Records only a compositor sends.
            Kind::Focus | Kind::Enter | Kind::Leave | Kind::ResizeOffer | Kind::CloseRequest => {}
        }
    }

    /// Replace the state of device `subject` with the keys and axes of a
    /// snapshot (the records `input_v1.snapshot` returns) and clear the stale
    /// mark. Held buttons and keys that the snapshot no longer holds are
    /// released, and those it holds that were not held are pressed: the
    /// records lost in between may have pressed a key that is still down.
    pub fn resync(&mut self, subject: u32, snapshot: &[Event], out: &mut Output) {
        let mut state = State::new();
        for event in snapshot {
            let _tracked = state.apply(event);
        }
        let old = self
            .devices
            .iter()
            .flatten()
            .find(|d| d.0 == subject)
            .map(|d| d.1);
        let mut changed = [(0_u32, KEY_UP); 64];
        let mut count = 0;
        let mut push = |code, value| {
            if let Some(slot) = changed.get_mut(count) {
                *slot = (code, value);
                count += 1;
            }
        };
        // Releases first, as a report lists them.
        if let Some(old) = &old {
            old.for_each_down(|code| {
                if !state.is_down(code) {
                    push(code, KEY_UP);
                }
            });
        }
        state.for_each_down(|code| {
            if !old.as_ref().is_some_and(|old| old.is_down(code)) {
                push(code, KEY_DOWN);
            }
        });
        for &(code, value) in &changed[..count] {
            self.queue(Event::new(0, subject, Kind::Key, code, value));
        }
        self.finish_frame(out);
        if let Some(slot) = self.devices.iter_mut().flatten().find(|d| d.0 == subject) {
            slot.1 = state;
        } else if let Some(slot) = self.devices.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some((subject, state));
        }
        self.stale = false;
    }

    fn queue(&mut self, event: Event) {
        if let Some(slot) = self.frame.get_mut(self.frame_len) {
            *slot = event;
            self.frame_len += 1;
        } else {
            self.counters.overflowed += 1;
        }
    }

    /// Queue releases for every key and button device `subject` holds.
    fn release_device(&mut self, subject: u32) {
        let Some(at) = self
            .devices
            .iter()
            .position(|slot| slot.is_some_and(|d| d.0 == subject))
        else {
            return;
        };
        let Some((_, state)) = self.devices[at].take() else {
            return;
        };
        let mut codes = [0_u32; MAX_FRAME_EVENTS];
        let mut count = 0;
        state.for_each_down(|held| {
            if count < codes.len() {
                codes[count] = held;
                count += 1;
            }
        });
        for &released in &codes[..count] {
            self.queue(Event::new(0, subject, Kind::Key, released, KEY_UP));
        }
    }

    /// Follow the keys of the queued records in their device's state.
    fn track(&mut self, event: &Event) {
        if let Some((_, state)) = self
            .devices
            .iter_mut()
            .flatten()
            .find(|device| device.0 == event.subject)
        {
            let _tracked = state.apply(event);
        }
    }

    fn finish_frame(&mut self, out: &mut Output) {
        let frame = self.frame;
        let length = self.frame_len;
        self.frame_len = 0;
        let mut sent = Sent::new();
        let (mut dx, mut dy, mut wheel) = (0_i32, 0_i32, 0_i32);
        for event in &frame[..length] {
            self.track(event);
            if event.kind == Kind::Rel && page_of(event.code) == page::GENERIC_DESKTOP {
                match usage_of(event.code) {
                    generic_desktop::X => dx = dx.saturating_add(event.value),
                    generic_desktop::Y => dy = dy.saturating_add(event.value),
                    generic_desktop::WHEEL => wheel = wheel.saturating_add(event.value),
                    _ => {}
                }
            }
        }
        let stamp = frame[..length].last().map_or(0, |event| event.timestamp);
        let moved = dx != 0 || dy != 0;
        if moved {
            self.x = clamp(self.x, dx, self.width);
            self.y = clamp(self.y, dy, self.height);
        }
        // Where the pointer's events go now, before any button changes it.
        self.update_hover(stamp, &mut sent, moved);
        if wheel != 0
            && let Some(target) = self.hover
        {
            sent.push(
                target,
                Event::new(
                    stamp,
                    target,
                    Kind::Rel,
                    code(page::GENERIC_DESKTOP, generic_desktop::WHEEL),
                    wheel,
                ),
            );
        }
        for event in &frame[..length] {
            if event.kind != Kind::Key {
                continue;
            }
            if page_of(event.code) == page::BUTTON {
                self.button(event, &mut sent);
            } else {
                self.key(event, &mut sent);
            }
        }
        self.flush(&sent, stamp, out);
    }

    /// Recompute the window the pointer's events go to; on a change send
    /// `Leave` and `Enter`, and on a change or motion the pointer's position.
    fn update_hover(&mut self, stamp: u64, sent: &mut Sent, moved: bool) {
        let target = self.grab.or_else(|| self.windows.top_at(self.x, self.y));
        if target != self.hover {
            if let Some(old) = self.hover
                && self.windows.get(old).is_some()
            {
                sent.push(old, Event::new(stamp, old, Kind::Leave, 0, 0));
            }
            if let Some(new) = target {
                sent.push(new, Event::new(stamp, new, Kind::Enter, 0, 0));
            }
            self.hover = target;
        } else if !moved {
            return;
        }
        if let Some(target) = self.hover {
            self.send_position(target, stamp, sent);
        }
    }

    fn send_position(&self, target: u32, stamp: u64, sent: &mut Sent) {
        let Some(window) = self.windows.get(target) else {
            return;
        };
        for (usage, value) in [
            (generic_desktop::X, self.x - window.rect.x),
            (generic_desktop::Y, self.y - window.rect.y),
        ] {
            sent.push(
                target,
                Event::new(
                    stamp,
                    target,
                    Kind::Abs,
                    code(page::GENERIC_DESKTOP, usage),
                    value,
                ),
            );
        }
    }

    fn button(&mut self, event: &Event, sent: &mut Sent) {
        let bit = u32::from(usage_of(event.code)).saturating_sub(1);
        if bit >= 32 {
            return;
        }
        let mask = 1_u32 << bit;
        let stamp = event.timestamp;
        if event.value == KEY_DOWN {
            if self.buttons & mask != 0 {
                return;
            }
            if self.buttons == 0 {
                // The first button focuses and grabs.
                let target = self.windows.top_at(self.x, self.y);
                self.set_focus(target, stamp, sent);
                self.grab = target;
                if let Some(target) = target {
                    self.windows.raise(target);
                }
            }
            self.buttons |= mask;
        } else if event.value == KEY_UP {
            if self.buttons & mask == 0 {
                return;
            }
            self.buttons &= !mask;
        } else {
            return;
        }
        let target = self.grab.or_else(|| self.windows.top_at(self.x, self.y));
        if let Some(target) = target {
            sent.push(
                target,
                Event::new(stamp, target, Kind::Key, event.code, event.value),
            );
        }
        if self.buttons == 0 && self.grab.take().is_some() {
            // The grab ends: the pointer's events go to whatever is under it.
            self.update_hover(stamp, sent, false);
        }
    }

    fn key(&mut self, event: &Event, sent: &mut Sent) {
        match self.focus {
            Some(target) => sent.push(
                target,
                Event::new(event.timestamp, target, Kind::Key, event.code, event.value),
            ),
            None => self.counters.unrouted += 1,
        }
    }

    fn set_focus(&mut self, target: Option<u32>, stamp: u64, sent: &mut Sent) {
        if target == self.focus {
            return;
        }
        if let Some(old) = self.focus
            && self.windows.get(old).is_some()
        {
            sent.push(old, Event::new(stamp, old, Kind::Focus, 0, 0));
        }
        if let Some(new) = target {
            sent.push(new, Event::new(stamp, new, Kind::Focus, 0, 1));
        }
        self.focus = target;
    }

    /// Write what was sent to `out`, each window's events together and ending
    /// with a `Sync`.
    fn flush(&mut self, sent: &Sent, stamp: u64, out: &mut Output) {
        self.counters.overflowed += group(sent, stamp, out);
    }
}

/// Write `sent` to `out`, each window's events together and ending with a
/// `Sync`. Returns how many events did not fit.
fn group(sent: &Sent, stamp: u64, out: &mut Output) -> u32 {
    let mut overflowed = 0;
    let mut done = [0_u32; MAX_ROUTED];
    let mut windows = 0;
    for first in sent.items() {
        if done[..windows].contains(&first.window) {
            continue;
        }
        done[windows] = first.window;
        windows += 1;
        for routed in sent.items().iter().filter(|r| r.window == first.window) {
            if !out.push(*routed) {
                overflowed += 1;
            }
        }
        let closing = Routed {
            window: first.window,
            event: Event::sync(stamp, first.window),
        };
        if !out.push(closing) {
            overflowed += 1;
        }
    }
    overflowed
}

/// `value + delta`, kept inside `0..size`.
fn clamp(value: i32, delta: i32, size: u32) -> i32 {
    let last = i32::try_from(size).unwrap_or(i32::MAX).saturating_sub(1);
    value.saturating_add(delta).clamp(0, last.max(0))
}

/// What a frame sends, before it is grouped by window.
struct Sent {
    items: [Routed; MAX_ROUTED],
    len: usize,
}

impl Sent {
    const fn new() -> Self {
        Self {
            items: [Routed {
                window: 0,
                event: Event::sync(0, 0),
            }; MAX_ROUTED],
            len: 0,
        }
    }

    fn push(&mut self, window: u32, event: Event) {
        if let Some(slot) = self.items.get_mut(self.len) {
            *slot = Routed { window, event };
            self.len += 1;
        }
    }

    fn items(&self) -> &[Routed] {
        &self.items[..self.len]
    }
}

//! Moving a window by dragging it with Alt held: what a client feeds its
//! window's events to, and asks where to `move_window` the window.
//!
//! A client keeps one [`Mover`] per window, made with the place it put the
//! window at (a client knows its window's place only if it placed the window
//! itself). Each event the window receives goes to [`Mover::feed`]. While Alt
//! is held, a left-button press starts a move; at the end of each producer
//! frame (`Sync`) during the move, `feed` returns the window's new place
//! whenever the pointer moved from where the press grabbed the window. The
//! client calls `move_window` with it. Releasing the button ends the move.
//!
//! The pointer reaches a window in the window's own coordinates, so during a
//! move the grabbed point stays under the pointer: the new place is the old
//! one plus how far the pointer is from the grabbed point. Records routed
//! before the compositor applied a move but read after it are measured from
//! the old place, so a client that falls behind lets the window lag a little
//! behind the pointer; the protocol that replaces this (the compositor moving
//! windows itself) will not.
//!
//! The press, motion and release of a move are the client's to ignore:
//! [`Mover::is_moving`] says whether the window is being moved, and
//! [`Mover::feed`] says whether the event belonged to a move.

use vesper_input_event::{
    record::{Event, KEY_DOWN, KEY_UP, Kind},
    usage::{code, generic_desktop, page},
};

/// The left button.
const LEFT_BUTTON: u32 = code(page::BUTTON, 1);
/// The left and right Alt keys (HID keyboard usages 0xE2 and 0xE6).
const LEFT_ALT: u32 = code(page::KEYBOARD, 0xe2);
const RIGHT_ALT: u32 = code(page::KEYBOARD, 0xe6);
/// The pointer's position in the window.
const POINTER_X: u32 = code(page::GENERIC_DESKTOP, generic_desktop::X);
const POINTER_Y: u32 = code(page::GENERIC_DESKTOP, generic_desktop::Y);

/// What one event meant to the [`Mover`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fed {
    /// Nothing to do with a move: the client handles it.
    Pass,
    /// Part of a move (its press, motion or release): the client ignores it.
    Moving,
    /// The end of a frame of a move: put the window's top left corner here.
    MoveTo(i32, i32),
}

/// One window's Alt-drag state.
#[derive(Clone, Copy, Debug)]
pub struct Mover {
    /// Where the window's top left corner is on the desktop.
    place: (i32, i32),
    /// Which Alt keys are held, as bits: 1 left, 2 right.
    alt: u8,
    /// The pointer's last position in the window.
    pointer: (i32, i32),
    /// During a move, the point of the window the press grabbed.
    grab: Option<(i32, i32)>,
}

impl Mover {
    /// A mover for a window whose top left corner the client put at `place`.
    #[must_use]
    pub const fn new(place: (i32, i32)) -> Self {
        Self {
            place,
            alt: 0,
            pointer: (0, 0),
            grab: None,
        }
    }

    /// Where the window's top left corner is.
    #[must_use]
    pub const fn place(&self) -> (i32, i32) {
        self.place
    }

    /// The client moved the window itself, to `place`.
    pub const fn moved_to(&mut self, place: (i32, i32)) {
        self.place = place;
    }

    /// Whether the window is being moved.
    #[must_use]
    pub const fn is_moving(&self) -> bool {
        self.grab.is_some()
    }

    /// Follow `event`, one the window received, and say what it meant.
    pub fn feed(&mut self, event: &Event) -> Fed {
        match event.kind {
            Kind::Focus if event.value == 0 => {
                // Keys held while the focus is elsewhere never reach the
                // window, nor their release.
                self.alt = 0;
                Fed::Pass
            }
            Kind::Abs if event.code == POINTER_X => {
                self.pointer.0 = event.value;
                self.during_move()
            }
            Kind::Abs if event.code == POINTER_Y => {
                self.pointer.1 = event.value;
                self.during_move()
            }
            Kind::Key if event.code == LEFT_ALT || event.code == RIGHT_ALT => {
                let bit = if event.code == LEFT_ALT { 1 } else { 2 };
                match event.value {
                    KEY_DOWN => self.alt |= bit,
                    KEY_UP => self.alt &= !bit,
                    _ => {}
                }
                Fed::Pass
            }
            Kind::Key if event.code == LEFT_BUTTON => match event.value {
                KEY_DOWN if self.alt != 0 => {
                    self.grab = Some(self.pointer);
                    Fed::Moving
                }
                KEY_UP if self.grab.is_some() => {
                    self.grab = None;
                    Fed::Moving
                }
                _ => self.during_move(),
            },
            Kind::Sync => self.step(),
            _ => self.during_move(),
        }
    }

    /// `Moving` during a move, `Pass` otherwise.
    const fn during_move(&self) -> Fed {
        if self.grab.is_some() {
            Fed::Moving
        } else {
            Fed::Pass
        }
    }

    /// At the end of a frame: during a move, the window's new place if the
    /// pointer left the grabbed point.
    const fn step(&mut self) -> Fed {
        let Some(grab) = self.grab else {
            return Fed::Pass;
        };
        let dx = self.pointer.0.saturating_sub(grab.0);
        let dy = self.pointer.1.saturating_sub(grab.1);
        if dx == 0 && dy == 0 {
            return Fed::Moving;
        }
        self.place = (
            self.place.0.saturating_add(dx),
            self.place.1.saturating_add(dy),
        );
        // After the move the grabbed point is under the pointer again.
        self.pointer = grab;
        Fed::MoveTo(self.place.0, self.place.1)
    }
}

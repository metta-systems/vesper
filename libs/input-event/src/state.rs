//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! The state of one device, folded from its events: which keys and buttons are
//! down and where its absolute axes are. A consumer keeps one [`State`] per
//! device id of a channel.
//!
//! Producers send only changes, so a lost record can leave a key stuck. A
//! `Dropped` record marks the state [stale](State::is_stale) until the consumer
//! [resynchronises](State::resync) it from a snapshot the producer serves.
//! `DeviceRemoved` releases everything.

use crate::{
    record::{Event, KEY_UP, Kind},
    usage::{self, page},
};

/// Held keys of pages without a bitmap (Consumer, Digitizer, usages past the
/// bitmaps) the tracker follows.
pub const MAX_OTHER_KEYS: usize = 16;
/// Absolute axes the tracker follows.
pub const MAX_AXES: usize = 16;

/// Why an event did not change the state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateError {
    /// The tracker has no room for another held key or axis. The state is
    /// unchanged.
    Full,
}

/// The keys, buttons and absolute axes of one device.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// Keyboard page usages 0 to 255, one bit each.
    keyboard: [u64; 4],
    /// Button page usages 1 to 64, bit `usage - 1`.
    buttons: u64,
    /// Other held codes, the first `other_len` of them.
    other: [u32; MAX_OTHER_KEYS],
    other_len: usize,
    /// Absolute axes as (code, value), the first `axes_len` of them.
    axes: [(u32, i32); MAX_AXES],
    axes_len: usize,
    stale: bool,
}

impl State {
    /// Nothing down, no axis seen.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            keyboard: [0; 4],
            buttons: 0,
            other: [0; MAX_OTHER_KEYS],
            other_len: 0,
            axes: [(0, 0); MAX_AXES],
            axes_len: 0,
            stale: false,
        }
    }

    /// Fold `event` into the state.
    ///
    /// `Key` presses (value 1 or 2) and releases, `Abs` positions, `Dropped`
    /// and `DeviceRemoved` change the state; every other kind leaves it alone.
    ///
    /// # Errors
    ///
    /// [`StateError::Full`] if a new held key or axis does not fit.
    pub fn apply(&mut self, event: &Event) -> Result<(), StateError> {
        match event.kind {
            Kind::Key => self.set_key(event.code, event.value != KEY_UP),
            Kind::Abs => self.set_axis(event.code, event.value),
            Kind::Dropped => {
                self.stale = true;
                Ok(())
            }
            Kind::DeviceRemoved => {
                *self = Self::new();
                Ok(())
            }
            Kind::Rel
            | Kind::Sync
            | Kind::DeviceAdded
            | Kind::Focus
            | Kind::Enter
            | Kind::Leave
            | Kind::ResizeOffer
            | Kind::CloseRequest => Ok(()),
        }
    }

    /// Whether the key or button `code` is down.
    #[must_use]
    pub fn is_down(&self, code: u32) -> bool {
        if let Some((word, bit)) = Self::bit_of(code) {
            return match word {
                Word::Keyboard(index) => self.keyboard[index] & bit != 0,
                Word::Buttons => self.buttons & bit != 0,
            };
        }
        self.other[..self.other_len].contains(&code)
    }

    /// The modifier keys that are down, as the HID boot report's modifier byte:
    /// bit 0 left Ctrl, 1 left Shift, 2 left Alt, 3 left GUI, 4 to 7 the right
    /// ones.
    #[must_use]
    pub fn modifiers(&self) -> u8 {
        // Usages 0xE0 to 0xE7 are bits 32 to 39 of the bitmap's fourth word.
        u8::try_from((self.keyboard[3] >> 32) & 0xFF).unwrap_or(0)
    }

    /// The last value of the absolute axis `code`, if the device reported it.
    #[must_use]
    pub fn axis(&self, code: u32) -> Option<i32> {
        self.axes[..self.axes_len]
            .iter()
            .find(|(axis, _)| *axis == code)
            .map(|&(_, value)| value)
    }

    /// Whether a `Dropped` record arrived and no snapshot has replaced the
    /// state since. A stale state may have stuck keys.
    #[must_use]
    pub const fn is_stale(&self) -> bool {
        self.stale
    }

    /// Replace the state with `snapshot`, the producer's current state, and
    /// clear the stale mark.
    pub fn resync(&mut self, snapshot: &Self) {
        *self = *snapshot;
        self.stale = false;
    }

    /// Number of keys and buttons down.
    #[must_use]
    pub fn down_count(&self) -> u32 {
        self.keyboard
            .iter()
            .map(|word| word.count_ones())
            .sum::<u32>()
            + self.buttons.count_ones()
            + u32::try_from(self.other_len).unwrap_or(u32::MAX)
    }

    /// Call `visit` with the code of every key and button that is down: the
    /// keyboard page in usage order, then the buttons, then the other codes in
    /// the order they went down.
    pub fn for_each_down(&self, mut visit: impl FnMut(u32)) {
        for (word_index, word) in self.keyboard.iter().enumerate() {
            for bit in 0..64_u32 {
                if word & (1 << bit) != 0 {
                    let usage = u32::try_from(word_index).unwrap_or(0) * 64 + bit;
                    visit(usage::code(
                        page::KEYBOARD,
                        u16::try_from(usage).unwrap_or(0),
                    ));
                }
            }
        }
        for bit in 0..64_u16 {
            if self.buttons & (1 << bit) != 0 {
                visit(usage::code(page::BUTTON, bit + 1));
            }
        }
        for &code in &self.other[..self.other_len] {
            visit(code);
        }
    }

    /// Call `visit` with the code and last value of every absolute axis seen.
    pub fn for_each_axis(&self, mut visit: impl FnMut(u32, i32)) {
        for &(code, value) in &self.axes[..self.axes_len] {
            visit(code, value);
        }
    }

    fn set_key(&mut self, code: u32, down: bool) -> Result<(), StateError> {
        if let Some((word, bit)) = Self::bit_of(code) {
            let target = match word {
                Word::Keyboard(index) => &mut self.keyboard[index],
                Word::Buttons => &mut self.buttons,
            };
            if down {
                *target |= bit;
            } else {
                *target &= !bit;
            }
            return Ok(());
        }
        let held = self.other[..self.other_len].iter().position(|&c| c == code);
        match (down, held) {
            (true, None) => {
                if self.other_len == MAX_OTHER_KEYS {
                    return Err(StateError::Full);
                }
                self.other[self.other_len] = code;
                self.other_len += 1;
            }
            (false, Some(index)) => {
                self.other_len -= 1;
                self.other[index] = self.other[self.other_len];
                self.other[self.other_len] = 0;
            }
            (true, Some(_)) | (false, None) => {}
        }
        Ok(())
    }

    fn set_axis(&mut self, code: u32, value: i32) -> Result<(), StateError> {
        if let Some(slot) = self.axes[..self.axes_len]
            .iter_mut()
            .find(|(axis, _)| *axis == code)
        {
            slot.1 = value;
            return Ok(());
        }
        if self.axes_len == MAX_AXES {
            return Err(StateError::Full);
        }
        self.axes[self.axes_len] = (code, value);
        self.axes_len += 1;
        Ok(())
    }

    /// The bitmap word and bit that hold `code`, if it has one.
    fn bit_of(code: u32) -> Option<(Word, u64)> {
        let usage = usage::usage_of(code);
        match usage::page_of(code) {
            page::KEYBOARD if usage < 256 => {
                Some((Word::Keyboard(usize::from(usage / 64)), 1 << (usage % 64)))
            }
            page::BUTTON if (1..=64).contains(&usage) => Some((Word::Buttons, 1 << (usage - 1))),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
enum Word {
    Keyboard(usize),
    Buttons,
}

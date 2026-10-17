//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! HID usages (USB HID Usage Tables) for the pages in use. An event `code` packs
//! a usage page and a usage as `(page << 16) | usage`.

/// Usage pages in use.
pub mod page {
    pub const GENERIC_DESKTOP: u16 = 0x01;
    pub const KEYBOARD: u16 = 0x07;
    pub const LED: u16 = 0x08;
    pub const BUTTON: u16 = 0x09;
    pub const CONSUMER: u16 = 0x0C;
    pub const DIGITIZER: u16 = 0x0D;
}

/// Generic Desktop page usages.
pub mod generic_desktop {
    pub const POINTER: u16 = 0x01;
    pub const MOUSE: u16 = 0x02;
    pub const KEYBOARD: u16 = 0x06;
    pub const X: u16 = 0x30;
    pub const Y: u16 = 0x31;
    pub const Z: u16 = 0x32;
    pub const RX: u16 = 0x33;
    pub const RY: u16 = 0x34;
    pub const RZ: u16 = 0x35;
    pub const WHEEL: u16 = 0x38;
    pub const HAT_SWITCH: u16 = 0x39;
}

/// Keyboard page usages: the modifiers. Other keys are the usages of the
/// table (0x04 `a` to 0xE7).
pub mod keyboard {
    pub const LEFT_CTRL: u16 = 0xE0;
    pub const LEFT_SHIFT: u16 = 0xE1;
    pub const LEFT_ALT: u16 = 0xE2;
    pub const LEFT_GUI: u16 = 0xE3;
    pub const RIGHT_CTRL: u16 = 0xE4;
    pub const RIGHT_SHIFT: u16 = 0xE5;
    pub const RIGHT_ALT: u16 = 0xE6;
    pub const RIGHT_GUI: u16 = 0xE7;
    /// The first and last modifier usage.
    pub const FIRST_MODIFIER: u16 = LEFT_CTRL;
    pub const LAST_MODIFIER: u16 = RIGHT_GUI;
}

/// LED page usages.
pub mod led {
    pub const NUM_LOCK: u16 = 0x01;
    pub const CAPS_LOCK: u16 = 0x02;
    pub const SCROLL_LOCK: u16 = 0x03;
    pub const COMPOSE: u16 = 0x04;
    pub const KANA: u16 = 0x05;
}

/// Consumer page usages.
pub mod consumer {
    /// Horizontal wheel.
    pub const AC_PAN: u16 = 0x0238;
}

/// The code of `usage` on `page`.
#[must_use]
pub const fn code(page: u16, usage: u16) -> u32 {
    (page as u32) << 16 | usage as u32
}

/// The usage page of `code`.
#[must_use]
#[allow(clippy::cast_possible_truncation, reason = "the shift leaves 16 bits")]
pub const fn page_of(code: u32) -> u16 {
    (code >> 16) as u16
}

/// The usage of `code`.
#[must_use]
#[allow(clippy::cast_possible_truncation, reason = "the mask leaves 16 bits")]
pub const fn usage_of(code: u32) -> u16 {
    (code & 0xFFFF) as u16
}

/// The code of mouse or gamepad button `number` (from 1) on the Button page.
#[must_use]
pub const fn button(number: u16) -> u32 {
    code(page::BUTTON, number)
}

/// Whether `code` is a keyboard modifier (usages 0xE0 to 0xE7).
#[must_use]
pub const fn is_modifier(code: u32) -> bool {
    page_of(code) == page::KEYBOARD
        && usage_of(code) >= keyboard::FIRST_MODIFIER
        && usage_of(code) <= keyboard::LAST_MODIFIER
}

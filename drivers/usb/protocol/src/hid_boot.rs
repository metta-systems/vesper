//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! HID boot-protocol reports (USB HID 1.11, Appendix B). With boot protocol set
//! the layout is fixed, so no report descriptor parser is needed.

/// Boot keyboard report: modifier bits, a reserved byte, six key usages.
pub const KEYBOARD_REPORT_LENGTH: usize = 8;
/// Boot mouse report: buttons, x, y, and an optional wheel byte.
pub const MOUSE_REPORT_MIN_LENGTH: usize = 3;

/// Usage in a key slot that means "no key".
pub const NO_KEY: u8 = 0x00;
/// Usage in every key slot when more keys are down than the report holds.
pub const ROLL_OVER: u8 = 0x01;

/// Left mouse button bit in the mouse report.
pub const BUTTON_LEFT: u8 = 0x01;
/// Right mouse button bit.
pub const BUTTON_RIGHT: u8 = 0x02;
/// Middle mouse button bit.
pub const BUTTON_MIDDLE: u8 = 0x04;

/// Why a report was not decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportError {
    /// The report is not the boot layout's length.
    WrongLength,
    /// The keyboard reports more keys than it can hold. The report is
    /// ignored, so the key state stays what it was.
    RollOver,
}

/// A decoded boot keyboard report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyboardReport {
    /// One bit per modifier: bit 0 left Ctrl, 1 left Shift, 2 left Alt, 3 left GUI,
    /// 4 right Ctrl, 5 right Shift, 6 right Alt, 7 right GUI.
    pub modifiers: u8,
    pub keys: [u8; 6],
}

impl KeyboardReport {
    /// Decode an 8-byte boot keyboard report.
    pub fn parse(bytes: &[u8]) -> Result<Self, ReportError> {
        let [modifiers, _reserved, slots @ ..] = bytes else {
            return Err(ReportError::WrongLength);
        };
        let Ok(keys) = <[u8; 6]>::try_from(slots) else {
            return Err(ReportError::WrongLength);
        };
        if keys.contains(&ROLL_OVER) {
            return Err(ReportError::RollOver);
        }
        Ok(Self {
            modifiers: *modifiers,
            keys,
        })
    }
}

/// One change between two keyboard reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyChange {
    /// A key usage went down or up.
    Key { usage: u8, pressed: bool },
    /// One modifier bit (a single-bit mask) went down or up.
    Modifier { mask: u8, pressed: bool },
}

/// Report every change from `previous` to `current` through `emit`: modifiers
/// first (bit order), then keys released, then keys pressed. Keys present in
/// both reports produce nothing, so holding a key repeats nothing here.
pub fn for_each_change(
    previous: &KeyboardReport,
    current: &KeyboardReport,
    mut emit: impl FnMut(KeyChange),
) {
    let changed_modifiers = previous.modifiers ^ current.modifiers;
    for bit in 0..8_u8 {
        let mask = 1_u8 << bit;
        if changed_modifiers & mask != 0 {
            emit(KeyChange::Modifier {
                mask,
                pressed: current.modifiers & mask != 0,
            });
        }
    }
    for &usage in previous.keys.iter().filter(|&&usage| usage != NO_KEY) {
        if !current.keys.contains(&usage) {
            emit(KeyChange::Key {
                usage,
                pressed: false,
            });
        }
    }
    for &usage in current.keys.iter().filter(|&&usage| usage != NO_KEY) {
        if !previous.keys.contains(&usage) {
            emit(KeyChange::Key {
                usage,
                pressed: true,
            });
        }
    }
}

/// A decoded boot mouse report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouseReport {
    /// Button bits, `BUTTON_LEFT` and friends.
    pub buttons: u8,
    pub dx: i8,
    pub dy: i8,
    /// Wheel movement; zero when the report has no wheel byte.
    pub wheel: i8,
}

impl MouseReport {
    /// Decode a 3- or 4-byte boot mouse report.
    pub fn parse(bytes: &[u8]) -> Result<Self, ReportError> {
        match *bytes {
            [buttons, dx, dy] => Ok(Self {
                buttons,
                dx: i8::from_le_bytes([dx]),
                dy: i8::from_le_bytes([dy]),
                wheel: 0,
            }),
            [buttons, dx, dy, wheel] => Ok(Self {
                buttons,
                dx: i8::from_le_bytes([dx]),
                dy: i8::from_le_bytes([dy]),
                wheel: i8::from_le_bytes([wheel]),
            }),
            _ => Err(ReportError::WrongLength),
        }
    }
}

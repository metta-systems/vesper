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
/// Usage in every key slot when more keys are down than the report holds
/// (`ErrorRollOver`, HID Usage Tables 1.5 §10).
pub const ROLL_OVER: u8 = 0x01;
/// Usage in every key slot of a keyboard whose self-test failed (`POSTFail`).
pub const POST_FAIL: u8 = 0x02;
/// Usage in every key slot of a keyboard in an undefined error state
/// (`ErrorUndefined`).
pub const ERROR_UNDEFINED: u8 = 0x03;

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
    /// The keyboard reports an error state (`POSTFail`, `ErrorUndefined`)
    /// in its key slots. The report is ignored like a roll-over: its slots
    /// hold no keys, so taking it would release every key held.
    KeyboardError,
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
        if keys.contains(&POST_FAIL) || keys.contains(&ERROR_UNDEFINED) {
            return Err(ReportError::KeyboardError);
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
    // A usage listed twice in one report is one key: only its first slot
    // counts.
    let first = |keys: &[u8; 6], index: usize| {
        keys[index] != NO_KEY && !keys[..index].contains(&keys[index])
    };
    for (index, &usage) in previous.keys.iter().enumerate() {
        if first(&previous.keys, index) && !current.keys.contains(&usage) {
            emit(KeyChange::Key {
                usage,
                pressed: false,
            });
        }
    }
    for (index, &usage) in current.keys.iter().enumerate() {
        if first(&current.keys, index) && !previous.keys.contains(&usage) {
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

/// `bmRequestType` of a class request to an interface, host to device.
pub const CLASS_INTERFACE_OUT: u8 = 0x21;
/// `bRequest` of the HID `SET_IDLE`.
pub const REQUEST_SET_IDLE: u8 = 0x0A;
/// `bRequest` of the HID `SET_PROTOCOL`.
pub const REQUEST_SET_PROTOCOL: u8 = 0x0B;

/// HID `SET_PROTOCOL` to `interface`: boot protocol, or the report protocol.
#[must_use]
pub const fn set_protocol(interface: u8, boot: bool) -> crate::setup::SetupPacket {
    crate::setup::SetupPacket {
        request_type: CLASS_INTERFACE_OUT,
        request: REQUEST_SET_PROTOCOL,
        value: if boot { 0 } else { 1 },
        index: interface as u16,
        length: 0,
    }
}

/// HID `SET_IDLE` to `interface` (HID 1.11 §7.2.4): the device reports a
/// changed state only when `duration_4ms` is zero, else also every
/// `duration_4ms` times 4 ms while nothing changes. `report_id` zero applies
/// to all reports.
#[must_use]
pub const fn set_idle(interface: u8, duration_4ms: u8, report_id: u8) -> crate::setup::SetupPacket {
    crate::setup::SetupPacket {
        request_type: CLASS_INTERFACE_OUT,
        request: REQUEST_SET_IDLE,
        value: u16::from_le_bytes([report_id, duration_4ms]),
        index: interface as u16,
        length: 0,
    }
}

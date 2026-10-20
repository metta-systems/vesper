//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! Hub class logic (USB 2.0 chapter 11): the hub descriptor, the class
//! requests, port status and the state machine one hub port goes through from
//! power-on to an enumerated device and back to empty. Nothing here does I/O:
//! the caller runs the actions [`Port::next`] asks for (control transfers to
//! the hub, delays) and feeds back what happened.

use crate::{
    descriptor::ParseError,
    enumeration::Speed,
    setup::{DEVICE_TO_HOST, SetupPacket},
};

/// Descriptor type of a hub descriptor.
pub const DESCRIPTOR_HUB: u8 = 0x29;
/// Device class code of a hub.
pub const CLASS_HUB: u8 = 9;

/// `bmRequestType` bits of a class request: type class.
const TYPE_CLASS: u8 = 0x20;
/// `bmRequestType` recipient: the hub.
const RECIPIENT_DEVICE: u8 = 0x00;
/// `bmRequestType` recipient: a port of the hub.
const RECIPIENT_OTHER: u8 = 0x03;

/// `bRequest` of `GET_STATUS`.
pub const REQUEST_GET_STATUS: u8 = 0;
/// `bRequest` of `CLEAR_FEATURE`.
pub const REQUEST_CLEAR_FEATURE: u8 = 1;
/// `bRequest` of `SET_FEATURE`.
pub const REQUEST_SET_FEATURE: u8 = 3;
/// `bRequest` of `GET_DESCRIPTOR`.
const REQUEST_GET_DESCRIPTOR: u8 = 6;

/// Port feature selector: the port's connection.
pub const PORT_CONNECTION: u16 = 0;
/// Port feature selector: the port is enabled.
pub const PORT_ENABLE: u16 = 1;
/// Port feature selector: the port is in reset.
pub const PORT_RESET: u16 = 4;
/// Port feature selector: the port is powered.
pub const PORT_POWER: u16 = 8;
/// Port feature selector: the connection changed.
pub const C_PORT_CONNECTION: u16 = 16;
/// Port feature selector: the enable state changed.
pub const C_PORT_ENABLE: u16 = 17;
/// Port feature selector: an over-current condition changed.
pub const C_PORT_OVER_CURRENT: u16 = 19;
/// Port feature selector: a reset finished.
pub const C_PORT_RESET: u16 = 20;

/// Milliseconds a connection must stay stable before the port is reset
/// (USB 2.0 §7.1.7.3, `TATTDB`).
pub const DEBOUNCE_MS: u32 = 100;
/// Milliseconds between two looks at a port that is empty, being debounced or
/// in use.
pub const POLL_MS: u32 = 25;
/// Milliseconds between two looks at a port that is being reset. A hub holds
/// reset for 10 to 20 ms (§11.5.1.5).
pub const RESET_POLL_MS: u32 = 10;
/// Milliseconds a device needs after reset before it takes requests (`TRSTRCY`).
pub const RESET_RECOVERY_MS: u32 = 10;

/// Largest hub a hub descriptor describes (a hub has at most 255 ports; the
/// port bitmaps take up to 32 bytes).
pub const MAX_HUB_DESCRIPTOR: usize = 7 + 2 * 32;

/// The part of a hub descriptor the host needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HubDescriptor {
    /// Downstream ports.
    pub ports: u8,
    /// `wHubCharacteristics`.
    pub characteristics: u16,
    /// Milliseconds from powering a port to the power being good.
    pub power_on_to_good_ms: u32,
    /// Current the hub controller draws, in milliamperes.
    pub controller_current_ma: u8,
}

impl HubDescriptor {
    /// Parse the hub descriptor in `bytes`.
    ///
    /// # Errors
    ///
    /// [`ParseError::Truncated`] if `bytes` is shorter than the descriptor says,
    /// [`ParseError::BadLength`] if it is not a
    /// hub descriptor or describes a hub with no ports.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let [length, kind, ports, low, high, power, current, ..] = *bytes else {
            return Err(ParseError::Truncated);
        };
        if kind != DESCRIPTOR_HUB || usize::from(length) < 7 {
            return Err(ParseError::BadLength);
        }
        if usize::from(length) > bytes.len() {
            return Err(ParseError::Truncated);
        }
        if ports == 0 {
            return Err(ParseError::BadLength);
        }
        Ok(Self {
            ports,
            characteristics: u16::from_le_bytes([low, high]),
            // `bPwrOn2PwrGood` is in units of 2 ms.
            power_on_to_good_ms: u32::from(power) * 2,
            controller_current_ma: current,
        })
    }
}

/// `GET_DESCRIPTOR` of the hub descriptor, asking for at most `length` bytes.
#[must_use]
pub const fn get_hub_descriptor(length: u16) -> SetupPacket {
    SetupPacket {
        request_type: DEVICE_TO_HOST | TYPE_CLASS | RECIPIENT_DEVICE,
        request: REQUEST_GET_DESCRIPTOR,
        value: (DESCRIPTOR_HUB as u16) << 8,
        index: 0,
        length,
    }
}

/// `GET_STATUS` of port `port` (counted from 1): four bytes, see [`PortStatus`].
#[must_use]
pub const fn get_port_status(port: u8) -> SetupPacket {
    SetupPacket {
        request_type: DEVICE_TO_HOST | TYPE_CLASS | RECIPIENT_OTHER,
        request: REQUEST_GET_STATUS,
        value: 0,
        index: port as u16,
        length: 4,
    }
}

/// `SET_FEATURE` of `feature` on port `port`.
#[must_use]
pub const fn set_port_feature(port: u8, feature: u16) -> SetupPacket {
    SetupPacket {
        request_type: TYPE_CLASS | RECIPIENT_OTHER,
        request: REQUEST_SET_FEATURE,
        value: feature,
        index: port as u16,
        length: 0,
    }
}

/// `CLEAR_FEATURE` of `feature` on port `port`.
#[must_use]
pub const fn clear_port_feature(port: u8, feature: u16) -> SetupPacket {
    SetupPacket {
        request_type: TYPE_CLASS | RECIPIENT_OTHER,
        request: REQUEST_CLEAR_FEATURE,
        value: feature,
        index: port as u16,
        length: 0,
    }
}

/// A port's status and its change bits, as `GET_STATUS` returns them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PortStatus {
    pub status: u16,
    pub change: u16,
}

impl PortStatus {
    /// Status bit: a device is attached.
    const CONNECTED: u16 = 1 << 0;
    /// Status bit: the port is enabled.
    const ENABLED: u16 = 1 << 1;
    /// Status bit: an over-current condition exists.
    const OVER_CURRENT: u16 = 1 << 3;
    /// Status bit: the port is in reset.
    const RESET: u16 = 1 << 4;
    /// Status bit: the port is powered.
    const POWER: u16 = 1 << 8;
    /// Status bit: a low-speed device is attached.
    const LOW_SPEED: u16 = 1 << 9;
    /// Status bit: a high-speed device is attached.
    const HIGH_SPEED: u16 = 1 << 10;
    /// Change bit: the connection changed.
    const CONNECTION_CHANGED: u16 = 1 << 0;
    /// Change bit: a reset finished.
    const RESET_CHANGED: u16 = 1 << 4;

    /// The status in the four bytes `GET_STATUS` returned.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 4]) -> Self {
        Self {
            status: u16::from_le_bytes([bytes[0], bytes[1]]),
            change: u16::from_le_bytes([bytes[2], bytes[3]]),
        }
    }

    /// A device is attached.
    #[must_use]
    pub const fn connected(self) -> bool {
        self.status & Self::CONNECTED != 0
    }

    /// The port is enabled (a reset enabled it).
    #[must_use]
    pub const fn enabled(self) -> bool {
        self.status & Self::ENABLED != 0
    }

    /// An over-current condition exists on the port.
    #[must_use]
    pub const fn over_current(self) -> bool {
        self.status & Self::OVER_CURRENT != 0
    }

    /// The port is in reset.
    #[must_use]
    pub const fn in_reset(self) -> bool {
        self.status & Self::RESET != 0
    }

    /// The port is powered.
    #[must_use]
    pub const fn powered(self) -> bool {
        self.status & Self::POWER != 0
    }

    /// The connection changed since the change was last cleared.
    #[must_use]
    pub const fn connection_changed(self) -> bool {
        self.change & Self::CONNECTION_CHANGED != 0
    }

    /// A reset finished since the change was last cleared.
    #[must_use]
    pub const fn reset_changed(self) -> bool {
        self.change & Self::RESET_CHANGED != 0
    }

    /// The speed of the attached device. A device is full speed unless the
    /// status says otherwise.
    #[must_use]
    pub const fn speed(self) -> Speed {
        if self.status & Self::HIGH_SPEED != 0 {
            Speed::High
        } else if self.status & Self::LOW_SPEED != 0 {
            Speed::Low
        } else {
            Speed::Full
        }
    }
}

/// What a port's state machine is told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortEvent {
    /// First call: the hub is configured.
    Start,
    /// The wait [`PortAction::Wait`] asked for has elapsed.
    Delayed,
    /// The hub's answer to [`PortAction::PollStatus`].
    Status(PortStatus),
    /// The request the last action asked for (set or clear a feature) finished.
    Done,
    /// The device behind the port was enumerated, or was not.
    Enumerated { ok: bool },
}

/// What the caller does for a port next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortAction {
    /// Nothing until the port is polled again.
    Idle,
    /// `SET_FEATURE(PORT_POWER)`, then report [`PortEvent::Done`].
    PowerOn,
    /// Wait this many milliseconds, then report [`PortEvent::Delayed`].
    Wait { milliseconds: u32 },
    /// `GET_STATUS`, then report [`PortEvent::Status`].
    PollStatus,
    /// `CLEAR_FEATURE` of this change feature, then report [`PortEvent::Done`].
    ClearChange { feature: u16 },
    /// `SET_FEATURE(PORT_RESET)`, then report [`PortEvent::Done`].
    Reset,
    /// Enumerate the device behind the port at this speed (the enumeration
    /// resets nothing: the port is reset already), then report
    /// [`PortEvent::Enumerated`].
    Enumerate { speed: Speed },
    /// The device is gone: release what it held, then report
    /// [`PortEvent::Done`].
    Teardown,
    /// The port cannot be used (over-current, or a reset that left it
    /// disabled).
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PortState {
    Unpowered,
    PoweringUp,
    Empty,
    /// A connection is stable for `stable_ms` so far.
    Debouncing {
        stable_ms: u32,
    },
    Resetting,
    ClearingReset {
        speed: Speed,
    },
    Recovering {
        speed: Speed,
    },
    Enumerating,
    Enumerated,
    TearingDown,
    /// The device did not enumerate: the port waits for it to be unplugged.
    Failed,
    /// The port is over-current and stays off.
    OverCurrent,
}

/// The state of one downstream port of a hub: power, debounce, reset,
/// enumeration and teardown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Port {
    state: PortState,
    power_good_ms: u32,
    /// The last look at an empty port found nothing: no connection, no change.
    quiet: bool,
}

impl Port {
    /// A port whose power takes `power_on_to_good_ms` to be good (from the hub
    /// descriptor).
    #[must_use]
    pub const fn new(power_on_to_good_ms: u32) -> Self {
        Self {
            state: PortState::Unpowered,
            power_good_ms: power_on_to_good_ms,
            quiet: false,
        }
    }

    /// Whether a device is enumerated on the port.
    #[must_use]
    pub const fn has_device(&self) -> bool {
        matches!(self.state, PortState::Enumerated)
    }

    /// Whether the port gave up.
    #[must_use]
    pub const fn failed(&self) -> bool {
        matches!(self.state, PortState::Failed | PortState::OverCurrent)
    }

    /// Whether the port has reached a resting state after power-on: a device is
    /// enumerated, the port gave up, or a look at the empty port found nothing.
    #[must_use]
    pub const fn settled(&self) -> bool {
        match self.state {
            PortState::Enumerated | PortState::Failed | PortState::OverCurrent => true,
            PortState::Empty => self.quiet,
            _ => false,
        }
    }

    /// Whether the device behind the port may still answer at address 0: the
    /// port is being reset, or its device is not yet given an address. Only one
    /// port of a hub may be in that state at a time, or the devices all answer
    /// the same requests and corrupt each other's transfers.
    #[must_use]
    pub const fn holds_default_address(&self) -> bool {
        matches!(
            self.state,
            PortState::Resetting
                | PortState::ClearingReset { .. }
                | PortState::Recovering { .. }
                | PortState::Enumerating
        )
    }

    /// Take back a [`PortAction::Reset`] that cannot run yet because another port
    /// holds the default address: the port keeps debouncing and asks to reset
    /// again at its next look.
    pub fn defer_reset(&mut self) {
        if matches!(self.state, PortState::Resetting) {
            self.state = PortState::Debouncing {
                stable_ms: DEBOUNCE_MS.saturating_sub(POLL_MS),
            };
        }
    }

    /// Wait one polling step, then look at the port again.
    const fn poll_later() -> PortAction {
        PortAction::Wait {
            milliseconds: POLL_MS,
        }
    }

    /// The action after `event`.
    ///
    /// A wait is followed by a poll in every state that waits to look at the
    /// port again; the other events answer the action just run.
    pub fn next(&mut self, event: PortEvent) -> PortAction {
        if let PortEvent::Status(status) = event
            && status.over_current()
            && !matches!(self.state, PortState::OverCurrent)
        {
            self.state = PortState::OverCurrent;
            return PortAction::Failed;
        }
        match (self.state, event) {
            (PortState::Unpowered, PortEvent::Start) => PortAction::PowerOn,
            (PortState::Unpowered, PortEvent::Done) => {
                self.state = PortState::PoweringUp;
                PortAction::Wait {
                    milliseconds: self.power_good_ms,
                }
            }
            (PortState::PoweringUp, PortEvent::Delayed) => {
                self.state = PortState::Empty;
                PortAction::PollStatus
            }
            (
                PortState::Empty
                | PortState::Debouncing { .. }
                | PortState::Resetting
                | PortState::Enumerated
                | PortState::Failed,
                PortEvent::Delayed,
            ) => PortAction::PollStatus,
            (PortState::Empty | PortState::Debouncing { .. }, PortEvent::Done) => {
                Self::poll_later()
            }
            (PortState::Empty, PortEvent::Status(status)) => self.when_empty(status),
            (PortState::Debouncing { stable_ms }, PortEvent::Status(status)) => {
                self.when_debouncing(stable_ms, status)
            }
            (PortState::Resetting, PortEvent::Done) => PortAction::Wait {
                milliseconds: RESET_POLL_MS,
            },
            (PortState::Resetting, PortEvent::Status(status)) => self.when_resetting(status),
            (PortState::ClearingReset { speed }, PortEvent::Done) => {
                self.state = PortState::Recovering { speed };
                PortAction::Wait {
                    milliseconds: RESET_RECOVERY_MS,
                }
            }
            (PortState::Recovering { speed }, PortEvent::Delayed) => {
                self.state = PortState::Enumerating;
                PortAction::Enumerate { speed }
            }
            (PortState::Enumerating, PortEvent::Enumerated { ok: true }) => {
                self.state = PortState::Enumerated;
                Self::poll_later()
            }
            (PortState::Enumerating, PortEvent::Enumerated { ok: false }) => {
                self.state = PortState::Failed;
                PortAction::Failed
            }
            (PortState::Failed, PortEvent::Status(status)) => self.when_failed(status),
            (PortState::Failed, PortEvent::Done) => Self::poll_later(),
            (PortState::Enumerated, PortEvent::Status(status)) => {
                if status.connected() && !status.connection_changed() {
                    Self::poll_later()
                } else {
                    self.state = PortState::TearingDown;
                    PortAction::Teardown
                }
            }
            (PortState::TearingDown, PortEvent::Done) => {
                self.state = PortState::Empty;
                PortAction::ClearChange {
                    feature: C_PORT_CONNECTION,
                }
            }
            _ => PortAction::Idle,
        }
    }

    /// A poll of a port that gave up: it waits for the device to be unplugged,
    /// and then takes the next one like an empty port.
    fn when_failed(&mut self, status: PortStatus) -> PortAction {
        if status.connected() {
            return Self::poll_later();
        }
        self.state = PortState::Empty;
        if status.connection_changed() {
            PortAction::ClearChange {
                feature: C_PORT_CONNECTION,
            }
        } else {
            Self::poll_later()
        }
    }

    /// A poll of an empty port: a connection starts debouncing.
    fn when_empty(&mut self, status: PortStatus) -> PortAction {
        self.quiet = !status.connected() && !status.connection_changed();
        if status.connected() {
            self.state = PortState::Debouncing { stable_ms: 0 };
        }
        if status.connection_changed() {
            return PortAction::ClearChange {
                feature: C_PORT_CONNECTION,
            };
        }
        Self::poll_later()
    }

    /// A poll of a connection being debounced.
    fn when_debouncing(&mut self, stable_ms: u32, status: PortStatus) -> PortAction {
        if !status.connected() {
            self.state = PortState::Empty;
            return if status.connection_changed() {
                PortAction::ClearChange {
                    feature: C_PORT_CONNECTION,
                }
            } else {
                Self::poll_later()
            };
        }
        if status.connection_changed() {
            // Bounced: start counting again.
            self.state = PortState::Debouncing { stable_ms: 0 };
            return PortAction::ClearChange {
                feature: C_PORT_CONNECTION,
            };
        }
        let stable_ms = stable_ms + POLL_MS;
        if stable_ms >= DEBOUNCE_MS {
            self.state = PortState::Resetting;
            return PortAction::Reset;
        }
        self.state = PortState::Debouncing { stable_ms };
        Self::poll_later()
    }

    /// A poll of a port being reset.
    fn when_resetting(&mut self, status: PortStatus) -> PortAction {
        if !status.connected() {
            self.state = PortState::Empty;
            return Self::poll_later();
        }
        if status.reset_changed() && !status.in_reset() {
            if !status.enabled() {
                self.state = PortState::Failed;
                return PortAction::Failed;
            }
            self.state = PortState::ClearingReset {
                speed: status.speed(),
            };
            return PortAction::ClearChange {
                feature: C_PORT_RESET,
            };
        }
        PortAction::Wait {
            milliseconds: RESET_POLL_MS,
        }
    }
}

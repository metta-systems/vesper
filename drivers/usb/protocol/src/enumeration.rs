//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! The enumeration state machine for one newly attached device. It owns no
//! I/O: the caller feeds it the results of each step (port reset, delays,
//! control transfers) and carries out the step it returns.
//!
//! Sequence: reset the port, read the first 8 bytes of the device descriptor
//! at address 0, `SET_ADDRESS`, read the full device descriptor, read the
//! configuration header, read the whole configuration, `SET_CONFIGURATION`. The
//! caller keeps the full configuration bytes it got back, and walks their
//! interfaces with `descriptor::walk`.

use crate::{
    descriptor::{Configuration, Descriptor, ParseError, walk},
    setup::{DESCRIPTOR_CONFIGURATION, DESCRIPTOR_DEVICE, SetupPacket},
};

/// Port reset signalling time, in milliseconds (USB 2.0 §7.1.7.5).
pub const RESET_MS: u32 = 10;
/// Recovery time after a port reset, in milliseconds.
pub const RECOVERY_MS: u32 = 10;
/// Time a device needs after `SET_ADDRESS`, in milliseconds (USB 2.0 §9.2.6.3).
pub const ADDRESS_SETTLE_MS: u32 = 2;

/// Lowest address a device may take. Address 0 is the default address.
pub const FIRST_ADDRESS: u8 = 1;
/// Highest address a device may take.
pub const LAST_ADDRESS: u8 = 127;

/// Speed a device runs at, as the host controller reports it after a reset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speed {
    Low,
    Full,
    High,
}

/// Why enumeration failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The device answered a request with a STALL.
    Stalled,
    /// The transfer failed on the bus.
    Transfer,
    /// A descriptor could not be read.
    Descriptor(ParseError),
    /// The device sent something the protocol does not allow here, such as a
    /// device with no configurations, or a reply in the wrong state.
    Protocol,
}

/// What the caller does next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Reset the root port, then report the device's speed with `Event::PortReset`.
    ResetPort,
    /// Wait this many milliseconds, then report `Event::Delayed`.
    Delay { milliseconds: u32 },
    /// Run this control transfer to device address 0 or the new address. The
    /// data stage is `setup.length` bytes long (zero for no data stage); pass
    /// the bytes read back in `Event::Control`.
    Control { setup: SetupPacket },
    /// Enumeration finished: the device is at `address` and configuration
    /// `configuration_value` is active. Interfaces of the configuration are
    /// the caller's to claim.
    Configured {
        address: u8,
        configuration_value: u8,
    },
    /// Enumeration failed. The caller should power the port down or leave it to
    /// the hub driver.
    Failed(Failure),
}

/// The result of the last step, handed to `Enumerator::next`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event<'a> {
    /// First call: begin enumeration.
    Start,
    /// The port reset completed at this speed.
    PortReset(Speed),
    /// The delay returned by `Step::Delay` has elapsed.
    Delayed,
    /// The control transfer completed; the slice is its data stage (empty for
    /// requests without one).
    Control(&'a [u8]),
    /// The control transfer failed.
    ControlFailed(Failure),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Idle,
    Resetting,
    Recovering,
    FirstDescriptor,
    SettlingAddress,
    DeviceDescriptor,
    ConfigurationHeader,
    ConfigurationFull,
    /// The whole configuration is being read; `value` is its `bConfigurationValue`.
    Configuring {
        value: u8,
    },
    SettingConfiguration {
        value: u8,
    },
    Done,
    Failed,
}

/// Enumeration of one device, from reset to configured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Enumerator {
    address: u8,
    speed: Speed,
    max_packet0: u8,
    state: State,
}

impl Enumerator {
    /// An enumerator that will give the device `address`, or `None` if the
    /// address is outside `FIRST_ADDRESS..=LAST_ADDRESS`.
    #[must_use]
    pub const fn new(address: u8) -> Option<Self> {
        if address < FIRST_ADDRESS || address > LAST_ADDRESS {
            return None;
        }
        Some(Self {
            address,
            speed: Speed::Full,
            max_packet0: 8,
            state: State::Idle,
        })
    }

    /// Advance by one event and return the step to carry out next.
    pub fn next(&mut self, event: Event<'_>) -> Step {
        if let Event::ControlFailed(failure) = event {
            return self.fail(failure);
        }
        match (self.state, event) {
            (State::Idle, Event::Start) => {
                self.state = State::Resetting;
                Step::ResetPort
            }
            (State::Resetting, Event::PortReset(speed)) => {
                self.speed = speed;
                self.state = State::Recovering;
                Step::Delay {
                    milliseconds: RECOVERY_MS,
                }
            }
            (State::Recovering, Event::Delayed) => {
                self.state = State::FirstDescriptor;
                Step::Control {
                    setup: SetupPacket::get_descriptor(DESCRIPTOR_DEVICE, 0, 8),
                }
            }
            (State::FirstDescriptor, Event::Control(data)) => match device_max_packet0(data) {
                Ok(max_packet0) => {
                    self.max_packet0 = max_packet0;
                    self.state = State::SettlingAddress;
                    Step::Control {
                        setup: SetupPacket::set_address(self.address),
                    }
                }
                Err(error) => self.fail(Failure::Descriptor(error)),
            },
            (State::SettlingAddress, Event::Control(_)) => {
                self.state = State::DeviceDescriptor;
                Step::Delay {
                    milliseconds: ADDRESS_SETTLE_MS,
                }
            }
            (State::DeviceDescriptor, Event::Delayed) => {
                self.state = State::ConfigurationHeader;
                Step::Control {
                    setup: SetupPacket::get_descriptor(DESCRIPTOR_DEVICE, 0, 18),
                }
            }
            (State::ConfigurationHeader, Event::Control(data)) => match device_configurations(data)
            {
                Ok(0) => self.fail(Failure::Protocol),
                Ok(_) => {
                    self.state = State::ConfigurationFull;
                    Step::Control {
                        setup: SetupPacket::get_descriptor(DESCRIPTOR_CONFIGURATION, 0, 9),
                    }
                }
                Err(error) => self.fail(Failure::Descriptor(error)),
            },
            (State::ConfigurationFull, Event::Control(data)) => match configuration_header(data) {
                Ok(header) => {
                    self.state = State::Configuring {
                        value: header.value,
                    };
                    Step::Control {
                        setup: SetupPacket::get_descriptor(
                            DESCRIPTOR_CONFIGURATION,
                            0,
                            header.total_length,
                        ),
                    }
                }
                Err(error) => self.fail(Failure::Descriptor(error)),
            },
            (State::Configuring { value }, Event::Control(_)) => {
                self.state = State::SettingConfiguration { value };
                Step::Control {
                    setup: SetupPacket::set_configuration(value),
                }
            }
            (State::SettingConfiguration { value }, Event::Control(_)) => {
                self.state = State::Done;
                Step::Configured {
                    address: self.address,
                    configuration_value: value,
                }
            }
            _ => self.fail(Failure::Protocol),
        }
    }

    /// The speed the port reported at its reset.
    #[must_use]
    pub const fn speed(&self) -> Speed {
        self.speed
    }

    fn fail(&mut self, failure: Failure) -> Step {
        self.state = State::Failed;
        Step::Failed(failure)
    }
}

/// The first 8 bytes of the device descriptor are enough for `bMaxPacketSize0`,
/// so this reads that byte directly rather than parsing a whole descriptor.
fn device_max_packet0(data: &[u8]) -> Result<u8, ParseError> {
    if data.get(1) != Some(&DESCRIPTOR_DEVICE) {
        return Err(ParseError::BadLength);
    }
    data.get(7).copied().ok_or(ParseError::Truncated)
}

fn device_configurations(data: &[u8]) -> Result<u8, ParseError> {
    match walk(data).next() {
        Some(Ok(Descriptor::Device(device))) => Ok(device.configurations),
        Some(Err(error)) => Err(error),
        _ => Err(ParseError::BadLength),
    }
}

fn configuration_header(data: &[u8]) -> Result<Configuration, ParseError> {
    match walk(data).next() {
        Some(Ok(Descriptor::Configuration(header))) => Ok(header),
        Some(Err(error)) => Err(error),
        _ => Err(ParseError::BadLength),
    }
}

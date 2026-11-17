//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! The enumeration state machine for one newly attached device. It owns no
//! I/O: the caller feeds it the results of each step (port reset, delays,
//! control transfers) and carries out the step it returns.
//!
//! Sequence: reset the port, have the host controller address the device
//! (`usb_hcd_v1.address_device`: an xHCI controller's Address Device
//! command, or a `SET_ADDRESS` the DWC2 controller sends itself), read the
//! first 8 bytes of the device descriptor at the new address, the full device
//! descriptor, the configuration header and the whole configuration, then
//! `SET_CONFIGURATION`. Linux enumerates behind xHCI in this order too. The
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

/// Hands out device addresses, lowest first, and takes them back: the
/// addresses a host controller that sends `SET_ADDRESS` itself gives devices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddressPool {
    /// Bit `n` of word `n / 64` is set when address `n` is taken.
    taken: [u64; 2],
}

impl AddressPool {
    /// A pool with every address from [`FIRST_ADDRESS`] to [`LAST_ADDRESS`] free.
    #[must_use]
    pub const fn new() -> Self {
        Self { taken: [0; 2] }
    }

    /// The lowest free address, now taken, or `None` when all are.
    pub fn take(&mut self) -> Option<u8> {
        (FIRST_ADDRESS..=LAST_ADDRESS).find(|&address| {
            let (word, bit) = Self::place(address);
            let free = self.taken[word] & bit == 0;
            if free {
                self.taken[word] |= bit;
            }
            free
        })
    }

    /// Whether `address` is taken.
    #[must_use]
    pub const fn is_taken(&self, address: u8) -> bool {
        if address < FIRST_ADDRESS || address > LAST_ADDRESS {
            return false;
        }
        let (word, bit) = Self::place(address);
        self.taken[word] & bit != 0
    }

    /// Give `address` back. An address that is not taken stays free.
    pub fn release(&mut self, address: u8) {
        if (FIRST_ADDRESS..=LAST_ADDRESS).contains(&address) {
            let (word, bit) = Self::place(address);
            self.taken[word] &= !bit;
        }
    }

    /// The word and bit of `address` in the bitmap.
    const fn place(address: u8) -> (usize, u64) {
        ((address / 64) as usize, 1 << (address % 64))
    }
}

impl Default for AddressPool {
    fn default() -> Self {
        Self::new()
    }
}

/// Speed a device runs at, as the host controller reports it after a reset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speed {
    Low,
    Full,
    High,
}

/// The polling interval in milliseconds an endpoint's `bInterval` asks for:
/// frames for a low- or full-speed device, `2^(bInterval - 1)` microframes for
/// a high-speed one, at least a millisecond and at most what the controller
/// polls.
#[must_use]
pub fn interval_ms(speed: Speed, b_interval: u8) -> u32 {
    let milliseconds = match speed {
        Speed::Low | Speed::Full => u32::from(b_interval),
        Speed::High => {
            let microframes = 1_u32 << u32::from(b_interval.clamp(1, 16) - 1);
            microframes.div_ceil(8)
        }
    };
    milliseconds.clamp(1, 255)
}

/// The control packet size a device's endpoint 0 is opened with before its
/// device descriptor says (USB 2.0 §5.5.3): a high-speed device's is always
/// 64 bytes, a low-speed device's always 8, and a full-speed device's is one
/// of 8, 16, 32 or 64, so 8 until `bMaxPacketSize0` is read.
#[must_use]
pub const fn default_max_packet0(speed: Speed) -> u16 {
    match speed {
        Speed::High => 64,
        Speed::Low | Speed::Full => 8,
    }
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
    /// Have the host controller address the device, then report the address
    /// it took with `Event::Addressed`.
    AddressDevice,
    /// Run this control transfer to the device at its address. The data stage
    /// is `setup.length` bytes long (zero for no data stage); pass the bytes
    /// read back in `Event::Control`.
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
    /// The host controller addressed the device to this address.
    Addressed(u8),
    /// The control transfer completed; the slice is its data stage (empty for
    /// requests without one).
    Control(&'a [u8]),
    /// The control transfer failed.
    ControlFailed(Failure),
}

/// What the enumerator waits for: each state names the reply or the delay
/// it expects next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Idle,
    Resetting,
    Recovering,
    Addressing,
    SettlingAddress,
    FirstDescriptor,
    DeviceDescriptor,
    ConfigurationHeader,
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
    /// An enumerator at its start; the device's address comes from the host
    /// controller.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            address: 0,
            speed: Speed::Full,
            max_packet0: 8,
            state: State::Idle,
        }
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
                self.state = State::Addressing;
                Step::AddressDevice
            }
            (State::Addressing, Event::Addressed(address))
                if (FIRST_ADDRESS..=LAST_ADDRESS).contains(&address) =>
            {
                self.address = address;
                self.state = State::SettlingAddress;
                Step::Delay {
                    milliseconds: ADDRESS_SETTLE_MS,
                }
            }
            (State::SettlingAddress, Event::Delayed) => {
                self.state = State::FirstDescriptor;
                Step::Control {
                    setup: SetupPacket::get_descriptor(DESCRIPTOR_DEVICE, 0, 8),
                }
            }
            (State::FirstDescriptor, Event::Control(data)) => match device_max_packet0(data) {
                Ok(max_packet0) => {
                    self.max_packet0 = max_packet0;
                    self.state = State::DeviceDescriptor;
                    Step::Control {
                        setup: SetupPacket::get_descriptor(DESCRIPTOR_DEVICE, 0, 18),
                    }
                }
                Err(error) => self.fail(Failure::Descriptor(error)),
            },
            (State::DeviceDescriptor, Event::Control(data)) => match device_configurations(data) {
                Ok(0) => self.fail(Failure::Protocol),
                Ok(_) => {
                    self.state = State::ConfigurationHeader;
                    Step::Control {
                        setup: SetupPacket::get_descriptor(DESCRIPTOR_CONFIGURATION, 0, 9),
                    }
                }
                Err(error) => self.fail(Failure::Descriptor(error)),
            },
            (State::ConfigurationHeader, Event::Control(data)) => {
                match configuration_header(data) {
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
                }
            }
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

impl Default for Enumerator {
    fn default() -> Self {
        Self::new()
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

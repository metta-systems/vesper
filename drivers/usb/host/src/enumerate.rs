//! Enumeration of one device over pipes to the host controller: the state
//! machine of `vesper_usb_protocol::enumeration`, run to completion.

use {
    crate::pipes::{OpenError, Pipe, Pipes},
    usb_hcd_client::{
        pipe_v1::{Error as PipeError, Exception as PipeException, TransferKind},
        usb_hcd_v1::Speed as HcdSpeed,
    },
    vesper_usb_protocol::{
        enumeration::{Enumerator, Event, Failure, Speed, Step},
        setup::{DESCRIPTOR_CONFIGURATION, DESCRIPTOR_DEVICE, REQUEST_SET_ADDRESS, SetupPacket},
    },
};

/// Bytes of configuration descriptor kept: a hub's or a HID device's is far
/// shorter.
pub const CONFIGURATION_CAPACITY: usize = 256;

/// The hub and its port a low- or full-speed device is reached through when the
/// hub runs split transactions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    pub hub_address: u8,
    pub hub_port: u8,
}

/// Why a device could not be enumerated.
#[derive(Debug)]
pub enum EnumerateError {
    /// The port reset failed.
    Reset,
    /// The controller refused a pipe.
    Pipe(OpenError),
    /// The controller refused to route a pipe through the hub.
    Route(PipeError),
    /// The device did not enumerate.
    Failed {
        failure: Failure,
        /// The controller's report of the last control transfer, if that
        /// is what failed.
        transfer: Option<PipeError>,
    },
}

/// An enumerated device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Enumerated {
    pub address: u8,
    pub speed: Speed,
    /// The device descriptor.
    pub device: [u8; 18],
    /// The configuration descriptor with its interfaces and endpoints, in
    /// `configuration[..configuration_len]`.
    pub configuration: [u8; CONFIGURATION_CAPACITY],
    pub configuration_len: usize,
    /// The packet size of the device's control endpoint.
    pub max_packet0: u16,
}

fn hcd_speed(speed: Speed) -> HcdSpeed {
    match speed {
        Speed::Low => HcdSpeed::Low,
        Speed::Full => HcdSpeed::Full,
        Speed::High => HcdSpeed::High,
    }
}

/// Open a control pipe to the endpoint 0 of the device at `address`.
fn open_control(
    pipes: &mut Pipes<'_>,
    address: u8,
    speed: Speed,
    max_packet: u16,
    route: Option<Route>,
) -> Result<Pipe, EnumerateError> {
    let pipe = pipes
        .open(
            u32::from(address),
            0,
            hcd_speed(speed),
            TransferKind::Control,
            max_packet,
        )
        .map_err(EnumerateError::Pipe)?;
    if let Some(route) = route
        && let Err(error) = pipe
            .client
            .set_route(u32::from(route.hub_address), u32::from(route.hub_port))
    {
        pipes.close(&pipe);
        return Err(EnumerateError::Route(error));
    }
    Ok(pipe)
}

/// Enumerate the device behind a port to `address`.
///
/// `reset` resets the port and returns the speed of the device; the
/// enumeration asks for it where the state machine does. A device behind a hub
/// that is already reset returns its speed at once. `route` is the hub and port
/// of a low- or full-speed device behind a high-speed hub. `sleep` waits that
/// many milliseconds. `observe` sees every control transfer with its result,
/// for tracing.
///
/// # Errors
///
/// [`EnumerateError`] for a refused pipe or a device that does not enumerate.
///
/// # Panics
///
/// When `address` is not a valid device address.
pub fn enumerate(
    pipes: &mut Pipes<'_>,
    address: u8,
    route: Option<Route>,
    mut reset: impl FnMut() -> Result<Speed, EnumerateError>,
    mut sleep: impl FnMut(u32),
    mut observe: impl FnMut(SetupPacket, &Result<usize, PipeError>),
) -> Result<Enumerated, EnumerateError> {
    let mut enumerator = Enumerator::new(address).expect("a valid address");
    let mut current = 0;
    let mut max_packet = 8;
    let mut speed = Speed::Full;
    let mut device = [0; 18];
    let mut configuration = [0; CONFIGURATION_CAPACITY];
    let mut configuration_len = 0;
    let mut reply = [0; CONFIGURATION_CAPACITY];
    // The pipe to the device as it is addressed now, with the packet size it
    // is configured for.
    let mut pipe = None::<(Pipe, u8, u16)>;
    let mut last_error = None;
    let mut step = enumerator.next(Event::Start);
    let result = loop {
        step = match step {
            Step::ResetPort => match reset() {
                Ok(reset_speed) => {
                    speed = reset_speed;
                    enumerator.next(Event::PortReset(speed))
                }
                Err(error) => break Err(error),
            },
            Step::Delay { milliseconds } => {
                sleep(milliseconds);
                enumerator.next(Event::Delayed)
            }
            Step::Control { setup } => {
                // Configure the pipe again when the device's packet size was
                // learned, and reopen it when the device's address changed.
                if let Some((open, open_address, open_packet)) = pipe.as_mut()
                    && *open_address == current
                    && *open_packet != max_packet
                {
                    if let Err(error) = open.client.configure(TransferKind::Control, max_packet, 0)
                    {
                        break Err(EnumerateError::Pipe(OpenError::Configure(error)));
                    }
                    *open_packet = max_packet;
                }
                if pipe
                    .as_ref()
                    .is_none_or(|(_, open_address, _)| *open_address != current)
                {
                    if let Some((old, _, _)) = pipe.take() {
                        pipes.close(&old);
                    }
                    match open_control(pipes, current, speed, max_packet, route) {
                        Ok(opened) => pipe = Some((opened, current, max_packet)),
                        Err(error) => break Err(error),
                    }
                }
                let (open, _, _) = pipe.as_ref().expect("a pipe is open");
                // A low-speed control endpoint has 8-byte packets (USB 2.0 §5.5.3),
                // so the first 8 bytes of the device descriptor are one full
                // packet and the data stage would end early. Read the whole
                // descriptor from such a device instead.
                let first_read =
                    setup.value >> 8 == u16::from(DESCRIPTOR_DEVICE) && setup.length == 8;
                let mut setup = setup;
                if first_read && speed == Speed::Low {
                    setup.length = 18;
                }
                let length = usize::from(setup.length).min(reply.len());
                let data = reply.get_mut(..length).unwrap_or(&mut []);
                let result = pipes.control(open, setup, &[], data);
                observe(setup, &result);
                match result {
                    Ok(moved) => {
                        let got = reply.get(..moved).unwrap_or_default();
                        let descriptor_type = setup.value >> 8;
                        if setup.request == REQUEST_SET_ADDRESS {
                            current = u8::try_from(setup.value).expect("a 7-bit address");
                        }
                        if descriptor_type == u16::from(DESCRIPTOR_DEVICE) {
                            if first_read {
                                max_packet = u16::from(got.get(7).copied().unwrap_or(8));
                            } else if let Some(copy) = device.get_mut(..got.len()) {
                                copy.copy_from_slice(got);
                            }
                        }
                        if descriptor_type == u16::from(DESCRIPTOR_CONFIGURATION)
                            && setup.length > 9
                            && let Some(copy) = configuration.get_mut(..got.len())
                        {
                            copy.copy_from_slice(got);
                            configuration_len = got.len();
                        }
                        enumerator.next(Event::Control(got))
                    }
                    Err(PipeError::Exception(PipeException::Stalled)) => {
                        last_error = Some(PipeError::Exception(PipeException::Stalled));
                        enumerator.next(Event::ControlFailed(Failure::Stalled))
                    }
                    Err(error) => {
                        last_error = Some(error);
                        enumerator.next(Event::ControlFailed(Failure::Transfer))
                    }
                }
            }
            Step::Configured { .. } => {
                break Ok(Enumerated {
                    address,
                    speed,
                    device,
                    configuration,
                    configuration_len,
                    max_packet0: max_packet,
                });
            }
            Step::Failed(failure) => {
                break Err(EnumerateError::Failed {
                    failure,
                    transfer: last_error.take(),
                });
            }
        };
    };
    if let Some((open, _, _)) = pipe {
        pipes.close(&open);
    }
    result
}

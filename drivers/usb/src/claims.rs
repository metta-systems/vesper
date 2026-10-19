//! Interfaces on offer and the claims on them.
//!
//! The core offers every interface of every enumerated device. A class driver
//! claims one by its class codes; the claim is theirs until they release it or
//! the device goes away. Claims are told apart by the badge of the key the
//! caller used, so one client cannot use or release another's claim.

use vesper_usb_protocol::{
    descriptor::{Descriptor, Endpoint, TransferKind, walk},
    enumeration::Speed,
};

/// Interfaces the core offers at once.
pub const MAX_OFFERS: usize = 16;

/// Claims held at once.
pub const MAX_CLAIMS: usize = 8;

/// The transaction translator route of a low- or full-speed device behind a
/// high-speed hub.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HubRoute {
    pub hub: u8,
    pub port: u8,
}

/// The interrupt-IN endpoint of an interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InterruptEndpoint {
    pub address: u8,
    pub max_packet: u16,
    pub interval: u8,
}

/// An interface of an attached device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offer {
    /// Address of the device.
    pub device: u8,
    pub speed: Speed,
    pub route: Option<HubRoute>,
    pub number: u8,
    pub class_code: u8,
    pub subclass: u8,
    pub protocol: u8,
    pub interrupt_in: Option<InterruptEndpoint>,
}

/// The interfaces of the device at `device` in `configuration`, a
/// configuration descriptor with the descriptors that follow it. An alternate
/// setting other than 0 is not offered.
pub fn offers_of<'a>(
    device: u8,
    speed: Speed,
    route: Option<HubRoute>,
    configuration: &'a [u8],
) -> impl Iterator<Item = Offer> + 'a {
    let mut current: Option<Offer> = None;
    let mut walker = walk(configuration);
    let mut finished = false;
    core::iter::from_fn(move || {
        if finished {
            return None;
        }
        loop {
            match walker.next() {
                Some(Ok(Descriptor::Interface(interface))) => {
                    let started = (interface.alternate == 0).then_some(Offer {
                        device,
                        speed,
                        route,
                        number: interface.number,
                        class_code: interface.class_code,
                        subclass: interface.subclass,
                        protocol: interface.protocol,
                        interrupt_in: None,
                    });
                    if let Some(done) = core::mem::replace(&mut current, started) {
                        return Some(done);
                    }
                }
                Some(Ok(Descriptor::Endpoint(endpoint))) => {
                    if let Some(offer) = current.as_mut()
                        && offer.interrupt_in.is_none()
                        && let Some(found) = interrupt_in(endpoint)
                    {
                        offer.interrupt_in = Some(found);
                    }
                }
                Some(Ok(_)) => {}
                Some(Err(_)) | None => {
                    finished = true;
                    return current.take();
                }
            }
        }
    })
}

fn interrupt_in(endpoint: Endpoint) -> Option<InterruptEndpoint> {
    (endpoint.is_in() && endpoint.transfer_kind() == TransferKind::Interrupt).then_some(
        InterruptEndpoint {
            address: endpoint.address,
            max_packet: endpoint.max_packet,
            interval: endpoint.interval,
        },
    )
}

/// What a claimant needs to open the interrupt pipe of its interface with
/// `usb_hcd_v1`. It travels in the claimant's data area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PipeParams {
    pub device: u8,
    pub endpoint: u8,
    pub speed: Speed,
    pub interval: u8,
    pub max_packet: u16,
    pub route: Option<HubRoute>,
}

impl PipeParams {
    /// Bytes of the encoded record.
    pub const BYTES: usize = 10;

    /// The record: device address, endpoint address, speed (0 low, 1 full,
    /// 2 high), interval, packet size (little endian), a route flag, the hub's
    /// address and port.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; Self::BYTES] {
        let speed = match self.speed {
            Speed::Low => 0,
            Speed::Full => 1,
            Speed::High => 2,
        };
        let (flag, hub, port) = match self.route {
            Some(route) => (1, route.hub, route.port),
            None => (0, 0, 0),
        };
        let packet = self.max_packet.to_le_bytes();
        [
            self.device,
            self.endpoint,
            speed,
            self.interval,
            packet[0],
            packet[1],
            flag,
            hub,
            port,
            0,
        ]
    }

    /// Decode a record; `None` for a short or malformed one.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let [
            device,
            endpoint,
            speed,
            interval,
            low,
            high,
            flag,
            hub,
            port,
            0,
        ] = *bytes.get(..Self::BYTES)?
        else {
            return None;
        };
        let speed = match speed {
            0 => Speed::Low,
            1 => Speed::Full,
            2 => Speed::High,
            _ => return None,
        };
        let route = match flag {
            0 => None,
            1 => Some(HubRoute { hub, port }),
            _ => return None,
        };
        Some(Self {
            device,
            endpoint,
            speed,
            interval,
            max_packet: u16::from_le_bytes([low, high]),
            route,
        })
    }
}

/// Why a claim was not made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimError {
    /// No unclaimed interface with those class codes is attached (yet).
    NotReady,
    /// The claim table is full.
    Full,
}

/// Why a claim could not be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseError {
    /// No such claim, or it belongs to another client.
    NoClaim,
    /// The device was unplugged.
    DeviceGone,
    /// The interface has no interrupt-IN endpoint.
    NoEndpoint,
}

#[derive(Clone, Copy, Debug)]
struct Claim {
    owner: u64,
    generation: u32,
    offer: Offer,
    gone: bool,
}

/// The interfaces on offer and the claims on them.
#[derive(Clone, Copy, Debug)]
pub struct Claims {
    offers: [Option<(Offer, bool)>; MAX_OFFERS],
    held: [Option<Claim>; MAX_CLAIMS],
    /// Generation the next claim in each slot takes.
    next_generation: [u32; MAX_CLAIMS],
}

impl Claims {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            offers: [None; MAX_OFFERS],
            held: [None; MAX_CLAIMS],
            next_generation: [1; MAX_CLAIMS],
        }
    }

    /// Offer an interface. Returns `false` when the table is full.
    pub fn offer(&mut self, offer: Offer) -> bool {
        match self.offers.iter_mut().find(|slot| slot.is_none()) {
            Some(slot) => {
                *slot = Some((offer, false));
                true
            }
            None => false,
        }
    }

    /// Withdraw every interface of the device at `device`; claims on them are
    /// marked gone and stay until released.
    pub fn withdraw(&mut self, device: u8) {
        for slot in &mut self.offers {
            if slot.is_some_and(|(offer, _)| offer.device == device) {
                *slot = None;
            }
        }
        for claim in self.held.iter_mut().flatten() {
            if claim.offer.device == device {
                claim.gone = true;
            }
        }
    }

    /// Claim the first unclaimed interface whose class codes match; `owner`
    /// is the badge of the caller's key. The handle holds the claim's slot in
    /// bits 7:0 and its generation above.
    ///
    /// # Errors
    ///
    /// [`ClaimError::NotReady`] when no such interface is attached,
    /// [`ClaimError::Full`] when the table has no room.
    pub fn claim(
        &mut self,
        owner: u64,
        class_code: u8,
        subclass: u8,
        protocol: u8,
    ) -> Result<u32, ClaimError> {
        let found = self.offers.iter().position(|slot| {
            slot.is_some_and(|(offer, claimed)| {
                !claimed
                    && offer.class_code == class_code
                    && offer.subclass == subclass
                    && offer.protocol == protocol
            })
        });
        let Some(index) = found else {
            return Err(ClaimError::NotReady);
        };
        let Some(slot) = self.held.iter().position(Option::is_none) else {
            return Err(ClaimError::Full);
        };
        let Some((offer, claimed)) = self.offers[index].as_mut() else {
            return Err(ClaimError::NotReady);
        };
        *claimed = true;
        let generation = self.next_generation[slot];
        self.next_generation[slot] = generation.wrapping_add(1).max(1);
        self.held[slot] = Some(Claim {
            owner,
            generation,
            offer: *offer,
            gone: false,
        });
        // The slot index is below `MAX_CLAIMS`; the generation uses 24 bits.
        let slot = u32::try_from(slot).unwrap_or(0);
        Ok(slot | ((generation & 0x00ff_ffff) << 8))
    }

    fn lookup(&self, owner: u64, handle: u32) -> Option<(usize, &Claim)> {
        let slot = usize::try_from(handle & 0xff).ok()?;
        let claim = self.held.get(slot)?.as_ref()?;
        (claim.owner == owner && claim.generation & 0x00ff_ffff == handle >> 8)
            .then_some((slot, claim))
    }

    /// Release a claim; the interface can be claimed again if its device is
    /// still attached.
    ///
    /// # Errors
    ///
    /// [`UseError::NoClaim`] for an unknown claim or another client's.
    pub fn release(&mut self, owner: u64, handle: u32) -> Result<(), UseError> {
        let Some((slot, claim)) = self.lookup(owner, handle) else {
            return Err(UseError::NoClaim);
        };
        let offer = claim.offer;
        self.held[slot] = None;
        for entry in self.offers.iter_mut().flatten() {
            if entry.0 == offer {
                entry.1 = false;
            }
        }
        Ok(())
    }

    /// Whether the claim is the owner's and its device still attached.
    ///
    /// # Errors
    ///
    /// [`UseError::NoClaim`] or [`UseError::DeviceGone`].
    pub fn check(&self, owner: u64, handle: u32) -> Result<(), UseError> {
        let (_, claim) = self.lookup(owner, handle).ok_or(UseError::NoClaim)?;
        if claim.gone {
            Err(UseError::DeviceGone)
        } else {
            Ok(())
        }
    }

    /// What the claimant needs to open its interface's interrupt-IN pipe.
    ///
    /// # Errors
    ///
    /// [`UseError::NoClaim`], [`UseError::DeviceGone`] once the device was
    /// unplugged, [`UseError::NoEndpoint`] for an interface without an
    /// interrupt-IN endpoint.
    pub fn interrupt_pipe(&self, owner: u64, handle: u32) -> Result<PipeParams, UseError> {
        let (_, claim) = self.lookup(owner, handle).ok_or(UseError::NoClaim)?;
        if claim.gone {
            return Err(UseError::DeviceGone);
        }
        let endpoint = claim.offer.interrupt_in.ok_or(UseError::NoEndpoint)?;
        Ok(PipeParams {
            device: claim.offer.device,
            endpoint: endpoint.address,
            speed: claim.offer.speed,
            interval: endpoint.interval,
            max_packet: endpoint.max_packet,
            route: claim.offer.route,
        })
    }
}

impl Default for Claims {
    fn default() -> Self {
        Self::new()
    }
}

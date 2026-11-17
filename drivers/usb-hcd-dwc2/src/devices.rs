//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! The devices the controller gave an address (`usb_hcd_v1.address_device`):
//! each one's speed and the transaction translator its split transactions go
//! through, which a device behind it inherits. Nothing here touches hardware.

use {
    crate::pipes::Route,
    vesper_usb_protocol::{
        enumeration::{AddressPool, LAST_ADDRESS, Speed},
        hub::{TranslatorRoute, translator_route},
    },
};

/// The root ports the controller has: one, port 0.
pub const ROOT_PORTS: u8 = 1;

/// What the controller keeps of an addressed device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Device {
    pub speed: Speed,
    /// The translator a low- or full-speed device behind a high-speed hub is
    /// reached through.
    pub route: Option<Route>,
}

/// Why a device cannot be addressed where the caller says it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceError {
    /// The root port does not exist.
    NoPort,
    /// The hub was not addressed by this controller, or the port number is 0.
    UnknownHub,
}

/// The addressed devices, by address.
pub struct Devices {
    addresses: AddressPool,
    devices: [Option<Device>; LAST_ADDRESS as usize + 1],
}

impl Devices {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            addresses: AddressPool::new(),
            devices: [None; LAST_ADDRESS as usize + 1],
        }
    }

    /// The device at `address`.
    #[must_use]
    pub fn get(&self, address: u8) -> Option<&Device> {
        self.devices.get(usize::from(address))?.as_ref()
    }

    /// The device a `speed` device on port `port` of the hub at `hub_address`
    /// (root port `port` when `hub_address` is 0) becomes: its translator
    /// comes from the hub (USB 2.0 §11.14).
    ///
    /// # Errors
    ///
    /// [`PlaceError`] for a root port the controller does not have, or a hub
    /// it did not address.
    pub fn place(&self, hub_address: u8, port: u8, speed: Speed) -> Result<Device, PlaceError> {
        if hub_address == 0 {
            return if port < ROOT_PORTS {
                Ok(Device { speed, route: None })
            } else {
                Err(PlaceError::NoPort)
            };
        }
        let hub = self
            .get(hub_address)
            .filter(|_| port >= 1)
            .ok_or(PlaceError::UnknownHub)?;
        let hub_route = hub.route.map(|route| TranslatorRoute {
            hub_address: route.hub_address,
            hub_port: route.hub_port,
        });
        let route =
            translator_route(hub_address, hub.speed, hub_route, port, speed).map(|route| Route {
                hub_address: route.hub_address,
                hub_port: route.hub_port,
            });
        Ok(Device { speed, route })
    }

    /// Give `device` the lowest free address, or `None` when none is left.
    pub fn add(&mut self, device: Device) -> Option<u8> {
        let address = self.addresses.take()?;
        if let Some(slot) = self.devices.get_mut(usize::from(address)) {
            *slot = Some(device);
        }
        Some(address)
    }

    /// Forget the device at `address`; `false` when there is none.
    pub fn release(&mut self, address: u8) -> bool {
        let Some(slot) = self
            .devices
            .get_mut(usize::from(address))
            .filter(|slot| slot.is_some())
        else {
            return false;
        };
        *slot = None;
        self.addresses.release(address);
        true
    }
}

impl Default for Devices {
    fn default() -> Self {
        Self::new()
    }
}

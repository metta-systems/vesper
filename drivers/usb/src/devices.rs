//! Device addresses and the table of attached devices.

use vesper_usb_protocol::enumeration::{FIRST_ADDRESS, LAST_ADDRESS, Speed};

/// Devices the table holds: the root device, the hub's other ports and nested
/// hubs.
pub const MAX_DEVICES: usize = 16;

/// Hands out device addresses, lowest first, and takes them back.
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

/// Where a device is attached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attachment {
    /// The root port of the host controller.
    Root,
    /// Port `port` of the hub at address `hub`.
    Hub { hub: u8, port: u8 },
}

/// An enumerated device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Device {
    pub address: u8,
    pub speed: Speed,
    pub attachment: Attachment,
    pub class_code: u8,
    pub vendor: u16,
    pub product: u16,
}

/// The attached devices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceTable {
    slots: [Option<Device>; MAX_DEVICES],
}

impl DeviceTable {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [None; MAX_DEVICES],
        }
    }

    /// Record `device`. Returns `false` when the table is full or a device
    /// already has its address.
    pub fn add(&mut self, device: Device) -> bool {
        if self.find(device.address).is_some() {
            return false;
        }
        match self.slots.iter_mut().find(|slot| slot.is_none()) {
            Some(slot) => {
                *slot = Some(device);
                true
            }
            None => false,
        }
    }

    /// The device at `address`.
    #[must_use]
    pub fn find(&self, address: u8) -> Option<&Device> {
        self.slots
            .iter()
            .flatten()
            .find(|device| device.address == address)
    }

    /// The device on port `port` of hub `hub`.
    #[must_use]
    pub fn on_port(&self, hub: u8, port: u8) -> Option<&Device> {
        self.slots
            .iter()
            .flatten()
            .find(|device| device.attachment == Attachment::Hub { hub, port })
    }

    /// Forget the device on port `port` of hub `hub`, and every device behind
    /// it if it is a hub; returns the addresses to give back, in the order they
    /// were forgotten.
    pub fn remove_port(&mut self, hub: u8, port: u8) -> Removed {
        let mut removed = Removed::new();
        if let Some(address) = self.on_port(hub, port).map(|device| device.address) {
            self.remove_tree(address, &mut removed);
        }
        removed
    }

    /// Forget the device at `address` and the devices behind it.
    fn remove_tree(&mut self, address: u8, removed: &mut Removed) {
        for slot in &mut self.slots {
            if slot.is_some_and(|device| device.address == address) {
                *slot = None;
                removed.push(address);
            }
        }
        // A forgotten hub takes the devices on its ports with it.
        let mut behind = [0_u8; MAX_DEVICES];
        let mut count = 0;
        for device in self.slots.iter().flatten() {
            if matches!(device.attachment, Attachment::Hub { hub, .. } if hub == address)
                && let Some(entry) = behind.get_mut(count)
            {
                *entry = device.address;
                count += 1;
            }
        }
        for &child in &behind[..count] {
            self.remove_tree(child, removed);
        }
    }

    /// How many devices are attached.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    /// Whether no device is attached.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for DeviceTable {
    fn default() -> Self {
        Self::new()
    }
}

/// The addresses of the devices [`DeviceTable::remove_port`] forgot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Removed {
    addresses: [u8; MAX_DEVICES],
    count: usize,
}

impl Removed {
    const fn new() -> Self {
        Self {
            addresses: [0; MAX_DEVICES],
            count: 0,
        }
    }

    fn push(&mut self, address: u8) {
        if let Some(slot) = self.addresses.get_mut(self.count) {
            *slot = address;
            self.count += 1;
        }
    }

    /// The addresses, in the order the devices were forgotten.
    #[must_use]
    pub fn addresses(&self) -> &[u8] {
        &self.addresses[..self.count]
    }
}

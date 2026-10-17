//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! The 8-byte setup packet that starts every control transfer.

/// `bmRequestType` direction bit: device to host.
pub const DEVICE_TO_HOST: u8 = 0x80;

/// `bmRequestType` for a standard request to the device.
pub const STANDARD_DEVICE: u8 = 0x00;

/// `bRequest` of `GET_DESCRIPTOR`.
pub const REQUEST_GET_DESCRIPTOR: u8 = 6;
/// `bRequest` of `SET_ADDRESS`.
pub const REQUEST_SET_ADDRESS: u8 = 5;
/// `bRequest` of `SET_CONFIGURATION`.
pub const REQUEST_SET_CONFIGURATION: u8 = 9;

/// Descriptor type of a device descriptor.
pub const DESCRIPTOR_DEVICE: u8 = 1;
/// Descriptor type of a configuration descriptor.
pub const DESCRIPTOR_CONFIGURATION: u8 = 2;

/// A control transfer's setup stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetupPacket {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    /// Bytes of the data stage the request expects.
    pub length: u16,
}

impl SetupPacket {
    /// The packet's wire form: little-endian words, in the order the USB spec gives.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; 8] {
        let value = self.value.to_le_bytes();
        let index = self.index.to_le_bytes();
        let length = self.length.to_le_bytes();
        [
            self.request_type,
            self.request,
            value[0],
            value[1],
            index[0],
            index[1],
            length[0],
            length[1],
        ]
    }

    /// `GET_DESCRIPTOR` for the device (`descriptor_type` 1) or a configuration
    /// (`2`), asking for at most `length` bytes.
    #[must_use]
    pub const fn get_descriptor(descriptor_type: u8, index: u8, length: u16) -> Self {
        Self {
            request_type: DEVICE_TO_HOST | STANDARD_DEVICE,
            request: REQUEST_GET_DESCRIPTOR,
            value: u16::from_le_bytes([index, descriptor_type]),
            index: 0,
            length,
        }
    }

    /// `SET_ADDRESS`, giving the device `address` for all later transfers.
    #[must_use]
    pub const fn set_address(address: u8) -> Self {
        Self {
            request_type: STANDARD_DEVICE,
            request: REQUEST_SET_ADDRESS,
            value: u16::from_le_bytes([address, 0]),
            index: 0,
            length: 0,
        }
    }

    /// `SET_CONFIGURATION`, activating configuration `value`.
    #[must_use]
    pub const fn set_configuration(value: u8) -> Self {
        Self {
            request_type: STANDARD_DEVICE,
            request: REQUEST_SET_CONFIGURATION,
            value: u16::from_le_bytes([value, 0]),
            index: 0,
            length: 0,
        }
    }
}

//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! Pure parts of the USB HID component: turning boot reports into input
//! events, the devices the component serves and their descriptors. Nothing
//! here touches hardware, so every part runs as a host test.

#![no_std]

pub mod channel;
pub mod decode;
pub mod devices;

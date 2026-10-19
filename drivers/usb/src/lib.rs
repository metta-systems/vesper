//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! Pure parts of the USB core component: device addresses and the table of
//! attached devices. Nothing here touches hardware, so every part runs as a host
//! test.

#![no_std]

pub mod claims;
pub mod devices;

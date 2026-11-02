//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! Pure USB protocol logic: setup packets, descriptor parsing, the enumeration
//! state machine and HID boot-protocol reports. Nothing here touches hardware
//! or allocates, so every part runs as a host test.

#![no_std]

pub mod descriptor;
pub mod enumeration;
pub mod hid_boot;
pub mod hub;
pub mod report_channel;
pub mod setup;

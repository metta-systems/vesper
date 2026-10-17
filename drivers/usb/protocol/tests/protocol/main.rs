//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! Host tests of the USB protocol logic: setup packets, descriptors, the
//! enumeration state machine and HID boot reports.

#[cfg(test)]
mod descriptor;
#[cfg(test)]
mod enumeration;
#[cfg(test)]
mod hid_boot;
#[cfg(test)]
mod setup;

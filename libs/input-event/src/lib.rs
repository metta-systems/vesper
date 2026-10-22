//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! The input event contract (`doc/agentic/input-design.md`): the 24-byte event
//! record that producers (USB HID) and consumers (console, compositor) exchange
//! through an io-channel, HID usage constants for the pages in use, the device
//! descriptor layout, and a state tracker that folds events into key and axis
//! state.
//!
//! Everything here is pure: no I/O, no allocation. Records and descriptors are
//! encoded field by field, so the wire format never depends on Rust's struct
//! layout. A consumer decodes every record it receives, because a producer is
//! untrusted.

#![no_std]

pub mod descriptor;
pub mod record;
pub mod state;
pub mod usage;

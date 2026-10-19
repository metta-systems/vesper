//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! Pure compositor logic: the windows and their stacking order, and the router
//! that turns the frames of an input producer into events for windows. Nothing
//! here touches hardware or allocates, so every part runs as a host test.

#![no_std]

pub mod router;
pub mod windows;

//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! Pure compositor logic: the windows and their stacking order, the router
//! that turns the frames of an input producer into events for windows, the
//! event channel that carries them to each client, the damage set and the
//! drawing of the screen from layers. Nothing here touches hardware or
//! allocates, so every part runs as a host test.

#![no_std]

pub mod blit;
pub mod cursor;
pub mod damage;
pub mod events;
pub mod router;
pub mod surface;
pub mod windows;

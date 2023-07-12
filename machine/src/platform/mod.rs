/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

pub mod rpi3;

#[cfg(any(feature = "rpi3", feature = "rpi4"))]
pub use rpi3::*;

/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

#[cfg(any(feature = "rpi3", feature = "rpi4"))]
pub mod raspberrypi;

#[cfg(any(feature = "rpi3", feature = "rpi4"))]
pub use raspberrypi::*;

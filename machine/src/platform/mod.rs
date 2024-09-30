// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
// SPDX-FileCopyrightText: 2024 Metta Systems OÜ
// SPDX-FileContributor: Berkus
//
// SPDX-License-Identifier: BlueOak-1.0.0

#[cfg(any(feature = "rpi3", feature = "rpi4"))]
pub mod raspberrypi;

#[cfg(any(feature = "rpi3", feature = "rpi4"))]
pub use raspberrypi::*;

// Copyright (c) 2018-2022 Andre Richter <andre.o.richter@gmail.com>
// SPDX-FileCopyrightText: 2024 Metta Systems OÜ
// SPDX-FileContributor: Berkus
//
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Device driver.

#[cfg(feature = "rpi4")]
mod arm;
#[cfg(any(feature = "rpi3", feature = "rpi4"))]
mod bcm;

pub mod common;

#[cfg(feature = "rpi4")]
pub use arm::*;
#[cfg(any(feature = "rpi3", feature = "rpi4"))]
pub use bcm::*;

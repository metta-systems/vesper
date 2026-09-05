// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2018-2022 Andre Richter <andre.o.richter@gmail.com>

//! Device driver.

#[cfg(board_rpi4)]
pub mod arm;
#[cfg(any(board_rpi3, board_rpi4))]
pub mod bcm;

#[cfg(board_rpi4)]
pub use arm::*;
#[cfg(any(board_rpi3, board_rpi4))]
pub use bcm::*;

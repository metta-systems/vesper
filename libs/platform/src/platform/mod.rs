/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

#[cfg(any(board_rpi3, board_rpi4))]
pub mod raspberrypi;

#[cfg(any(board_rpi3, board_rpi4))]
pub use raspberrypi::*;

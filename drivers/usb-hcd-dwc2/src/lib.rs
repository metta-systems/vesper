//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! Pure parts of the `DesignWare` DWC2 USB host controller driver: the register
//! map and bit fields, and the stages of a control transfer. Nothing here
//! touches hardware, so every part runs as a host test.

#![no_std]

pub mod control;
pub mod regs;

//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! Pure parts of the `DesignWare` DWC2 USB host controller driver: the register
//! map and bit fields, the stages of a control transfer, the sequence of a split
//! transaction and the microframes its complete splits go in, and the table of
//! open pipes. Nothing here touches hardware, so every part runs as a host test.

#![no_std]

pub mod control;
pub mod microframe;
pub mod periodic;
pub mod pipes;
pub mod regs;
pub mod split;

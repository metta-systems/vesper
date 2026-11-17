//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! Pure parts of the xHCI host controller driver: the register map and bit
//! fields, the Transfer Request Block (TRB) format, the rings software and
//! the controller exchange TRBs on, the device contexts and the table of open
//! pipes. Nothing here
//! touches hardware, so every part runs as a host test. Section numbers are
//! those of the eXtensible Host Controller Interface for USB specification,
//! revision 1.2.

#![no_std]

pub mod context;
pub mod pipes;
pub mod regs;
pub mod ring;
pub mod trb;

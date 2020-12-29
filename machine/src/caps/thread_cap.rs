/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

use {
    super::{CapError, Capability, TryFrom},
    crate::{arch::memory::PhysAddr, capdef},
    paste::paste,
    register::{LocalRegisterCopy, register_bitfields},
};

//=====================
// Cap definition
//=====================

register_bitfields! {
    u128,
    ThreadCap [
        Type OFFSET(0) NUMBITS(6) [
            value = 12
        ],
        TCBPtr OFFSET(64) NUMBITS(48) [],
    ]
}

capdef! { Thread }

//=====================
// Cap implementation
//=====================

impl ThreadCapability {
    pub(crate) fn ptr(&self) -> PhysAddr {
        0.into() // @todo
    }
}

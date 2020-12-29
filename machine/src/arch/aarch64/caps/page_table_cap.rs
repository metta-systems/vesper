/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

use {
    crate::{
        capdef,
        caps::{CapError, Capability},
    },
    core::convert::TryFrom,
    paste::paste,
    register::{LocalRegisterCopy, register_bitfields},
};

//=====================
// Cap definition
//=====================

register_bitfields! {
    u128,
    PageTableCap [
        Type OFFSET(0) NUMBITS(6) [
            value = 3
        ],
        IsMapped OFFSET(6) NUMBITS(1) [],
        BasePtr OFFSET(16) NUMBITS(48) [], // PhysAddr
        MappedASID OFFSET(64) NUMBITS(16) [],
        MappedAddress OFFSET(80) NUMBITS(48) [], // VirtAddr
    ],
}

capdef! { PageTable }

//=====================
// Cap implementation
//=====================

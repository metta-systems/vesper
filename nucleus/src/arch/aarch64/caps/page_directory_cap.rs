/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

use {
    crate::{
        capdef,
        caps::{CapError, Capability},
    },
    core::convert::TryFrom,
    paste::paste,
    register::{register_bitfields, LocalRegisterCopy},
};

//=====================
// Cap definition
//=====================

register_bitfields! {
    u128,
    PageDirectoryCap [
        MappedASID OFFSET(0) NUMBITS(16) [],
        BasePtr OFFSET(16) NUMBITS(48) [], // PhysAddr
        Type OFFSET(64) NUMBITS(5) [
            value = 5
        ],
        IsMapped OFFSET(79) NUMBITS(1) [],
        MappedAddress OFFSET(80) NUMBITS(19) [] // VirtAddr
    ]
}

capdef! { PageDirectory }

//=====================
// Cap implementation
//=====================

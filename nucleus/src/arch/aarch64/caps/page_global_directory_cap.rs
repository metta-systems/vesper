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
    PageGlobalDirectoryCap [
        MappedASID OFFSET(0) NUMBITS(16) [],
        BasePtr OFFSET(16) NUMBITS(48) [], // PhysAddr
        Type OFFSET(64) NUMBITS(5) [
            value = 9
        ],
        IsMapped OFFSET(79) NUMBITS(1) []
    ]
}

capdef! { PageGlobalDirectory }

//=====================
// Cap implementation
//=====================

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
    FrameCap [
        MappedASID OFFSET(0) NUMBITS(16) [],
        BasePtr OFFSET(16) NUMBITS(48) [], // PhysAddr
        Type OFFSET(64) NUMBITS(5) [
            value = 1
        ],
        Size OFFSET(69) NUMBITS(2) [],
        VMRights OFFSET(71) NUMBITS(2) [],
        IsDevice OFFSET(73) NUMBITS(1) [],
        MappedAddress OFFSET(80) NUMBITS(48) [] // VirtAddr
    ]
}

capdef! { Frame }

//=====================
// Cap implementation
//=====================

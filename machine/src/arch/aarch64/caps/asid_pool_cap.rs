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
    AsidPoolCap [
        Type OFFSET(64) NUMBITS(5) [
            value = 13
        ],
        ASIDBase OFFSET(69) NUMBITS(16) [],
        ASIDPool OFFSET(91) NUMBITS(37) []
    ]
}

capdef! { AsidPool }

//=====================
// Cap implementation
//=====================

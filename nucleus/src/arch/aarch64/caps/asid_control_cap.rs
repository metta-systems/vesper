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
    AsidControlCap [
        Type OFFSET(0) NUMBITS(6) [
            value = 11
        ]
    ]
}

capdef! { AsidControl }

//=====================
// Cap implementation
//=====================

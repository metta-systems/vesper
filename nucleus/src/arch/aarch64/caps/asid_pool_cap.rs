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
    register::{register_bitfields, LocalRegisterCopy},
};

//=====================
// Cap definition
//=====================

register_bitfields! {
    u128,
    AsidPoolCap [
        Type OFFSET(0) NUMBITS(6) [
            value = 13
        ],
        ASIDBase OFFSET(64) NUMBITS(16) [],
        ASIDPool OFFSET(80) NUMBITS(37) [],
    ]
}

capdef! { AsidPool }

//=====================
// Cap implementation
//=====================

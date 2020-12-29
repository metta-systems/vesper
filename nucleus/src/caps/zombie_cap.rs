/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

use {
    super::{CapError, Capability, TryFrom},
    crate::capdef,
    paste::paste,
    register::{register_bitfields, LocalRegisterCopy},
};

//=====================
// Cap definition
//=====================

register_bitfields! {
    u128,
    ZombieCap [
        Type OFFSET(0) NUMBITS(6) [
            value = 18
        ],
        ZombieType OFFSET(58) NUMBITS(6) [],
        ZombieID OFFSET(64) NUMBITS(64) [],
    ]
}

capdef! { Zombie }

//=====================
// Cap implementation
//=====================

/*
 * SPDX-License-Identifier: BlueOak-1.0.0
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
        ZombieID OFFSET(0) NUMBITS(64) [],
        Type OFFSET(64) NUMBITS(5) [
            value = 18
        ],
        ZombieType OFFSET(121) NUMBITS(7) []
    ]
}

capdef! { Zombie }

//=====================
// Cap implementation
//=====================

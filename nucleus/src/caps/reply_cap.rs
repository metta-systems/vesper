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
    ReplyCap [
        Type OFFSET(0) NUMBITS(6) [
            value = 8
        ],
        ReplyCanGrant OFFSET(62) NUMBITS(1) [],
        ReplyMaster OFFSET(63) NUMBITS(1) [],
        TCBPtr OFFSET(64) NUMBITS(64) [],
    ]
}

capdef! { Reply }

//=====================
// Cap implementation
//=====================

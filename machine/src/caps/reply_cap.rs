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
        TCBPtr OFFSET(0) NUMBITS(64) [],
        Type OFFSET(64) NUMBITS(5) [
            value = 8
        ],
        ReplyCanGrant OFFSET(126) NUMBITS(1) [],
        ReplyMaster OFFSET(127) NUMBITS(1) [],
    ]
}

capdef! { Reply }

//=====================
// Cap implementation
//=====================

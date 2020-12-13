/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

//! @todo replace with Event

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
    NotificationCap [
        Badge OFFSET(0) NUMBITS(64) [],
        Type OFFSET(64) NUMBITS(5) [
            value = 6
        ],
        CanReceive OFFSET(69) NUMBITS(1) [],
        CanSend OFFSET(70) NUMBITS(1) [],
        Ptr OFFSET(80) NUMBITS(48) [],
    ]
}

capdef! { Notification }

//=====================
// Cap implementation
//=====================

// Notifications support NBSend (Signal), Wait and NBWait (Poll) (see COMP9242 slides by Gernot)
// Other objects support only Call() (see COMP9242 slides by Gernot)
// Appear as (kernel-implemented) servers
//     • Each has a kernel-defined protocol
//         • operations encoded in message tag
//         • parameters passed in message words
//     • Mostly hidden behind “syscall” wrappers
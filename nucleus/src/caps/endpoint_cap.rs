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
    EndpointCap [
        Badge OFFSET(0) NUMBITS(64) [],
        Type OFFSET(64) NUMBITS(5) [
            value = 4
        ],
        CanGrantReply OFFSET(69) NUMBITS(1) [],
        CanGrant OFFSET(70) NUMBITS(1) [],
        CanReceive OFFSET(71) NUMBITS(1) [],
        CanSend OFFSET(72) NUMBITS(1) [],
        Ptr OFFSET(80) NUMBITS(48) [],
    ]
}

capdef! { Endpoint }

//=====================
// Cap implementation
//=====================

// Endpoints support all 10 IPC variants (see COMP9242 slides by Gernot)
impl EndpointCapability {}

/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

use {
    super::{CapError, Capability, TryFrom},
    crate::capdef,
    paste::paste,
    register::{LocalRegisterCopy, register_bitfields},
};

//=====================
// Cap definition
//=====================

register_bitfields! {
    u128,
    EndpointCap [
        Type OFFSET(0) NUMBITS(6) [
            value = 4
        ],
        CanGrantReply OFFSET(6) NUMBITS(1) [],
        CanGrant OFFSET(7) NUMBITS(1) [],
        CanReceive OFFSET(8) NUMBITS(1) [],
        CanSend OFFSET(9) NUMBITS(1) [],
        Ptr OFFSET(16) NUMBITS(48) [],
        // @todo Badge has 4 lower bits all-zero - why?
        Badge OFFSET(64) NUMBITS(64) [],
    ]
}

capdef! { Endpoint }

//=====================
// Cap implementation
//=====================

// Endpoints support all 10 IPC variants (see COMP9242 slides by Gernot)
impl EndpointCapability {}

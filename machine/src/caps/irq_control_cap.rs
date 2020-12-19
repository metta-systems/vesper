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
    IrqControlCap [
        Type OFFSET(0) NUMBITS(6) [
            value = 14
        ]
    ]
}

capdef! { IrqControl }

//=====================
// Cap implementation
//=====================

impl IrqControlCapability {}

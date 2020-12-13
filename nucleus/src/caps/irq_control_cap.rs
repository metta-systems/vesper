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
    IrqControlCap [
        Type OFFSET(64) NUMBITS(5) [
            value = 14
        ]
    ]
}

capdef! { IrqControl }

//=====================
// Cap implementation
//=====================

impl IrqControlCapability {}

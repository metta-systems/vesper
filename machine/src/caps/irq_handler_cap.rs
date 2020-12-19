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
    IrqHandlerCap [
        Type OFFSET(0) NUMBITS(6) [
            value = 16
        ]
        Irq OFFSET(52) NUMBITS(12) [],
    ]
}

capdef! { IrqHandler }

//=====================
// Cap implementation
//=====================

impl IrqHandlerCapability {}

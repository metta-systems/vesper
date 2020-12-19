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
    NullCap [
        Type OFFSET(0) NUMBITS(6) [
            value = 0
        ]
    ]
}

capdef! { Null }

//=====================
// Cap implementation
//=====================

impl NullCapability {
    /// Create a Null capability.
    ///
    /// Such capabilities are invalid and can not be used for anything.
    pub fn new() -> NullCapability {
        NullCapability(LocalRegisterCopy::new(u128::from(NullCap::Type::value)))
    }
}

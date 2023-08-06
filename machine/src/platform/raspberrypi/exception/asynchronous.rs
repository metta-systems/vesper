// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2020-2022 Andre Richter <andre.o.richter@gmail.com>

//! Platform asynchronous exception handling.

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// Export for reuse in generic asynchronous.rs.
pub use crate::platform::device_driver::IRQNumber;

#[cfg(feature = "rpi3")]
pub(in crate::platform) mod irq_map {
    use crate::platform::device_driver::{IRQNumber, PeripheralIRQ};

    pub const PL011_UART: IRQNumber = IRQNumber::Peripheral(PeripheralIRQ::new(57));
}

#[cfg(feature = "rpi4")]
pub(in crate::platform) mod irq_map {
    use crate::platform::device_driver::IRQNumber;

    pub const PL011_UART: IRQNumber = IRQNumber::new(153);
}

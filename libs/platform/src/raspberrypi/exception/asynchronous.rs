// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2020-2022 Andre Richter <andre.o.richter@gmail.com>

//! Platform asynchronous exception handling.

use {
    libexception::asynchronous::{IRQContext, IRQHandlerDescriptor, interface::IRQManager},
    liblocking::InitStateLock,
};

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// Export for reuse in generic asynchronous.rs.
pub use crate::device_driver::IRQNumber; // @todo

#[cfg(board_rpi3)]
pub(crate) mod irq_map {
    use crate::device_driver::{IRQNumber, PeripheralIRQ};

    pub const PL011_UART: IRQNumber = IRQNumber::Peripheral(PeripheralIRQ::new(57));
}

#[cfg(board_rpi4)]
pub(crate) mod irq_map {
    use crate::device_driver::IRQNumber;

    pub const PL011_UART: IRQNumber = IRQNumber::new(153);
}

impl IRQManager for NullIRQManager {
    type IRQNumberType = IRQNumber;

    fn register_handler(
        &self,
        _descriptor: IRQHandlerDescriptor<Self::IRQNumberType>,
    ) -> Result<(), &'static str> {
        panic!("No IRQ Manager registered yet");
    }

    fn enable(&self, _irq_number: Self::IRQNumberType) {
        panic!("No IRQ Manager registered yet");
    }

    fn handle_pending_irqs<'irq_context>(&'irq_context self, _ic: &IRQContext<'irq_context>) {
        panic!("No IRQ Manager registered yet");
    }
}

/// Register a new IRQ manager.
pub fn register_irq_manager(
    new_manager: &'static (dyn IRQManager<IRQNumberType = IRQNumber> + Sync),
) {
    use liblocking::interface::ReadWriteEx;
    IRQ_MANAGER.write(|manager| *manager = new_manager);
}

/// Return a reference to the currently registered IRQ manager.
///
/// This is the IRQ manager used by the architectural interrupt handling code.
pub fn irq_manager() -> &'static dyn IRQManager<IRQNumberType = IRQNumber> {
    use liblocking::interface::ReadWriteEx;
    IRQ_MANAGER.read(|manager| *manager)
}

//--------------------------------------------------------------------------------------------------
// Global instances
//--------------------------------------------------------------------------------------------------

static IRQ_MANAGER: InitStateLock<&'static (dyn IRQManager<IRQNumberType = IRQNumber> + Sync)> =
    InitStateLock::new(&NULL_IRQ_MANAGER);

struct NullIRQManager;

static NULL_IRQ_MANAGER: NullIRQManager = NullIRQManager;

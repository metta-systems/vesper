// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2020-2022 Andre Richter <andre.o.richter@gmail.com>

//! Peripheral Interrupt Controller Driver.
//!
//! # Resources
//!
//! - <https://github.com/raspberrypi/documentation/files/1888662/BCM2837-ARM-Peripherals.-.Revised.-.V2-1.pdf>

use {
    super::{PendingIRQs, PeripheralIRQ},
    derive_mmio::Mmio,
    libexception::asynchronous::{IRQContext, IRQHandlerDescriptor, interface::IRQManager},
    liblocking::{self, IRQSafeNullLock, InitStateLock},
    libmmio::assert_register_offsets,
};

//--------------------------------------------------------------------------------------------------
// Private Definitions
//--------------------------------------------------------------------------------------------------

/// The peripheral interrupt registers of the ARM-control block.
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
struct RegisterBlock {
    _basic_pending: u32,
    #[mmio(PureRead)]
    pending_1: u32,
    #[mmio(PureRead)]
    pending_2: u32,
    _fiq_control: u32,
    #[mmio(Write)]
    enable_1: u32,
    #[mmio(Write)]
    enable_2: u32,
}

assert_register_offsets!(RegisterBlock, 0x18, {
    pending_1: 0x04,
    pending_2: 0x08,
    enable_1: 0x10,
    enable_2: 0x14,
});

type HandlerTable = [Option<IRQHandlerDescriptor<PeripheralIRQ>>; PeripheralIRQ::MAX_INCLUSIVE + 1];

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// Representation of the peripheral interrupt controller.
pub struct PeripheralIC {
    /// Access to the registers is guarded with a lock.
    registers: IRQSafeNullLock<MmioRegisterBlock<'static>>,

    /// Stores registered IRQ handlers. Writable only during kernel init. RO afterwards.
    handler_table: InitStateLock<HandlerTable>,
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

impl PeripheralIC {
    /// Create an instance.
    ///
    /// # Safety
    ///
    /// - The user must ensure to provide a correct MMIO start address.
    pub const unsafe fn new(mmio_start_addr: Address<Virtual>) -> Self {
        Self {
            // SAFETY: the caller's contract.
            registers: IRQSafeNullLock::new(unsafe {
                RegisterBlock::new_mmio(mmio_start_addr.as_mut_ptr())
            }),
            handler_table: InitStateLock::new([None; PeripheralIRQ::MAX_INCLUSIVE + 1]),
        }
    }

    /// Query the list of pending IRQs.
    fn pending_irqs(&self) -> PendingIRQs {
        let pending_mask: u64 = self.registers.lock(|registers| {
            (u64::from(registers.read_pending_2()) << 32) | u64::from(registers.read_pending_1())
        });

        PendingIRQs::new(pending_mask)
    }
}

//------------------------------------------------------------------------------
// OS Interface Code
//------------------------------------------------------------------------------
use {
    libaddress::{Address, Virtual},
    liblocking::interface::{Mutex, ReadWriteEx},
};

impl IRQManager for PeripheralIC {
    type IRQNumberType = PeripheralIRQ;

    fn register_handler(
        &self,
        irq_handler_descriptor: IRQHandlerDescriptor<Self::IRQNumberType>,
    ) -> Result<(), &'static str> {
        self.handler_table.write(|table| {
            let irq_number = irq_handler_descriptor.number().get();

            if table[irq_number].is_some() {
                return Err("IRQ handler already registered");
            }

            table[irq_number] = Some(irq_handler_descriptor);

            Ok(())
        })
    }

    fn enable(&self, irq: Self::IRQNumberType) {
        self.registers.lock(|registers| {
            let enable_bit: u32 = 1 << (irq.get() % 32);

            // Writing a 1 to a bit will set the corresponding IRQ enable bit. All other IRQ enable
            // bits are unaffected. So we don't need read and OR'ing here.
            if irq.get() <= 31 {
                registers.write_enable_1(enable_bit);
            } else {
                registers.write_enable_2(enable_bit);
            }
        });
    }

    fn handle_pending_irqs<'irq_context>(&'irq_context self, _ic: &IRQContext<'irq_context>) {
        self.handler_table.read(|table| {
            for irq_number in self.pending_irqs() {
                match table[irq_number] {
                    None => panic!("No handler registered for IRQ {irq_number}"),
                    Some(descriptor) => {
                        // Call the IRQ handler. Panics on failure.
                        descriptor.handler().handle().expect("Error handling IRQ");
                    }
                }
            }
        });
    }

    fn print_handler(&self) {
        use liblog::info;

        info!("      Peripheral handler:");

        self.handler_table.read(|table| {
            for (i, opt) in table.iter().enumerate() {
                if let Some(handler) = opt {
                    info!("            {: >3}. {}", i, handler.name());
                }
            }
        });
    }
}

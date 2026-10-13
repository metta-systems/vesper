// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2020-2022 Andre Richter <andre.o.richter@gmail.com>

//! GICC Driver - GIC CPU interface.

use {
    arbitrary_int::u10,
    bitbybit::bitfield,
    derive_mmio::Mmio,
    libaddress::{Address, Virtual},
    liblocking::{IRQSafeNullLock, interface::Mutex},
    libmmio::assert_register_offsets,
};

//--------------------------------------------------------------------------------------------------
// Private Definitions
//--------------------------------------------------------------------------------------------------

/// CPU Interface Control Register
#[bitfield(u32, default = 0, debug)]
struct CpuInterfaceControl {
    #[bit(0, rw)]
    enable: bool,
}

/// Interrupt Priority Mask Register
#[bitfield(u32, default = 0, debug)]
struct PriorityMask {
    #[bits(0..=7, rw)]
    priority: u8,
}

/// Interrupt Acknowledge Register and End of Interrupt Register
#[bitfield(u32, default = 0, debug)]
struct InterruptId {
    #[bits(0..=9, rw)]
    id: u10,
}

#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
struct RegisterBlock {
    control: CpuInterfaceControl,
    priority_mask: PriorityMask,
    _binary_point: u32,
    /// Reading acknowledges the interrupt it returns.
    #[mmio(Read)]
    interrupt_acknowledge: InterruptId,
    #[mmio(Write)]
    end_of_interrupt: InterruptId,
}

assert_register_offsets!(RegisterBlock, 0x14, {
    control: 0x000,
    priority_mask: 0x004,
    interrupt_acknowledge: 0x00c,
    end_of_interrupt: 0x010,
});

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// Representation of the GIC CPU interface.
///
/// GICC MMIO registers are banked per CPU core, so the lock only serializes
/// this core's accesses.
#[allow(clippy::upper_case_acronyms)]
pub struct GICC {
    registers: IRQSafeNullLock<MmioRegisterBlock<'static>>,
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

impl GICC {
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
        }
    }

    /// Accept interrupts of any priority.
    ///
    /// Quoting the `GICv2` Architecture Specification:
    ///
    ///   "Writing 255 to the `GICC_PMR` always sets it to the largest supported priority field
    ///    value."
    pub fn priority_accept_all(&self) {
        self.registers.lock(|registers| {
            registers.write_priority_mask(PriorityMask::ZERO.with_priority(255)); // Comment in arch spec.
        });
    }

    /// Enable the interface - start accepting IRQs.
    pub fn enable(&self) {
        self.registers.lock(|registers| {
            registers.write_control(CpuInterfaceControl::ZERO.with_enable(true));
        });
    }

    /// Extract the number of the highest-priority pending IRQ.
    ///
    /// Can only be called from IRQ context, which is ensured by taking an `IRQContext` token.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn pending_irq_number<'irq_context>(
        &self,
        _ic: &libexception::asynchronous::IRQContext<'irq_context>,
    ) -> u10 {
        self.registers
            .lock(|registers| registers.read_interrupt_acknowledge().id())
    }

    /// Complete handling of the currently active IRQ.
    ///
    /// Can only be called from IRQ context, which is ensured by taking an `IRQContext` token.
    ///
    /// To be called after `pending_irq_number()`.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn mark_completed<'irq_context>(
        &self,
        irq_number: u10,
        _ic: &libexception::asynchronous::IRQContext<'irq_context>,
    ) {
        self.registers.lock(|registers| {
            registers.write_end_of_interrupt(InterruptId::ZERO.with_id(irq_number));
        });
    }
}

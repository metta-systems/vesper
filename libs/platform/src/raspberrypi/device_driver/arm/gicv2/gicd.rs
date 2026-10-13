// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2020-2022 Andre Richter <andre.o.richter@gmail.com>

//! GICD Driver - GIC Distributor.
//!
//! # Glossary
//!   - SPI - Shared Peripheral Interrupt.

use {
    arbitrary_int::u5,
    bitbybit::bitfield,
    derive_mmio::Mmio,
    libaddress::{Address, Virtual},
    liblocking::IRQSafeNullLock,
    libmmio::assert_register_offsets,
};

//--------------------------------------------------------------------------------------------------
// Private Definitions
//--------------------------------------------------------------------------------------------------

/// Distributor Control Register
#[bitfield(u32, default = 0, debug)]
struct DistributorControl {
    #[bit(0, rw)]
    enable: bool,
}

/// Interrupt Controller Type Register
#[bitfield(u32, default = 0, debug)]
struct ControllerType {
    #[bits(0..=4, r)]
    it_lines_number: u5,
}

/// Interrupt Processor Targets Registers: the CPU target mask of four
/// interrupts, one byte each.
#[bitfield(u32, default = 0, debug)]
struct ProcessorTargets {
    #[bits(0..=7, rw)]
    target: [u8; 4],
}

#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
struct SharedRegisterBlock {
    control: DistributorControl,
    #[mmio(PureRead)]
    controller_type: ControllerType,
    _reserved1: [u32; 63],
    set_enable: [u32; 31],
    _reserved2: [u32; 424],
    targets: [ProcessorTargets; 248],
}

assert_register_offsets!(SharedRegisterBlock, 0xc00, {
    control: 0x000,
    controller_type: 0x004,
    set_enable: 0x104,
    targets: 0x820,
});

#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
struct BankedRegisterBlock {
    _reserved1: [u32; 64],
    set_enable: u32,
    _reserved2: [u32; 447],
    #[mmio(PureRead)]
    targets: [ProcessorTargets; 8],
}

assert_register_offsets!(BankedRegisterBlock, 0x820, {
    set_enable: 0x100,
    targets: 0x800,
});

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// Representation of the GIC Distributor.
#[allow(clippy::upper_case_acronyms)]
pub struct GICD {
    /// Access to shared registers is guarded with a lock.
    shared_registers: IRQSafeNullLock<MmioSharedRegisterBlock<'static>>,

    /// Access to banked registers is guarded with a lock as well; they are
    /// per core, so the lock only serializes this core's accesses.
    banked_registers: IRQSafeNullLock<MmioBankedRegisterBlock<'static>>,
}

//--------------------------------------------------------------------------------------------------
// Private Code
//--------------------------------------------------------------------------------------------------

/// Return the number of IRQs that this HW implements.
#[inline(always)]
fn num_irqs(registers: &MmioSharedRegisterBlock<'_>) -> usize {
    // Query number of implemented IRQs.
    //
    // Refer to GICv2 Architecture Specification, Section 4.3.2.
    (usize::from(registers.read_controller_type().it_lines_number().value()) + 1) * 32
}

/// Return the number of implemented SPI ITARGETSR registers.
#[inline(always)]
fn implemented_spi_targets(registers: &MmioSharedRegisterBlock<'_>) -> usize {
    assert!(num_irqs(registers) >= 36);

    // The first 32 IRQs are private, so not included in `shared_registers`. Each ITARGETS
    // register has four entries, so shift right by two.
    ((num_irqs(registers) - 32) >> 2).min(registers.len_targets())
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------
use liblocking::interface::Mutex;

impl GICD {
    /// Create an instance.
    ///
    /// # Safety
    ///
    /// - The user must ensure to provide a correct MMIO start address.
    pub const unsafe fn new(mmio_start_addr: Address<Virtual>) -> Self {
        Self {
            // SAFETY: the caller's contract. The two blocks overlap at the
            // same base; each covers registers the other does not.
            shared_registers: IRQSafeNullLock::new(unsafe {
                SharedRegisterBlock::new_mmio(mmio_start_addr.as_mut_ptr())
            }),
            // SAFETY: as above.
            banked_registers: IRQSafeNullLock::new(unsafe {
                BankedRegisterBlock::new_mmio(mmio_start_addr.as_mut_ptr())
            }),
        }
    }

    /// Use a banked ITARGETSR to retrieve the executing core's GIC target mask.
    ///
    /// Quoting the `GICv2` Architecture Specification:
    ///
    ///   "`GICD_ITARGETSR0` to `GICD_ITARGETSR7` are read-only, and each field returns a value that
    ///    corresponds only to the processor reading the register."
    fn local_gic_target_mask(&self) -> u8 {
        self.banked_registers.lock(|registers| {
            registers
                .read_targets(0)
                .map_or(0, |targets| targets.target(0))
        })
    }

    /// Route all SPIs to the boot core and enable the distributor.
    pub fn boot_core_init(&self) {
        assert!(
            libkernel_state::state_manager().is_init(),
            "Only allowed during kernel init phase"
        );

        // Target all SPIs to the boot core only.
        let mask = self.local_gic_target_mask();
        let targets = (0..4).fold(ProcessorTargets::ZERO, |targets, index| {
            targets.with_target(index, mask)
        });

        self.shared_registers.lock(|registers| {
            for index in 0..implemented_spi_targets(registers) {
                let _in_range = registers.write_targets(index, targets);
            }

            registers.write_control(DistributorControl::ZERO.with_enable(true));
        });
    }

    /// Enable an interrupt.
    pub fn enable(&self, irq_num: super::IRQNumber) {
        let irq_num = irq_num.get();

        // Each bit in the u32 enable register corresponds to one IRQ number. Shift right by 5
        // (division by 32) and arrive at the index for the respective ISENABLER[i].
        let enable_reg_index = irq_num >> 5;
        let enable_bit: u32 = 1_u32 << (irq_num % 32);

        // Writing a 1 to a bit of ISENABLER enables that IRQ. All other IRQ
        // enable bits are unaffected, so no read and OR'ing is needed.
        match irq_num {
            // Private.
            0..=31 => {
                self.banked_registers
                    .lock(|registers| registers.write_set_enable(enable_bit));
            }
            // Shared.
            _ => {
                let enable_reg_index_shared = enable_reg_index - 1;

                self.shared_registers.lock(|registers| {
                    let _in_range = registers.write_set_enable(enable_reg_index_shared, enable_bit);
                });
            }
        }
    }
}

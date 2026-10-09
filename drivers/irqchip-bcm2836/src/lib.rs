#![no_std]

//! Register logic for the BCM2836/7 interrupt controllers (Raspberry Pi 3).
//!
//! Two device tree nodes form one controller tree:
//!
//! - `brcm,bcm2836-l1-intc` — the per-core local controller (root). Its
//!   specifier is `<hwirq flags>`; `hwirq` is the bit in the core's IRQ source
//!   register: 0-3 the generic timers (CNTPS, CNTPNS, CNTHP, CNTV), 4-7
//!   mailboxes, 8 the cascaded GPU controller, 9 PMU.
//! - `brcm,bcm2836-armctrl-ic` — the BCM2835 ARM-control block, cascaded on
//!   local line 8. Its specifier is `<bank irq>`: bank 0 the ARM peripherals
//!   of the basic register, banks 1 and 2 GPU interrupts 0-31 and 32-63.
//!
//! Lines: `0..32` local (the local hwirq), then `32 + 32 * bank + irq` for the
//! cascaded block (see [`armctrl_line`]). Only core 0 is driven (single-core
//! nucleus).
//!
//! Resources: the BCM2836 "QA7" local peripherals and BCM2835 ARM
//! peripherals datasheets; the `brcm,bcm2836-l1-intc` and
//! `brcm,bcm2835-armctrl-ic` device tree bindings; QEMU
//! `hw/intc/bcm2836_control.c` and `bcm2835_ic.c`.

use {
    libaddress::{Address, Virtual},
    libirqchip::{INVALID_LINE, IrqChipConfig, SPURIOUS},
    libmmio::MMIODerefWrapper,
    tock_registers::{
        LocalRegisterCopy,
        fields::{Field, FieldValue},
        interfaces::{ReadWriteable, Readable, Writeable},
        register_bitfields, register_structs,
        registers::{ReadOnly, ReadWrite, WriteOnly},
    },
};

/// Index of `brcm,bcm2836-l1-intc` in the component's `compatible` list
/// (declared with `irqchip_export!` in `main.rs`).
pub const KIND_LOCAL: u32 = 0;
/// Index of `brcm,bcm2836-armctrl-ic` in the component's `compatible` list.
pub const KIND_ARMCTRL: u32 = 1;

/// Lines of the local controller.
const LOCAL_LINES: u32 = 32;
/// Lines per bank of the cascaded block.
const BANK_LINES: u32 = 32;
/// Banks of the cascaded block (basic, GPU 0-31, GPU 32-63).
const BANKS: u32 = 3;
/// Bank of the ARM peripherals in the basic registers.
const BASIC_BANK: u32 = 0;
/// Total line count.
pub const LINE_COUNT: u32 = LOCAL_LINES + BANKS * BANK_LINES;

// Local lines: the hwirq numbers of the `brcm,bcm2836-l1-intc` device tree
// binding, i.e. the bit of the core's IRQ source register.

/// Secure EL1 physical timer.
pub const LINE_CNTPS: u32 = 0;
/// Non-secure EL1 physical timer: the nucleus tick.
pub const LINE_CNTPNS: u32 = 1;
/// EL2 physical timer.
pub const LINE_CNTHP: u32 = 2;
/// Virtual timer.
pub const LINE_CNTV: u32 = 3;
/// Mailbox 0; mailboxes 1-3 follow.
pub const LINE_MAILBOX0: u32 = 4;
/// The cascaded GPU controller: never a deliverable line.
const LINE_GPU: u32 = 8;
/// Performance monitor.
pub const LINE_PMU: u32 = 9;

/// The line of `irq` in `bank` of the cascaded ARM-control block.
pub const fn armctrl_line(bank: u32, irq: u32) -> u32 {
    LOCAL_LINES + bank * BANK_LINES + irq
}

register_bitfields! {
    u32,

    /// Per-core generic timer interrupt control (`CORE_TIMER_INT_CONTROL`).
    pub TIMER_CONTROL [
        CNTPS_IRQ OFFSET(0) NUMBITS(1) [],
        CNTPNS_IRQ OFFSET(1) NUMBITS(1) [],
        CNTHP_IRQ OFFSET(2) NUMBITS(1) [],
        CNTV_IRQ OFFSET(3) NUMBITS(1) [],
        CNTPS_FIQ OFFSET(4) NUMBITS(1) [],
        CNTPNS_FIQ OFFSET(5) NUMBITS(1) [],
        CNTHP_FIQ OFFSET(6) NUMBITS(1) [],
        CNTV_FIQ OFFSET(7) NUMBITS(1) [],
    ],

    /// Per-core mailbox interrupt control (`CORE_MAILBOX_INT_CONTROL`).
    pub MAILBOX_CONTROL [
        MAILBOX0_IRQ OFFSET(0) NUMBITS(1) [],
        MAILBOX1_IRQ OFFSET(1) NUMBITS(1) [],
        MAILBOX2_IRQ OFFSET(2) NUMBITS(1) [],
        MAILBOX3_IRQ OFFSET(3) NUMBITS(1) [],
        MAILBOX0_FIQ OFFSET(4) NUMBITS(1) [],
        MAILBOX1_FIQ OFFSET(5) NUMBITS(1) [],
        MAILBOX2_FIQ OFFSET(6) NUMBITS(1) [],
        MAILBOX3_FIQ OFFSET(7) NUMBITS(1) [],
    ],

    /// Per-core interrupt sources (`CORE_IRQ_SOURCE`); bit `n` is local hwirq `n`.
    pub IRQ_SOURCE [
        CNTPS OFFSET(0) NUMBITS(1) [],
        CNTPNS OFFSET(1) NUMBITS(1) [],
        CNTHP OFFSET(2) NUMBITS(1) [],
        CNTV OFFSET(3) NUMBITS(1) [],
        MAILBOX0 OFFSET(4) NUMBITS(1) [],
        MAILBOX1 OFFSET(5) NUMBITS(1) [],
        MAILBOX2 OFFSET(6) NUMBITS(1) [],
        MAILBOX3 OFFSET(7) NUMBITS(1) [],
        /// The cascaded GPU interrupt controller
        GPU OFFSET(8) NUMBITS(1) [],
        PMU OFFSET(9) NUMBITS(1) [],
        AXI OFFSET(10) NUMBITS(1) [],
        LOCAL_TIMER OFFSET(11) NUMBITS(1) [],
    ],

    /// GPU interrupt routing (`GPU_INT_ROUTING`).
    pub GPU_ROUTING [
        IRQ_CORE OFFSET(0) NUMBITS(2) [
            Core0 = 0
        ],
        FIQ_CORE OFFSET(2) NUMBITS(2) [
            Core0 = 0
        ],
    ],

    /// PMU interrupt routing (`PMU_INT_ROUTING_SET`/`_CLEAR`).
    pub PMU_ROUTING [
        CORE0_IRQ OFFSET(0) NUMBITS(1) [],
    ],

    /// ARM-control basic pending, enable and disable registers.
    pub BASIC [
        ARM_TIMER OFFSET(0) NUMBITS(1) [],
        ARM_MAILBOX OFFSET(1) NUMBITS(1) [],
        ARM_DOORBELL0 OFFSET(2) NUMBITS(1) [],
        ARM_DOORBELL1 OFFSET(3) NUMBITS(1) [],
        GPU0_HALTED OFFSET(4) NUMBITS(1) [],
        GPU1_HALTED OFFSET(5) NUMBITS(1) [],
        ILLEGAL_ACCESS1 OFFSET(6) NUMBITS(1) [],
        ILLEGAL_ACCESS0 OFFSET(7) NUMBITS(1) [],
        /// The ARM peripheral interrupts above (bank 0)
        ARM_PERIPHERALS OFFSET(0) NUMBITS(8) [],
        /// Some GPU interrupt 0-31 is pending (read-only)
        PENDING1 OFFSET(8) NUMBITS(1) [],
        /// Some GPU interrupt 32-63 is pending (read-only)
        PENDING2 OFFSET(9) NUMBITS(1) [],
    ],

    /// ARM-control GPU pending, enable and disable registers: bit `n` is GPU
    /// interrupt `n` of the bank.
    pub GPU_IRQS [
        ALL OFFSET(0) NUMBITS(32) [],
    ],
}

register_structs! {
    /// The BCM2836 local peripherals block.
    #[allow(non_snake_case)]
    pub LocalRegisterBlock {
        (0x00 => _control),
        (0x0C => GPU_INT_ROUTING: ReadWrite<u32, GPU_ROUTING::Register>),
        (0x10 => PMU_INT_ROUTING_SET: WriteOnly<u32, PMU_ROUTING::Register>),
        (0x14 => PMU_INT_ROUTING_CLEAR: WriteOnly<u32, PMU_ROUTING::Register>),
        (0x18 => _timers),
        (0x40 => CORE_TIMER_INT_CONTROL: [ReadWrite<u32, TIMER_CONTROL::Register>; 4]),
        (0x50 => CORE_MAILBOX_INT_CONTROL: [ReadWrite<u32, MAILBOX_CONTROL::Register>; 4]),
        (0x60 => CORE_IRQ_SOURCE: [ReadOnly<u32, IRQ_SOURCE::Register>; 4]),
        (0x70 => _fiq_sources),
        (0x80 => @END),
    }
}

register_structs! {
    /// The BCM2835 ARM-control interrupt block.
    #[allow(non_snake_case)]
    pub ArmctrlRegisterBlock {
        (0x00 => BASIC_PENDING: ReadOnly<u32, BASIC::Register>),
        (0x04 => GPU_PENDING: [ReadOnly<u32, GPU_IRQS::Register>; 2]),
        (0x0C => _fiq_control),
        (0x10 => GPU_ENABLE: [WriteOnly<u32, GPU_IRQS::Register>; 2]),
        (0x18 => BASIC_ENABLE: WriteOnly<u32, BASIC::Register>),
        (0x1C => GPU_DISABLE: [WriteOnly<u32, GPU_IRQS::Register>; 2]),
        (0x24 => BASIC_DISABLE: WriteOnly<u32, BASIC::Register>),
        (0x28 => @END),
    }
}

type LocalRegisters = MMIODerefWrapper<LocalRegisterBlock>;
type ArmctrlRegisters = MMIODerefWrapper<ArmctrlRegisterBlock>;

/// IRQ enables of the generic timers, by local line.
const TIMER_IRQS: [Field<u32, TIMER_CONTROL::Register>; 4] = [
    TIMER_CONTROL::CNTPS_IRQ,
    TIMER_CONTROL::CNTPNS_IRQ,
    TIMER_CONTROL::CNTHP_IRQ,
    TIMER_CONTROL::CNTV_IRQ,
];

/// IRQ enables of the mailboxes, by local line minus [`LINE_MAILBOX0`].
const MAILBOX_IRQS: [Field<u32, MAILBOX_CONTROL::Register>; 4] = [
    MAILBOX_CONTROL::MAILBOX0_IRQ,
    MAILBOX_CONTROL::MAILBOX1_IRQ,
    MAILBOX_CONTROL::MAILBOX2_IRQ,
    MAILBOX_CONTROL::MAILBOX3_IRQ,
];

/// Directly delivered local sources, in claim order, with their lines.
const LOCAL_SOURCES: [(u32, Field<u32, IRQ_SOURCE::Register>); 9] = [
    (LINE_CNTPS, IRQ_SOURCE::CNTPS),
    (LINE_CNTPNS, IRQ_SOURCE::CNTPNS),
    (LINE_CNTHP, IRQ_SOURCE::CNTHP),
    (LINE_CNTV, IRQ_SOURCE::CNTV),
    (LINE_MAILBOX0, IRQ_SOURCE::MAILBOX0),
    (LINE_MAILBOX0 + 1, IRQ_SOURCE::MAILBOX1),
    (LINE_MAILBOX0 + 2, IRQ_SOURCE::MAILBOX2),
    (LINE_MAILBOX0 + 3, IRQ_SOURCE::MAILBOX3),
    (LINE_PMU, IRQ_SOURCE::PMU),
];

/// ARM peripheral interrupts of the basic registers, by irq of bank 0.
const BASIC_IRQS: [Field<u32, BASIC::Register>; 8] = [
    BASIC::ARM_TIMER,
    BASIC::ARM_MAILBOX,
    BASIC::ARM_DOORBELL0,
    BASIC::ARM_DOORBELL1,
    BASIC::GPU0_HALTED,
    BASIC::GPU1_HALTED,
    BASIC::ILLEGAL_ACCESS1,
    BASIC::ILLEGAL_ACCESS0,
];

/// GPU interrupt `irq` of a bank.
pub const fn gpu_irq(irq: u32) -> FieldValue<u32, GPU_IRQS::Register> {
    FieldValue::<u32, GPU_IRQS::Register>::new(1, irq as usize, 1)
}

/// A cascaded-block line as `(bank, irq)`.
const fn bank_of(line: u32) -> Option<(u32, u32)> {
    if line < LOCAL_LINES || line >= LINE_COUNT {
        return None;
    }
    let offset = line - LOCAL_LINES;
    Some((offset / BANK_LINES, offset % BANK_LINES))
}

/// Errors from [`Bcm2836::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// No `brcm,bcm2836-l1-intc` controller was configured
    MissingLocal,
    /// A controller has no MMIO region, or one too small for its registers
    BadRegion,
}

/// The local controller and, when present, the cascaded ARM-control block.
pub struct Bcm2836 {
    local: LocalRegisters,
    armctrl: Option<ArmctrlRegisters>,
    /// Enabled interrupts of the GPU banks: the enable registers are
    /// write-only and the pending registers are not masked by them
    gpu_enabled: [LocalRegisterCopy<u32, GPU_IRQS::Register>; 2],
    /// Enabled ARM peripheral interrupts of the basic bank
    basic_enabled: LocalRegisterCopy<u32, BASIC::Register>,
}

/// The mapping of `region` for a register block of type `T`.
fn block_address<T>(region: &libirqchip::MmioRegion) -> Result<Address<Virtual>, ConfigError> {
    if region.size < size_of::<T>() as u64 {
        return Err(ConfigError::BadRegion);
    }
    Ok(Address::<Virtual>::new(region.virt))
}

impl Bcm2836 {
    /// Bind to the mapped controllers in `config` and mask every line.
    ///
    /// # Safety
    ///
    /// Each region's `virt` must map the named controller's registers for EL1
    /// for `'static`, and nothing else may drive them.
    pub unsafe fn new(config: &IrqChipConfig) -> Result<Self, ConfigError> {
        let mut local = None;
        let mut armctrl = None;
        for controller in config.controllers() {
            let region = controller.regions().first().ok_or(ConfigError::BadRegion)?;
            match controller.kind {
                KIND_LOCAL => {
                    local = Some(LocalRegisters::new(block_address::<LocalRegisterBlock>(
                        region,
                    )?));
                }
                KIND_ARMCTRL => {
                    armctrl = Some(ArmctrlRegisters::new(
                        block_address::<ArmctrlRegisterBlock>(region)?,
                    ));
                }
                _ => {}
            }
        }
        let chip = Self {
            local: local.ok_or(ConfigError::MissingLocal)?,
            armctrl,
            gpu_enabled: [LocalRegisterCopy::new(0); 2],
            basic_enabled: LocalRegisterCopy::new(0),
        };
        chip.reset();
        Ok(chip)
    }

    /// Mask every line and route the GPU interrupt to core 0.
    fn reset(&self) {
        self.local.CORE_TIMER_INT_CONTROL[0].set(0);
        self.local.CORE_MAILBOX_INT_CONTROL[0].set(0);
        self.local
            .PMU_INT_ROUTING_CLEAR
            .write(PMU_ROUTING::CORE0_IRQ::SET);
        self.local
            .GPU_INT_ROUTING
            .write(GPU_ROUTING::IRQ_CORE::Core0 + GPU_ROUTING::FIQ_CORE::Core0);
        if let Some(armctrl) = &self.armctrl {
            armctrl
                .BASIC_DISABLE
                .write(BASIC::ARM_PERIPHERALS.val(BASIC::ARM_PERIPHERALS.mask));
            for disable in &armctrl.GPU_DISABLE {
                disable.write(GPU_IRQS::ALL.val(GPU_IRQS::ALL.mask));
            }
        }
    }

    /// The first pending, enabled line on core 0, or [`SPURIOUS`].
    pub fn claim(&self) -> u32 {
        let source = self.local.CORE_IRQ_SOURCE[0].extract();
        if let Some(&(line, _)) = LOCAL_SOURCES
            .iter()
            .find(|(_, field)| source.is_set(*field))
        {
            return line;
        }
        let Some(armctrl) = &self.armctrl else {
            return SPURIOUS;
        };
        if !source.is_set(IRQ_SOURCE::GPU) {
            return SPURIOUS;
        }
        let basic = armctrl.BASIC_PENDING.extract();
        if let Some(irq) = (0..).zip(BASIC_IRQS).find_map(|(irq, field)| {
            (basic.is_set(field) && self.basic_enabled.is_set(field)).then_some(irq)
        }) {
            return armctrl_line(BASIC_BANK, irq);
        }
        (1..)
            .zip(armctrl.GPU_PENDING.iter().zip(self.gpu_enabled))
            .find_map(|(bank, (pending, enabled))| {
                let lines = pending.read(GPU_IRQS::ALL) & enabled.get();
                (lines != 0).then(|| armctrl_line(bank, lines.trailing_zeros()))
            })
            .unwrap_or(SPURIOUS)
    }

    /// Enable (`enable`) or disable delivery of `line`.
    pub fn set_enabled(&mut self, line: u32, enable: bool) {
        let value = u32::from(enable);
        if let Some(&timer) = TIMER_IRQS.get(line as usize) {
            self.local.CORE_TIMER_INT_CONTROL[0].modify(timer.val(value));
        } else if let Some(&mailbox) = line
            .checked_sub(LINE_MAILBOX0)
            .and_then(|index| MAILBOX_IRQS.get(index as usize))
        {
            self.local.CORE_MAILBOX_INT_CONTROL[0].modify(mailbox.val(value));
        } else if line == LINE_PMU {
            let route = PMU_ROUTING::CORE0_IRQ::SET;
            if enable {
                self.local.PMU_INT_ROUTING_SET.write(route);
            } else {
                self.local.PMU_INT_ROUTING_CLEAR.write(route);
            }
        } else if let (Some((bank, irq)), Some(armctrl)) = (bank_of(line), &self.armctrl) {
            if bank == BASIC_BANK {
                let Some(&field) = BASIC_IRQS.get(irq as usize) else {
                    return;
                };
                self.basic_enabled.modify(field.val(value));
                if enable {
                    armctrl.BASIC_ENABLE.write(field.val(1));
                } else {
                    armctrl.BASIC_DISABLE.write(field.val(1));
                }
            } else {
                let index = (bank - 1) as usize;
                let interrupt = Field::<u32, GPU_IRQS::Register>::new(1, irq as usize);
                self.gpu_enabled[index].modify(interrupt.val(value));
                if enable {
                    armctrl.GPU_ENABLE[index].write(gpu_irq(irq));
                } else {
                    armctrl.GPU_DISABLE[index].write(gpu_irq(irq));
                }
            }
        }
        // The GPU cascade (local line 8) is always routed; other local lines
        // are not implemented by the controller.
    }

    /// Decode a device tree interrupt specifier for the controller of `kind`.
    pub fn xlate(kind: u32, cells: &[u32]) -> u32 {
        match (kind, cells) {
            (KIND_LOCAL, [hwirq, ..]) if *hwirq < LOCAL_LINES && *hwirq != LINE_GPU => *hwirq,
            (KIND_ARMCTRL, [bank, irq, ..]) if *bank < BANKS && *irq < BANK_LINES => {
                armctrl_line(*bank, *irq)
            }
            _ => INVALID_LINE,
        }
    }
}

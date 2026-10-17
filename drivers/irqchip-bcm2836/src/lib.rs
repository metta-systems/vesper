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
    bitbybit::{bitenum, bitfield},
    derive_mmio::Mmio,
    libirqchip::{INVALID_LINE, IrqChipConfig, SPURIOUS},
    libmmio::assert_register_offsets,
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

/// A core of the BCM2836, as the routing registers name it.
#[bitenum(u2, exhaustive = true)]
#[derive(Debug, PartialEq, Eq)]
pub enum Core {
    Core0 = 0,
    Core1 = 1,
    Core2 = 2,
    Core3 = 3,
}

/// Per-core generic timer interrupt control (`CORE_TIMER_INT_CONTROL`). The
/// timers are indexed by local line: CNTPS, CNTPNS, CNTHP, CNTV.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct TimerControl {
    /// IRQ enable of each timer
    #[bit(0, rw)]
    irq: [bool; 4],
    /// FIQ enable of each timer, overriding its IRQ enable
    #[bit(4, rw)]
    fiq: [bool; 4],
}

/// Per-core mailbox interrupt control (`CORE_MAILBOX_INT_CONTROL`), by
/// mailbox.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct MailboxControl {
    /// IRQ enable of each mailbox
    #[bit(0, rw)]
    irq: [bool; 4],
    /// FIQ enable of each mailbox, overriding its IRQ enable
    #[bit(4, rw)]
    fiq: [bool; 4],
}

/// Per-core interrupt sources (`CORE_IRQ_SOURCE`); bit `n` is local hwirq `n`.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct IrqSource {
    /// The generic timers, by local line
    #[bit(0, r)]
    timer: [bool; 4],
    /// The mailboxes
    #[bit(4, r)]
    mailbox: [bool; 4],
    /// The cascaded GPU interrupt controller
    #[bit(8, r)]
    gpu: bool,
    /// The performance monitor
    #[bit(9, r)]
    pmu: bool,
    /// AXI outstanding writes or reads
    #[bit(10, r)]
    axi: bool,
    /// The local timer
    #[bit(11, r)]
    local_timer: bool,
}

/// GPU interrupt routing (`GPU_INT_ROUTING`).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct GpuRouting {
    /// The core that takes the GPU IRQ
    #[bits(0..=1, rw)]
    irq_core: Core,
    /// The core that takes the GPU FIQ
    #[bits(2..=3, rw)]
    fiq_core: Core,
}

/// PMU interrupt routing (`PMU_INT_ROUTING_SET` / `_CLEAR`): writing a one
/// sets or clears the route, a zero leaves it.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct PmuRouting {
    /// The PMU IRQ goes to each core
    #[bit(0, rw)]
    irq_core: [bool; 4],
    /// The PMU FIQ goes to each core
    #[bit(4, rw)]
    fiq_core: [bool; 4],
}

/// ARM-control basic pending, enable and disable registers.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct BasicInterrupts {
    /// The ARM peripheral interrupts of bank 0, by irq: ARM timer, ARM
    /// mailbox, ARM doorbell 0 and 1, GPU 0 and 1 halted, illegal access
    /// types 1 and 0
    #[bit(0, rw)]
    arm_peripheral: [bool; 8],
    /// Some GPU interrupt 0-31 is pending (read-only)
    #[bit(8, r)]
    gpu_pending_low: bool,
    /// Some GPU interrupt 32-63 is pending (read-only)
    #[bit(9, r)]
    gpu_pending_high: bool,
}

/// ARM-control GPU pending, enable and disable registers: bit `n` is GPU
/// interrupt `n` of the bank.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct GpuInterrupts {
    #[bit(0, rw)]
    irq: [bool; 32],
}

/// ARM peripheral interrupts in bank 0.
const ARM_PERIPHERALS: u32 = 8;

impl BasicInterrupts {
    /// Every ARM peripheral interrupt.
    pub const ALL_ARM_PERIPHERALS: Self = Self::new_with_raw_value(0xff);
}

impl GpuInterrupts {
    /// Every interrupt of the bank.
    pub const ALL: Self = Self::new_with_raw_value(u32::MAX);
}

/// The BCM2836 local peripherals block. Only core 0 is driven, so the other
/// cores' registers are reserved here.
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
pub struct LocalRegisterBlock {
    _control: [u32; 3],
    gpu_int_routing: GpuRouting,
    #[mmio(Write)]
    pmu_int_routing_set: PmuRouting,
    #[mmio(Write)]
    pmu_int_routing_clear: PmuRouting,
    _timers: [u32; 10],
    core0_timer_int_control: TimerControl,
    _core1_3_timer_int_control: [u32; 3],
    core0_mailbox_int_control: MailboxControl,
    _core1_3_mailbox_int_control: [u32; 3],
    #[mmio(PureRead)]
    core0_irq_source: IrqSource,
    _core1_3_irq_source: [u32; 3],
    _fiq_sources: [u32; 4],
}

assert_register_offsets!(LocalRegisterBlock, 0x80, {
    gpu_int_routing: 0x0c,
    pmu_int_routing_set: 0x10,
    pmu_int_routing_clear: 0x14,
    core0_timer_int_control: 0x40,
    core0_mailbox_int_control: 0x50,
    core0_irq_source: 0x60,
    _fiq_sources: 0x70,
});

/// The BCM2835 ARM-control interrupt block.
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
pub struct ArmctrlRegisterBlock {
    #[mmio(PureRead)]
    basic_pending: BasicInterrupts,
    #[mmio(PureRead)]
    gpu_pending: [GpuInterrupts; 2],
    _fiq_control: u32,
    #[mmio(Write)]
    gpu_enable: [GpuInterrupts; 2],
    #[mmio(Write)]
    basic_enable: BasicInterrupts,
    #[mmio(Write)]
    gpu_disable: [GpuInterrupts; 2],
    #[mmio(Write)]
    basic_disable: BasicInterrupts,
}

assert_register_offsets!(ArmctrlRegisterBlock, 0x28, {
    basic_pending: 0x00,
    gpu_pending: 0x04,
    gpu_enable: 0x10,
    basic_enable: 0x18,
    gpu_disable: 0x1c,
    basic_disable: 0x24,
});

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
    local: MmioLocalRegisterBlock<'static>,
    armctrl: Option<MmioArmctrlRegisterBlock<'static>>,
    /// Enabled interrupts of the GPU banks: the enable registers are
    /// write-only and the pending registers are not masked by them
    gpu_enabled: [GpuInterrupts; 2],
    /// Enabled ARM peripheral interrupts of the basic bank
    basic_enabled: BasicInterrupts,
}

/// The register block of type `T` mapped by `region`.
fn block<T>(region: &libirqchip::MmioRegion) -> Result<*mut T, ConfigError> {
    if region.size < size_of::<T>() as u64 {
        return Err(ConfigError::BadRegion);
    }
    Ok(region.virt as *mut T)
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
                    // SAFETY: the caller guarantees a mapped, exclusive block.
                    local = Some(unsafe { LocalRegisterBlock::new_mmio(block(region)?) });
                }
                KIND_ARMCTRL => {
                    // SAFETY: as above.
                    armctrl = Some(unsafe { ArmctrlRegisterBlock::new_mmio(block(region)?) });
                }
                _ => {}
            }
        }
        let mut chip = Self {
            local: local.ok_or(ConfigError::MissingLocal)?,
            armctrl,
            gpu_enabled: [GpuInterrupts::ZERO; 2],
            basic_enabled: BasicInterrupts::ZERO,
        };
        chip.reset();
        Ok(chip)
    }

    /// Mask every line and route the GPU interrupt to core 0.
    fn reset(&mut self) {
        self.local.write_core0_timer_int_control(TimerControl::ZERO);
        self.local
            .write_core0_mailbox_int_control(MailboxControl::ZERO);
        self.local
            .write_pmu_int_routing_clear(PmuRouting::ZERO.with_irq_core(0, true));
        self.local.write_gpu_int_routing(
            GpuRouting::ZERO
                .with_irq_core(Core::Core0)
                .with_fiq_core(Core::Core0),
        );
        if let Some(armctrl) = &mut self.armctrl {
            armctrl.write_basic_disable(BasicInterrupts::ALL_ARM_PERIPHERALS);
            for bank in 0..armctrl.len_gpu_disable() {
                if armctrl.write_gpu_disable(bank, GpuInterrupts::ALL).is_err() {
                    break;
                }
            }
        }
    }

    /// The first pending, enabled line on core 0, or [`SPURIOUS`].
    pub fn claim(&self) -> u32 {
        let source = self.local.read_core0_irq_source();
        let direct = (0..4_u32)
            .find(|&timer| source.timer(timer as usize))
            .map(|timer| LINE_CNTPS + timer)
            .or_else(|| {
                (0..4_u32)
                    .find(|&mailbox| source.mailbox(mailbox as usize))
                    .map(|mailbox| LINE_MAILBOX0 + mailbox)
            })
            .or_else(|| source.pmu().then_some(LINE_PMU));
        if let Some(line) = direct {
            return line;
        }
        let Some(armctrl) = &self.armctrl else {
            return SPURIOUS;
        };
        if !source.gpu() {
            return SPURIOUS;
        }
        let basic = armctrl.read_basic_pending().raw_value() & self.basic_enabled.raw_value();
        if basic != 0 {
            return armctrl_line(BASIC_BANK, basic.trailing_zeros());
        }
        (1..)
            .zip(self.gpu_enabled.iter().enumerate())
            .find_map(|(bank, (index, enabled))| {
                let pending = armctrl.read_gpu_pending(index).ok()?;
                let lines = pending.raw_value() & enabled.raw_value();
                (lines != 0).then(|| armctrl_line(bank, lines.trailing_zeros()))
            })
            .unwrap_or(SPURIOUS)
    }

    /// Enable (`enable`) or disable delivery of `line`.
    pub fn set_enabled(&mut self, line: u32, enable: bool) {
        if line <= LINE_CNTV {
            let timer = line as usize;
            self.local
                .modify_core0_timer_int_control(|control| control.with_irq(timer, enable));
        } else if (LINE_MAILBOX0..LINE_MAILBOX0 + 4).contains(&line) {
            let mailbox = (line - LINE_MAILBOX0) as usize;
            self.local
                .modify_core0_mailbox_int_control(|control| control.with_irq(mailbox, enable));
        } else if line == LINE_PMU {
            let route = PmuRouting::ZERO.with_irq_core(0, true);
            if enable {
                self.local.write_pmu_int_routing_set(route);
            } else {
                self.local.write_pmu_int_routing_clear(route);
            }
        } else if let (Some((bank, irq)), Some(armctrl)) = (bank_of(line), &mut self.armctrl) {
            if bank == BASIC_BANK {
                if irq >= ARM_PERIPHERALS {
                    return;
                }
                let irq = irq as usize;
                self.basic_enabled = self.basic_enabled.with_arm_peripheral(irq, enable);
                let interrupt = BasicInterrupts::ZERO.with_arm_peripheral(irq, true);
                if enable {
                    armctrl.write_basic_enable(interrupt);
                } else {
                    armctrl.write_basic_disable(interrupt);
                }
            } else {
                let index = (bank - 1) as usize;
                let irq = irq as usize;
                let Some(enabled) = self.gpu_enabled.get_mut(index) else {
                    return;
                };
                *enabled = enabled.with_irq(irq, enable);
                let interrupt = GpuInterrupts::ZERO.with_irq(irq, true);
                // `index` is a bank of `gpu_enabled`, so of the registers too.
                let _in_range = if enable {
                    armctrl.write_gpu_enable(index, interrupt)
                } else {
                    armctrl.write_gpu_disable(index, interrupt)
                };
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

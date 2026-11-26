#![no_std]

//! Controller logic for the Arm `GICv2` (GIC-400) on top of `arm-gic`.
//!
//! One device tree node, `arm,gic-400` (or `arm,cortex-a15-gic`), whose `reg`
//! lists the distributor first and the CPU interface second. Its specifier is
//! `<type number flags>`: type 0 is a shared peripheral interrupt (SPI
//! `number`, INTID `32 + number`), type 1 a private peripheral interrupt (PPI
//! `number`, INTID `16 + number`).
//!
//! Lines are INTIDs. Delivery follows the mask-on-delivery protocol shared
//! with the BCM component: the nucleus masks a line it cannot hand off and
//! signals end-of-interrupt (priority drop and deactivation together); the
//! line's handler unmasks it when done.
//!
//! Security state: only the Non-secure (or no-Security-Extensions) view is
//! used. Interrupts are acknowledged and ended through the banked
//! `GICC_IAR`/`GICC_EOIR` (`InterruptGroup::Group0` in `arm-gic`), which in
//! the Non-secure view serve Group 1 — the group the Raspberry Pi firmware
//! assigns to every interrupt. The aliased Group 1 registers are Secure-only.
//!
//! Each core has its own CPU interface and its own SGIs and PPIs (their
//! enables in `GICD_ISENABLER0` and targets in `GICD_ITARGETSR0`-`7` are
//! banked), so [`Gic400::init_cpu`] runs on every core. Inter-processor
//! interrupts are SGI [`IPI_SGI`], sent through `GICD_SGIR` with a CPU target
//! list. `GICC_IAR` returns an SGI's ID with its source CPU in bits 12:10, and
//! `GICC_EOIR` must be written with that whole value: a claimed SGI is ended
//! at once, so its line needs no [`Gic400::complete`].
//!
//! Resources: the Arm `GICv2` architecture specification, the GIC-400 TRM, the
//! `arm,gic` device tree binding, QEMU `hw/intc/arm_gic.c`; the BCM2711 ARM
//! Peripherals manual §6.5.1 (the GIC-400 configured with four CPU
//! interfaces).

use {
    arm_gic::{
        IntId, InterruptGroup, Trigger,
        gicv2::{
            GicV2, SgiTarget, SgiTargetListFilter,
            registers::{Gicc, Gicd},
        },
    },
    libirqchip::{
        INVALID_LINE, IrqChipConfig, MAX_CPUS, MmioRegion, SPURIOUS, TRIGGER_EDGE, TRIGGER_FIXED,
        TRIGGER_LEVEL,
    },
};

/// Number of INTIDs a `GICv2` can deliver; 1020-1023 are special.
pub const LINE_COUNT: u32 = 1020;

/// The SGI that carries inter-processor interrupts.
pub const IPI_SGI: u32 = 0;
/// The line of the inter-processor interrupt: SGI [`IPI_SGI`]'s INTID.
pub const IPI_LINE: u32 = IPI_SGI;
/// The interrupt ID field of `GICC_IAR`; above it, for an SGI, the source CPU.
const INTERRUPT_ID_MASK: u32 = 0x3ff;
/// SGIs are INTIDs 0-15.
const SGI_COUNT: u32 = 16;

/// Device tree specifier type of a shared peripheral interrupt.
const SPECIFIER_SPI: u32 = 0;
/// Device tree specifier type of a private peripheral interrupt.
const SPECIFIER_PPI: u32 = 1;
/// SPIs a `GICv2` can number (INTIDs 32-1019).
const SPI_COUNT: u32 = LINE_COUNT - 32;
/// PPIs per CPU (INTIDs 16-31).
const PPI_COUNT: u32 = 16;

// Device tree `IRQ_TYPE_*` values in the specifier's flags cell.
const IRQ_TYPE_MASK: u32 = 0xf;
const IRQ_TYPE_EDGE_RISING: u32 = 1;
const IRQ_TYPE_EDGE_FALLING: u32 = 2;
const IRQ_TYPE_LEVEL_HIGH: u32 = 4;
const IRQ_TYPE_LEVEL_LOW: u32 = 8;

/// Errors from [`Gic400`] operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// No controller was configured
    MissingController,
    /// The distributor or CPU interface region is missing or too small
    BadRegion,
    /// The line is not a configurable interrupt
    BadLine,
    /// A logical CPU at least [`MAX_CPUS`]
    BadCpu,
    /// The calling core reads no CPU interface of its own (a uniprocessor GIC)
    NoCpuInterface,
    /// A logical CPU that has not run [`Gic400::init_cpu`]
    UnknownCpu,
}

/// A bound `GICv2`.
pub struct Gic400 {
    gic: GicV2<'static>,
    /// The distributor, for the banked target bytes `arm-gic` does not read
    distributor: *mut Gicd,
    /// The CPU interface bit of each logical CPU that ran
    /// [`Gic400::init_cpu`], zero for one that has not
    interfaces: [u8; MAX_CPUS],
}

/// The CPU interface of the core running this, as the bit `GICD_ITARGETSR0`-`7`
/// (read-only and banked per core) report for its SGIs and PPIs; zero on a
/// uniprocessor GIC.
///
/// # Safety
///
/// `distributor` maps the distributor for EL1.
unsafe fn this_cpu_interface(distributor: *mut Gicd) -> u8 {
    // The targets are one byte per INTID; the GIC allows byte accesses.
    // SAFETY: a field of the caller's mapped distributor, not dereferenced.
    let targets = unsafe { &raw mut (*distributor).itargetsr }.cast::<u8>();
    (0..32).fold(0_u8, |mask, intid| {
        // SAFETY: bytes 0-31 are the SGI/PPI targets, inside the block.
        mask | unsafe { targets.add(intid).read_volatile() }
    })
}

/// The interrupt of `line`, if it is a deliverable INTID.
fn intid(line: u32) -> Option<IntId> {
    (line < LINE_COUNT)
        .then(|| IntId::try_from(line).ok())
        .flatten()
}

/// The register block of type `T` mapped by `region`.
fn block<T>(region: Option<&MmioRegion>) -> Result<*mut T, ConfigError> {
    let region = region.ok_or(ConfigError::BadRegion)?;
    if region.size < size_of::<T>() as u64 || region.virt == 0 {
        return Err(ConfigError::BadRegion);
    }
    Ok(region.virt as *mut T)
}

/// Target every SPI below `lines` at the CPU interface of the core running
/// this, the one `GICD_ITARGETSR0`-`7` (read-only and banked per core)
/// report for its SGIs and PPIs. SPI targets reset to no core, and an SPI
/// without a target is never forwarded. A uniprocessor GIC reads zero there
/// and ignores the target registers, so nothing is written.
///
/// # Safety
///
/// `distributor` maps the distributor for EL1, and nothing else accesses it
/// during the call.
unsafe fn route_spis_to_this_cpu(distributor: *mut Gicd, lines: u32) {
    // SAFETY: the caller's mapped distributor.
    let this_cpu = unsafe { this_cpu_interface(distributor) };
    // SAFETY: a field of the caller's mapped distributor, not dereferenced.
    let targets = unsafe { &raw mut (*distributor).itargetsr }.cast::<u8>();
    if this_cpu == 0 {
        return;
    }
    for intid in 32..lines as usize {
        // SAFETY: `lines` is at most `LINE_COUNT`, inside the target bank.
        unsafe {
            targets.add(intid).write_volatile(this_cpu);
        }
    }
}

impl Gic400 {
    /// Bind to the mapped controller in `config`: route every SPI to this
    /// core, mask every interrupt, then enable forwarding in the distributor
    /// and the CPU interface with no priority filtering.
    ///
    /// # Safety
    ///
    /// The regions must map the GIC's distributor and CPU interface for EL1
    /// for `'static`, and nothing else may drive them.
    pub unsafe fn new(config: &IrqChipConfig) -> Result<Self, ConfigError> {
        let controller = config
            .controllers()
            .first()
            .ok_or(ConfigError::MissingController)?;
        let regions = controller.regions();
        let distributor = block::<Gicd>(regions.first())?;
        let cpu_interface = block::<Gicc>(regions.get(1))?;
        // SAFETY: the caller guarantees exclusive, mapped register blocks;
        // this instance is gone before the targets are written.
        let lines = unsafe { GicV2::new(distributor, cpu_interface) }
            .typer()
            .num_irqs()
            .min(LINE_COUNT);
        // SAFETY: as above; no driver instance holds the distributor now.
        unsafe {
            route_spis_to_this_cpu(distributor, lines);
        }
        // SAFETY: the caller guarantees exclusive, mapped register blocks.
        let mut gic = unsafe { GicV2::new(distributor, cpu_interface) };
        gic.enable_all_interrupts(false);
        // `setup` enables the CPU interface and opens the priority mask; in
        // the Non-secure view its group writes are ignored, so also set
        // GICD_CTLR bit 0, which is the Non-secure Group 1 enable there and
        // the Group 0 enable without Security Extensions.
        gic.setup();
        gic.enable_group0(true);
        gic.set_priority_mask(0xff);
        Ok(Self {
            gic,
            distributor,
            interfaces: [0; MAX_CPUS],
        })
    }

    /// The highest-priority pending interrupt on the calling core,
    /// acknowledged, or [`SPURIOUS`]. An SGI is also ended here, with the
    /// whole `GICC_IAR` value its end needs.
    pub fn claim(&mut self) -> u32 {
        let Some(acknowledged) = self
            .gic
            .get_and_acknowledge_interrupt(InterruptGroup::Group0)
        else {
            return SPURIOUS;
        };
        let line = u32::from(acknowledged) & INTERRUPT_ID_MASK;
        if line >= LINE_COUNT {
            return SPURIOUS;
        }
        if line < SGI_COUNT {
            self.gic.end_interrupt(acknowledged, InterruptGroup::Group0);
        }
        line
    }

    /// End-of-interrupt for a claimed `line`; an SGI was ended by its claim.
    pub fn complete(&mut self, line: u32) {
        if let Some(intid) = intid(line).filter(|intid| !intid.is_sgi()) {
            self.gic.end_interrupt(intid, InterruptGroup::Group0);
        }
    }

    /// Prepare the calling core, logical CPU `cpu`: record its CPU interface,
    /// enable that interface with no priority filtering, and enable the IPI
    /// SGI there (SGI enables are banked per core).
    ///
    /// # Errors
    ///
    /// If `cpu` is at least [`MAX_CPUS`], or the GIC reports no CPU interface
    /// for this core.
    pub fn init_cpu(&mut self, cpu: u32) -> Result<(), ConfigError> {
        // SAFETY: `new` checked the distributor mapping, which is `'static`.
        let interface = unsafe { this_cpu_interface(self.distributor) };
        if interface == 0 {
            return Err(ConfigError::NoCpuInterface);
        }
        let slot = self
            .interfaces
            .get_mut(cpu as usize)
            .ok_or(ConfigError::BadCpu)?;
        *slot = interface;
        // The CPU interface's control and priority mask are banked; `setup`
        // and the group 0 enable only repeat the distributor's settings.
        self.gic.setup();
        self.gic.enable_group0(true);
        self.gic.set_priority_mask(0xff);
        self.set_enabled(IPI_LINE, true);
        Ok(())
    }

    /// Raise SGI [`IPI_SGI`] on every logical CPU of `cpu_mask` (bit `n` is
    /// CPU `n`), in one `GICD_SGIR` write.
    ///
    /// # Errors
    ///
    /// If the mask names a CPU that has not run [`Self::init_cpu`]; then no
    /// core is signalled.
    pub fn send_ipi(&mut self, cpu_mask: u32) -> Result<(), ConfigError> {
        if cpu_mask >> MAX_CPUS != 0 {
            return Err(ConfigError::UnknownCpu);
        }
        let mut target_list = 0_u16;
        for (cpu, &interface) in self.interfaces.iter().enumerate() {
            if cpu_mask & (1 << cpu) != 0 {
                if interface == 0 {
                    return Err(ConfigError::UnknownCpu);
                }
                target_list |= u16::from(interface);
            }
        }
        if target_list == 0 {
            return Ok(());
        }
        self.gic.send_sgi(
            IntId::sgi(IPI_SGI),
            SgiTarget::List {
                target_list_filter: SgiTargetListFilter::CPUTargetList,
                target_list,
            },
        );
        Ok(())
    }

    /// Enable (`enable`) or disable forwarding of `line`. SGIs and lines the
    /// GIC does not implement stay as they are.
    pub fn set_enabled(&mut self, line: u32, enable: bool) {
        if let Some(intid) = intid(line) {
            // Only a line the implementation does not have fails to enable.
            let _unimplemented = self.gic.enable_interrupt(intid, enable);
        }
    }

    /// Configure `line` as level or edge triggered.
    pub fn set_trigger(&mut self, line: u32, level: bool) -> Result<(), ConfigError> {
        let intid = intid(line)
            .filter(|intid| !intid.is_sgi())
            .ok_or(ConfigError::BadLine)?;
        self.gic
            .set_trigger(intid, if level { Trigger::Level } else { Trigger::Edge });
        Ok(())
    }

    /// Decode a `<type number flags>` device tree interrupt specifier.
    pub fn xlate(cells: &[u32]) -> u32 {
        match cells {
            [SPECIFIER_SPI, number, ..] if *number < SPI_COUNT => IntId::spi(*number).into(),
            [SPECIFIER_PPI, number, ..] if *number < PPI_COUNT => IntId::ppi(*number).into(),
            _ => INVALID_LINE,
        }
    }

    /// The trigger a `<type number flags>` specifier asks for: the flags'
    /// `IRQ_TYPE` bits 3:0 (1 and 2 edge, 4 and 8 level); anything else
    /// leaves the line's trigger as it is.
    pub fn trigger(cells: &[u32]) -> u32 {
        match cells.get(2).map(|flags| flags & IRQ_TYPE_MASK) {
            Some(IRQ_TYPE_EDGE_RISING | IRQ_TYPE_EDGE_FALLING) => TRIGGER_EDGE,
            Some(IRQ_TYPE_LEVEL_HIGH | IRQ_TYPE_LEVEL_LOW) => TRIGGER_LEVEL,
            _ => TRIGGER_FIXED,
        }
    }
}

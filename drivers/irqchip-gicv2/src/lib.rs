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
//! Resources: the Arm `GICv2` architecture specification, the GIC-400 TRM, the
//! `arm,gic` device tree binding, QEMU `hw/intc/arm_gic.c`.

use {
    arm_gic::{
        IntId, InterruptGroup, Trigger,
        gicv2::{
            GicV2,
            registers::{Gicc, Gicd},
        },
    },
    libirqchip::{INVALID_LINE, IrqChipConfig, MmioRegion, SPURIOUS},
};

/// Number of INTIDs a `GICv2` can deliver; 1020-1023 are special.
pub const LINE_COUNT: u32 = 1020;

/// Device tree specifier type of a shared peripheral interrupt.
const SPECIFIER_SPI: u32 = 0;
/// Device tree specifier type of a private peripheral interrupt.
const SPECIFIER_PPI: u32 = 1;
/// SPIs a `GICv2` can number (INTIDs 32-1019).
const SPI_COUNT: u32 = LINE_COUNT - 32;
/// PPIs per CPU (INTIDs 16-31).
const PPI_COUNT: u32 = 16;

/// Errors from [`Gic400`] operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// No controller was configured
    MissingController,
    /// The distributor or CPU interface region is missing or too small
    BadRegion,
    /// The line is not a configurable interrupt
    BadLine,
}

/// A bound `GICv2`.
pub struct Gic400 {
    gic: GicV2<'static>,
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

impl Gic400 {
    /// Bind to the mapped controller in `config`: mask every interrupt, then
    /// enable forwarding in the distributor and the CPU interface with no
    /// priority filtering.
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
        Ok(Self { gic })
    }

    /// The highest-priority pending interrupt, acknowledged, or [`SPURIOUS`].
    pub fn claim(&mut self) -> u32 {
        self.gic
            .get_and_acknowledge_interrupt(InterruptGroup::Group0)
            .map(u32::from)
            .filter(|&line: &u32| line < LINE_COUNT)
            .unwrap_or(SPURIOUS)
    }

    /// End-of-interrupt for a claimed `line`.
    pub fn complete(&mut self, line: u32) {
        if let Some(intid) = intid(line) {
            self.gic.end_interrupt(intid, InterruptGroup::Group0);
        }
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
}

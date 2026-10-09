#![no_std]

//! ABI between the nucleus and a privileged interrupt-controller component.
//!
//! An IC component is a position-independent EL1 blob bundled in the boot
//! image. Kickstart picks the one matching the device tree's root interrupt
//! controller (the CPU timer's `interrupt-parent`), relocates it into the
//! kernel half, maps the controllers' MMIO next to it and hands the nucleus
//! its [`IrqChipOps`] table. The nucleus then calls the component directly —
//! a trusted callout, like QNX interrupt callouts or Nemesis interrupt stubs.
//!
//! Every operation runs at EL1 with interrupts masked, must not block or
//! allocate, and touches only the component's own data and MMIO.
//!
//! Interrupt lines are component-defined numbers in `0..line_count`; device
//! tree interrupt specifiers become lines through [`IrqChipOps::xlate`].

/// Version of this ABI; [`IrqChipOps::abi_version`] must equal it.
pub const ABI_VERSION: u32 = 1;

/// Name of the [`IrqChipOps`] static every IC component exports.
pub const OPS_SYMBOL: &str = "IRQCHIP_OPS";

/// [`IrqChipOps::claim`] result when no line is pending.
pub const SPURIOUS: u32 = u32::MAX;

/// [`IrqChipOps::xlate`] result for a specifier the component does not understand.
pub const INVALID_LINE: u32 = u32::MAX - 1;

/// Maximum controllers (device tree nodes) one component drives.
pub const MAX_CONTROLLERS: usize = 4;

/// Maximum MMIO regions (`reg` entries) per controller.
pub const MAX_REGIONS: usize = 4;

/// One MMIO region of a controller, already mapped for EL1 access.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct MmioRegion {
    /// Kernel virtual address of the mapping
    pub virt: u64,
    /// Size in bytes
    pub size: u64,
}

/// One controller node the component drives.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Controller {
    /// Index of the matched string in the component's `compatible` list
    pub kind: u32,
    /// Number of valid entries in `regions`
    pub region_count: u32,
    /// The node's `reg` entries, in device tree order
    pub regions: [MmioRegion; MAX_REGIONS],
}

impl Controller {
    /// The valid MMIO regions.
    pub fn regions(&self) -> &[MmioRegion] {
        &self.regions[..(self.region_count as usize).min(MAX_REGIONS)]
    }
}

/// What kickstart found for the component: the root controller first, then
/// cascaded controllers in device tree order.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct IrqChipConfig {
    /// Number of valid entries in `controllers`
    pub controller_count: u32,
    pub controllers: [Controller; MAX_CONTROLLERS],
}

impl IrqChipConfig {
    /// The valid controllers, root first.
    pub fn controllers(&self) -> &[Controller] {
        &self.controllers[..(self.controller_count as usize).min(MAX_CONTROLLERS)]
    }
}

/// Maximum cells of one device tree interrupt specifier.
pub const MAX_SPECIFIER_CELLS: usize = 4;

/// A device tree interrupt specifier, relative to the controller of `kind`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Specifier {
    /// Kind of the controller the specifier is relative to
    pub kind: u32,
    /// Number of valid entries in `cells`
    pub cell_count: u32,
    pub cells: [u32; MAX_SPECIFIER_CELLS],
}

impl Specifier {
    /// The valid cells.
    pub fn cells(&self) -> &[u32] {
        &self.cells[..(self.cell_count as usize).min(MAX_SPECIFIER_CELLS)]
    }
}

/// What kickstart hands the nucleus about the loaded IC component.
///
/// Kickstart loads and maps the component at EL2, where the kernel half is
/// not yet live; the nucleus copies this record, calls [`IrqChipOps::init`]
/// and decodes the timer line itself.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PlatformHandoff {
    /// Kernel virtual address of the component's [`IrqChipOps`]
    pub ops: u64,
    pub config: IrqChipConfig,
    /// The CPU's non-secure EL1 physical timer interrupt (`CNTP`)
    pub physical_timer: Specifier,
}

/// The operation table an IC component exports as [`OPS_SYMBOL`].
///
/// All function pointers are absolute after kickstart applies the component's
/// relocations.
#[repr(C)]
pub struct IrqChipOps {
    /// Must equal [`ABI_VERSION`]
    pub abi_version: u32,
    /// Number of lines the component numbers
    pub line_count: u32,
    /// Take ownership of the controllers in `config`: record the mappings and
    /// mask every line. Returns 0 on success.
    pub init: unsafe extern "C" fn(config: *const IrqChipConfig) -> i32,
    /// The highest-priority pending line on the calling CPU, or [`SPURIOUS`].
    /// A claimed line stays claimed until [`IrqChipOps::complete`].
    pub claim: unsafe extern "C" fn() -> u32,
    /// End-of-interrupt for a line returned by `claim`.
    pub complete: unsafe extern "C" fn(line: u32),
    /// Stop `line` from being delivered.
    pub mask: unsafe extern "C" fn(line: u32),
    /// Allow `line` to be delivered.
    pub unmask: unsafe extern "C" fn(line: u32),
    /// Configure `line` as level (`level != 0`) or edge triggered. Returns 0
    /// on success, nonzero if the line's trigger is fixed.
    pub set_trigger: unsafe extern "C" fn(line: u32, level: u32) -> i32,
    /// Decode a device tree interrupt specifier of `count` cells, relative to
    /// the controller of kind `kind`, into a line, or [`INVALID_LINE`].
    pub xlate: unsafe extern "C" fn(kind: u32, cells: *const u32, count: u32) -> u32,
}

/// Name of the section holding a component's NUL-terminated `compatible`
/// strings; the position of a string is the controller kind
/// ([`Controller::kind`]).
pub const COMPATIBLE_SECTION: &str = ".irqchip_compatible";

/// Export `$ops` (an [`IrqChipOps`] expression) as the component's table and
/// declare the device tree `compatible` strings it drives, in kind order.
#[macro_export]
macro_rules! irqchip_export {
    ($ops:expr, compatible = [$($compatible:literal),+ $(,)?]) => {
        #[unsafe(no_mangle)]
        #[used]
        pub static IRQCHIP_OPS: $crate::IrqChipOps = $ops;

        const IRQCHIP_COMPATIBLE_LIST: &str = concat!($($compatible, "\0"),+);

        #[cfg_attr(target_os = "vesper", unsafe(link_section = ".irqchip_compatible"))]
        #[used]
        static IRQCHIP_COMPATIBLE: [u8; IRQCHIP_COMPATIBLE_LIST.len()] =
            *IRQCHIP_COMPATIBLE_LIST.as_bytes().first_chunk().unwrap();
    };
}

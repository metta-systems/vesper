#![no_std]

//! ABI between Kickstart and a start-method component, which starts the secondary CPU
//! cores.
//!
//! A start-method component is a position-independent blob bundled in the boot image,
//! like an interrupt-controller component (`libirqchip`). Its own section lists the
//! device tree `enable-method` values it implements. Kickstart reads the `enable-method`
//! every `/cpus` node names, relocates the one matching component and calls it at EL2,
//! where its MMU is off, so the component runs at its physical load address and every
//! address in this ABI is physical. The other components are not loaded, and the nucleus
//! never gets the table.
//!
//! Kickstart calls [`CpuStartOps::init`] once with a [`CpuStartConfig`], then
//! [`CpuStartOps::start`] for each core to release. Every operation runs with interrupts
//! masked, must not block or allocate, and touches only the component's own data and the
//! addresses the configuration names.

/// Version of this ABI; [`CpuStartOps::abi_version`] must equal it.
pub const ABI_VERSION: u32 = 1;

/// Name of the [`CpuStartOps`] static every start-method component exports.
pub const OPS_SYMBOL: &str = "CPU_START_OPS";

/// Name of the section holding a component's NUL-terminated `enable-method` strings.
pub const ENABLE_METHOD_SECTION: &str = ".cpu_start_enable_method";

/// Most CPUs one configuration describes: the supported boards have four.
pub const MAX_CPUS: usize = 4;

/// One CPU, as the device tree's `/cpus` node describes it.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuEntry {
    /// Its `MPIDR_EL1` affinity bits (its `reg`), in the register's layout
    pub affinity: u64,
    /// Physical address of its spin-table release word (`cpu-release-addr`), or 0 when it
    /// has none
    pub release_address: u64,
}

/// How a PSCI component calls the firmware.
#[repr(u32)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Conduit {
    /// No PSCI node: a component that needs one fails [`CpuStartOps::init`].
    #[default]
    None = 0,
    /// `SMC`, to EL3 firmware
    Smc = 1,
    /// `HVC`, to a hypervisor
    Hvc = 2,
}

/// What Kickstart found in the device tree for the component.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuStartConfig {
    /// Number of valid entries in `cpus`
    pub cpu_count: u32,
    /// The PSCI conduit, from the PSCI node's `method`
    pub conduit: Conduit,
    /// The PSCI `CPU_ON` function ID, or 0 without a PSCI node
    pub psci_cpu_on: u32,
    /// Reserved, zero
    pub reserved: u32,
    /// The CPUs of `/cpus`, by logical index
    pub cpus: [CpuEntry; MAX_CPUS],
}

impl CpuStartConfig {
    /// The valid CPUs, by logical index.
    #[must_use]
    pub fn cpus(&self) -> &[CpuEntry] {
        &self.cpus[..(self.cpu_count as usize).min(MAX_CPUS)]
    }

    /// The CPU with logical index `cpu`, if the configuration has it.
    #[must_use]
    pub fn cpu(&self, cpu: u32) -> Option<&CpuEntry> {
        self.cpus().get(cpu as usize)
    }
}

/// Result of [`CpuStartOps::init`] and [`CpuStartOps::start`].
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartStatus {
    /// Done: the configuration is accepted, or the core was released
    Success = 0,
    /// The configuration lacks what the method needs (a release address, a PSCI conduit)
    InvalidConfig = -1,
    /// No CPU with that logical index, or its affinity does not match
    NoSuchCpu = -2,
    /// The core is running already
    AlreadyOn = -3,
    /// A start of the core is still in progress
    Pending = -4,
    /// The entry address is not one the method accepts
    InvalidAddress = -5,
    /// The firmware refused (denied, not present, disabled)
    Denied = -6,
    /// Any other failure
    Failed = -7,
}

impl StartStatus {
    /// The status a raw return value names; any unknown value is [`StartStatus::Failed`].
    #[must_use]
    pub const fn from_raw(raw: i32) -> Self {
        match raw {
            0 => Self::Success,
            -1 => Self::InvalidConfig,
            -2 => Self::NoSuchCpu,
            -3 => Self::AlreadyOn,
            -4 => Self::Pending,
            -5 => Self::InvalidAddress,
            -6 => Self::Denied,
            _ => Self::Failed,
        }
    }
}

/// The operation table a start-method component exports as [`OPS_SYMBOL`].
///
/// All function pointers are absolute after Kickstart applies the component's
/// relocations.
#[repr(C)]
pub struct CpuStartOps {
    /// Must equal [`ABI_VERSION`]
    pub abi_version: u32,
    /// Reserved, zero
    pub reserved: u32,
    /// Take the configuration: copy what the method needs from `config` and check it.
    /// Returns a [`StartStatus`] as `i32`.
    pub init: unsafe extern "C" fn(config: *const CpuStartConfig) -> i32,
    /// Release core `cpu` (its logical index), whose affinity is `mpidr`, to start at the
    /// physical address `entry`. `context` is the physical address of that core's boot
    /// parameters: a PSCI component passes it to the core in `x0`; a spin table cannot, so
    /// the core finds its parameters by its affinity. Returns a [`StartStatus`] as `i32`.
    pub start: unsafe extern "C" fn(cpu: u32, mpidr: u64, entry: u64, context: u64) -> i32,
}

/// Export `$ops` (a [`CpuStartOps`] expression) as the component's table and declare the
/// device tree `enable-method` values it implements.
#[macro_export]
macro_rules! cpu_start_export {
    ($ops:expr, enable_method = [$($method:literal),+ $(,)?]) => {
        #[unsafe(no_mangle)]
        #[used]
        pub static CPU_START_OPS: $crate::CpuStartOps = $ops;

        const CPU_START_ENABLE_METHOD_LIST: &str = concat!($($method, "\0"),+);

        #[cfg_attr(target_os = "vesper", unsafe(link_section = ".cpu_start_enable_method"))]
        #[used]
        static CPU_START_ENABLE_METHOD: [u8; CPU_START_ENABLE_METHOD_LIST.len()] =
            *CPU_START_ENABLE_METHOD_LIST.as_bytes().first_chunk().unwrap();
    };
}

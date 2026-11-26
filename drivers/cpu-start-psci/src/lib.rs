#![no_std]

//! Starting secondary cores with PSCI `CPU_ON` (the device tree `enable-method` `psci`:
//! the Raspberry Pi 5, whose secondaries wait in TF-A).
//!
//! `CPU_ON` takes the target core's affinity in `x1` (the `MPIDR_EL1` layout), the entry's
//! physical address in `x2` and a context in `x3`, which the core finds in `x0` when it
//! starts at the entry. The call goes to the firmware through the PSCI node's conduit,
//! `SMC` or `HVC`, with the function ID the node gives (the standard SMC64 ID from PSCI 0.2
//! on).
//!
//! Resources: the Arm Power State Coordination Interface specification (DEN0022), its
//! `CPU_ON` function and return codes; the Linux `psci.yaml` binding.

use libcpu_start::{Conduit, CpuEntry, CpuStartConfig, MAX_CPUS, StartStatus};

/// PSCI return codes (DEN0022).
pub mod psci_return {
    pub const SUCCESS: i64 = 0;
    pub const NOT_SUPPORTED: i64 = -1;
    pub const INVALID_PARAMETERS: i64 = -2;
    pub const DENIED: i64 = -3;
    pub const ALREADY_ON: i64 = -4;
    pub const ON_PENDING: i64 = -5;
    pub const INTERNAL_FAILURE: i64 = -6;
    pub const NOT_PRESENT: i64 = -7;
    pub const DISABLED: i64 = -8;
    pub const INVALID_ADDRESS: i64 = -9;
}

/// The status a PSCI `CPU_ON` return code means.
#[must_use]
pub const fn cpu_on_status(code: i64) -> StartStatus {
    match code {
        psci_return::SUCCESS => StartStatus::Success,
        psci_return::INVALID_PARAMETERS => StartStatus::NoSuchCpu,
        psci_return::ALREADY_ON => StartStatus::AlreadyOn,
        psci_return::ON_PENDING => StartStatus::Pending,
        psci_return::INVALID_ADDRESS => StartStatus::InvalidAddress,
        psci_return::DENIED | psci_return::NOT_PRESENT | psci_return::DISABLED => {
            StartStatus::Denied
        }
        // NOT_SUPPORTED, INTERNAL_FAILURE and anything the specification does not define
        _ => StartStatus::Failed,
    }
}

/// A call into the firmware.
pub trait Firmware {
    /// Call `function` with `x1..x3` through `conduit`; returns `x0`.
    ///
    /// # Safety
    /// `function` and its arguments are a PSCI call the firmware implements.
    unsafe fn call(&mut self, conduit: Conduit, function: u32, x1: u64, x2: u64, x3: u64) -> i64;
}

/// The PSCI configuration: how to call the firmware and which cores exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Psci {
    conduit: Conduit,
    cpu_on: u32,
    cpu_count: usize,
    cpus: [CpuEntry; MAX_CPUS],
}

impl Psci {
    /// The configuration `config` describes; [`StartStatus::InvalidConfig`] without a
    /// conduit, a `CPU_ON` function ID or any CPU.
    pub fn new(config: &CpuStartConfig) -> Result<Self, StartStatus> {
        let cpus = config.cpus();
        if config.conduit == Conduit::None || config.psci_cpu_on == 0 || cpus.is_empty() {
            return Err(StartStatus::InvalidConfig);
        }
        let mut table = [CpuEntry::default(); MAX_CPUS];
        table[..cpus.len()].copy_from_slice(cpus);
        Ok(Self {
            conduit: config.conduit,
            cpu_on: config.psci_cpu_on,
            cpu_count: cpus.len(),
            cpus: table,
        })
    }

    /// Start core `cpu`, whose affinity is `mpidr`, at the physical address `entry`, with
    /// `context` in its `x0`.
    pub fn start(
        &self,
        cpu: u32,
        mpidr: u64,
        entry: u64,
        context: u64,
        firmware: &mut impl Firmware,
    ) -> StartStatus {
        let Some(found) = self.cpus[..self.cpu_count].get(cpu as usize) else {
            return StartStatus::NoSuchCpu;
        };
        if found.affinity != mpidr {
            return StartStatus::NoSuchCpu;
        }
        // SAFETY: `CPU_ON` with the configuration's function ID and a known core.
        let code = unsafe { firmware.call(self.conduit, self.cpu_on, mpidr, entry, context) };
        cpu_on_status(code)
    }
}

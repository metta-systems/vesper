#![no_std]

//! Releasing secondary cores through a spin table (the device tree `enable-method`
//! `spin-table`: the Raspberry Pi 3 and 4, and QEMU's `raspi3b`/`raspi4b`).
//!
//! The firmware parks each secondary core in a loop that waits for a nonzero value in its
//! 64-bit release word (`cpu-release-addr`), then jumps to it. Releasing a core writes the
//! entry's physical address to that word, cleans it to the Point of Coherency (the parked
//! core may read memory with its caches off), completes the write with `dsb sy` and wakes
//! the parked cores with `sev`. The spin table passes the core no argument, so `context` is
//! not used: the core finds its parameters by its affinity.
//!
//! Resources: the Linux `enable-method` binding (`cpus.yaml`, "spin-table"), the Raspberry
//! Pi `armstub8.S`, QEMU `hw/arm/raspi.c` (`write_smpboot64`).

use libcpu_start::{CpuEntry, CpuStartConfig, MAX_CPUS, StartStatus};

/// What releasing a core does to the machine.
pub trait ReleaseWord {
    /// Store `entry` to the release word at physical address `word`, make it visible to a
    /// core reading with its caches off, and wake the parked cores.
    ///
    /// # Safety
    /// `word` is a spin-table release word the configuration named.
    unsafe fn release(&mut self, word: u64, entry: u64);
}

/// The spin table: the release word of every CPU, by logical index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpinTable {
    cpu_count: usize,
    cpus: [CpuEntry; MAX_CPUS],
}

impl SpinTable {
    /// The table `config` describes; [`StartStatus::InvalidConfig`] without any CPU or
    /// when a CPU has no release word.
    pub fn new(config: &CpuStartConfig) -> Result<Self, StartStatus> {
        let cpus = config.cpus();
        if cpus.is_empty() || cpus.iter().any(|cpu| cpu.release_address == 0) {
            return Err(StartStatus::InvalidConfig);
        }
        let mut table = [CpuEntry::default(); MAX_CPUS];
        table[..cpus.len()].copy_from_slice(cpus);
        Ok(Self {
            cpu_count: cpus.len(),
            cpus: table,
        })
    }

    /// Release core `cpu`, whose affinity is `mpidr`, to start at the physical address
    /// `entry` (nonzero, 4-byte aligned: an instruction).
    pub fn start(
        &self,
        cpu: u32,
        mpidr: u64,
        entry: u64,
        machine: &mut impl ReleaseWord,
    ) -> StartStatus {
        let Some(found) = self.cpus[..self.cpu_count].get(cpu as usize) else {
            return StartStatus::NoSuchCpu;
        };
        if found.affinity != mpidr {
            return StartStatus::NoSuchCpu;
        }
        if entry == 0 || !entry.is_multiple_of(4) {
            return StartStatus::InvalidAddress;
        }
        // SAFETY: the release word came from the configuration.
        unsafe {
            machine.release(found.release_address, entry);
        }
        StartStatus::Success
    }
}

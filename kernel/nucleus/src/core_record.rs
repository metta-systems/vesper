//! The per-core record (SMP design, "Per-core record", C3(a)).
//!
//! Kickstart carves one record per core from accounted memory, sized from the device
//! tree's `/cpus`, and points each core's `TPIDR_EL1` at its own record (an EL1-only
//! register: no component sees it). The nucleus names its core by the record's logical
//! index, never by `MPIDR_EL1`: the Raspberry Pi 5 numbers its cores in Aff1.
//!
//! Implementation status: the record holds the core's identity and trap stack; the
//! per-core scheduling, timer, FP and IPI state moves in with the kernel lock (SMP2).

use aarch64_cpu::registers::{Read, TPIDR_EL1};

/// One core's record. Kickstart writes it before the core first runs at EL1; it is
/// read-only afterwards.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreRecord {
    /// The core's logical index: its position among the CPUs of `/cpus` (SMP C9(a))
    pub index: u32,
    /// Reserved, zero
    pub reserved: u32,
    /// The core's `MPIDR_EL1` affinity bits (`libcpu::smp::CoreAffinity`)
    pub affinity: u64,
    /// Kernel virtual address of the top of the core's `SP_EL1` trap stack
    pub trap_stack_top: u64,
}

/// The record of the core this runs on.
///
/// # Panics
///
/// If `TPIDR_EL1` names no record: Kickstart sets it before the core runs at EL1.
pub fn current() -> &'static CoreRecord {
    let address = TPIDR_EL1.get();
    assert_ne!(address, 0, "TPIDR_EL1 names no per-core record");
    // SAFETY: Kickstart points `TPIDR_EL1` at this core's record, in memory it carved
    // for the nucleus and mapped in the kernel half, and nothing writes the record
    // after the core starts.
    unsafe { &*(address as *const CoreRecord) }
}

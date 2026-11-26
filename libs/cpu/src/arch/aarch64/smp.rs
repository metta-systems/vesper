//! Core identity: the affinity fields of `MPIDR_EL1`.
//!
//! A core is named by all four affinity levels, not by `Aff0` alone: the Raspberry Pi 5's
//! Cortex-A76 numbers its cores in `Aff1` (`reg` 0x000, 0x100, 0x200, 0x300) and reads
//! `Aff0` as 0 on every core.

use {
    aarch64_cpu::registers::MPIDR_EL1,
    tock_registers::{Read, fields::Field},
};

/// The bits of one `MPIDR_EL1` field, in place.
const fn field_bits(field: Field<u64, MPIDR_EL1::Register>) -> u64 {
    field.mask << field.shift
}

/// The low affinity levels, `Aff2..Aff0` in bits 23:0: one contiguous run, so assembly can
/// use it as a logical immediate.
pub const LOW_AFFINITY_MASK: u64 =
    field_bits(MPIDR_EL1::Aff2) | field_bits(MPIDR_EL1::Aff1) | field_bits(MPIDR_EL1::Aff0);

/// The bits of `MPIDR_EL1` that name a core: `Aff3` in 39:32 and `Aff2..Aff0` in 23:0.
/// `U`, `MT` and the RES1 bit 31 are left out. PSCI `CPU_ON` takes its target core in this
/// same layout.
pub const AFFINITY_MASK: u64 = field_bits(MPIDR_EL1::Aff3) | LOW_AFFINITY_MASK;

/// The affinity of the core that boots; the others park. Zero on every supported board:
/// Aff0 on the Raspberry Pi 3 and 4, Aff1 on the Pi 5.
pub const BOOT_CORE_AFFINITY: u64 = 0;

/// A core's affinity, the `MPIDR_EL1` affinity fields in their register positions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CoreAffinity(u64);

impl CoreAffinity {
    /// The affinity in an `MPIDR_EL1` value.
    #[must_use]
    pub const fn from_mpidr(mpidr: u64) -> Self {
        Self(mpidr & AFFINITY_MASK)
    }

    /// The affinity bits, in `MPIDR_EL1` (and PSCI `CPU_ON` target) layout.
    #[must_use]
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// The affinity of the core this runs on.
    #[inline(always)]
    #[must_use]
    pub fn current() -> Self {
        Self::from_mpidr(MPIDR_EL1.get())
    }
}

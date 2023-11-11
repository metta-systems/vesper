#[inline(always)]
pub fn core_id() -> u64 {
    use aarch64_cpu::registers::{Readable, MPIDR_EL1};

    const CORE_MASK: u64 = 0x3;
    MPIDR_EL1.get() & CORE_MASK
}

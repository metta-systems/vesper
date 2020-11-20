pub use self::memory::{PhysAddr, VirtAddr};
use aarch64_cpu::{asm, regs::*};

#[cfg(not(feature = "no_boot"))] // Move this to nucleus??
pub mod boot;
pub mod smp;

/// Expose CPU-specific no-op opcode.
pub use asm::nop;

/// Loop forever in sleep mode.
#[inline]
pub fn endless_sleep() -> ! {
    loop {
        asm::wfe();
    }
}

#[inline]
pub fn flushcache(address: usize) {
    unsafe {
        asm!("dc ivac, {addr}", addr = in(reg) address);
    }
}

#[inline]
pub fn read_cpu_id() -> u64 {
    const CORE_MASK: u64 = 0x3; // This is RasPi-specific?
    MPIDR_EL1.get() & CORE_MASK
}

#[inline]
pub fn current_el() -> u32 {
    CurrentEL.get()
}

pub fn read_translation_table_base() -> PhysAddr {
    TTBR0_EL1.get_baddr().into()
}

pub fn write_translation_table_base(base: PhysAddr) {
    TTBR0_EL1.set_baddr(base.into());
}

pub fn read_translation_control() -> u64 {
    TCR_EL1.get()
}

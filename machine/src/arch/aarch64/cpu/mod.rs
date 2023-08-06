use aarch64_cpu::asm;

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

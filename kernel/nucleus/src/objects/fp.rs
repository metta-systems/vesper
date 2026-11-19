//! The FP/SIMD register file as the transition logic sees it
//! (`doc/agentic/simd-design.md`, "The hardware interface").
//!
//! The object layer decides when to save, load or zero the registers and
//! calls an [`FpUnit`]; the architecture supplies the unit as
//! [`ArchObjects::Fp`](crate::objects::ArchObjects::Fp). The nucleus tests
//! use [`FakeFpUnit`], a register file in memory, so they can check what
//! every transition leaves in the registers.

use libexception::arch::aarch64::{FpPreserved, FpState};

/// The hardware FP/SIMD register file: V0–V31, FPCR and FPSR.
///
/// Kernel code touches these registers only through this interface. Every
/// method leaves the registers it does not name unchanged.
pub trait FpUnit {
    /// Copy the whole register file into `state`.
    fn save(state: &mut FpState);
    /// Write the whole register file from `state`.
    fn load(state: &FpState);
    /// Write zeros to the whole register file.
    fn zero();
    /// Copy V8–V15 (all 128 bits), FPCR and FPSR into `preserved`.
    fn save_preserved(preserved: &mut FpPreserved);
    /// Write V8–V15, FPCR and FPSR from `preserved`, and zero every other
    /// register.
    fn load_preserved(preserved: &FpPreserved);
}

/// The kernel's unit until the save and load routines exist (S2): FP stays
/// trapped at EL0 and EL1 for every `AddressSpace`, so no code can observe
/// the registers and there is nothing to move.
#[cfg(not(test))]
pub struct TrappedFpUnit;

#[cfg(not(test))]
impl FpUnit for TrappedFpUnit {
    fn save(_state: &mut FpState) {}
    fn load(_state: &FpState) {}
    fn zero() {}
    fn save_preserved(_preserved: &mut FpPreserved) {}
    fn load_preserved(_preserved: &FpPreserved) {}
}

#[cfg(test)]
pub use fake::FakeFpUnit;

#[cfg(test)]
mod fake {
    use {
        super::FpUnit,
        core::cell::UnsafeCell,
        libexception::arch::aarch64::{FpPreserved, FpState},
    };

    /// The register file of [`FakeFpUnit`].
    struct Registers(UnsafeCell<FpState>);

    // SAFETY: the nucleus tests run on one core, one test at a time, and no
    // reference into the cell outlives a single method call.
    unsafe impl Sync for Registers {}

    static REGISTERS: Registers = Registers(UnsafeCell::new(FpState::ZERO));

    /// An FP/SIMD register file in memory, for the nucleus tests: the
    /// transition logic runs against it, and a test reads or sets what is "in
    /// the hardware" with [`registers`](Self::registers) and
    /// [`set_registers`](Self::set_registers).
    pub struct FakeFpUnit;

    impl FakeFpUnit {
        /// What the register file holds.
        pub fn registers() -> FpState {
            // SAFETY: see `Registers`; the copy ends the access.
            unsafe { *REGISTERS.0.get() }
        }

        /// Make the register file hold `state`, as code running in an FP frame
        /// would.
        pub fn set_registers(state: FpState) {
            // SAFETY: see `Registers`.
            unsafe { *REGISTERS.0.get() = state }
        }
    }

    impl FpUnit for FakeFpUnit {
        fn save(state: &mut FpState) {
            *state = Self::registers();
        }

        fn load(state: &FpState) {
            Self::set_registers(*state);
        }

        fn zero() {
            Self::set_registers(FpState::ZERO);
        }

        fn save_preserved(preserved: &mut FpPreserved) {
            *preserved = Self::registers().preserved();
        }

        fn load_preserved(preserved: &FpPreserved) {
            Self::set_registers(FpState::from_preserved(preserved));
        }
    }
}

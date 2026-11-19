//! FP/SIMD register state a Thread keeps while it runs in an `AddressSpace`
//! that allows FP (see `doc/agentic/simd-design.md`).

use core::mem::{align_of, offset_of, size_of};

/// The whole FP/SIMD register file: V0–V31, FPCR and FPSR.
///
/// The kernel loads and stores it only through its FP save and load
/// routines; ordinary kernel code never touches the registers it describes.
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FpState {
    /// V0–V31, each 128 bits.
    pub vectors: [u128; 32],
    /// Floating-point Control Register.
    pub fpcr: u32,
    /// Floating-point Status Register.
    pub fpsr: u32,
}

impl FpState {
    /// The state a Thread starts with, and the state a call target is
    /// entered with: every register zero.
    pub const ZERO: Self = Self {
        vectors: [0; 32],
        fpcr: 0,
        fpsr: 0,
    };

    /// The state a frame is owed after a Call returns: its own `preserved`
    /// registers, everything else zero.
    pub const fn from_preserved(preserved: &FpPreserved) -> Self {
        let mut state = Self::ZERO;
        let mut index = 0;
        while index < 8 {
            let [low, high] = preserved.q8_q15[index];
            state.vectors[8 + index] = (high as u128) << 64 | low as u128;
            index += 1;
        }
        state.fpcr = preserved.fpcr;
        state.fpsr = preserved.fpsr;
        state
    }

    /// The registers a Call preserves for its caller.
    pub const fn preserved(&self) -> FpPreserved {
        let mut q8_q15 = [[0; 2]; 8];
        let mut index = 0;
        while index < 8 {
            let value = self.vectors[8 + index];
            #[allow(
                clippy::cast_possible_truncation,
                reason = "splitting a 128-bit register into its two halves"
            )]
            {
                q8_q15[index] = [value as u64, (value >> 64) as u64];
            }
            index += 1;
        }
        FpPreserved {
            q8_q15,
            fpcr: self.fpcr,
            fpsr: self.fpsr,
        }
    }
}

impl Default for FpState {
    fn default() -> Self {
        Self::ZERO
    }
}

/// What a Call keeps for a caller in an FP `AddressSpace`: all 128 bits of
/// V8–V15, FPCR and FPSR. AAPCS64 preserves only the low halves of V8–V15,
/// but Rust's `asm!` treats unnamed V8–V15 as preserved in full.
///
/// Each register is stored as its low and high 64-bit halves, so the record
/// keeps 8-byte alignment inside the call record.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FpPreserved {
    /// V8–V15 as `[low, high]` halves.
    pub q8_q15: [[u64; 2]; 8],
    /// Floating-point Control Register.
    pub fpcr: u32,
    /// Floating-point Status Register.
    pub fpsr: u32,
}

impl FpPreserved {
    /// Nothing preserved: the record of a call out of an integer-only frame.
    pub const ZERO: Self = Self {
        q8_q15: [[0; 2]; 8],
        fpcr: 0,
        fpsr: 0,
    };
}

impl Default for FpPreserved {
    fn default() -> Self {
        Self::ZERO
    }
}

const _: () = {
    assert!(size_of::<FpState>() == 528);
    assert!(align_of::<FpState>() == 16);
    assert!(offset_of!(FpState, vectors) == 0);
    assert!(offset_of!(FpState, fpcr) == 512);
    assert!(offset_of!(FpState, fpsr) == 516);
    assert!(size_of::<FpPreserved>() == 136);
    assert!(align_of::<FpPreserved>() == 8);
    assert!(offset_of!(FpPreserved, q8_q15) == 0);
    assert!(offset_of!(FpPreserved, fpcr) == 128);
    assert!(offset_of!(FpPreserved, fpsr) == 132);
};

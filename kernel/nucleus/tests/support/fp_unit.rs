//! The hardware FP/SIMD routines against the real register file: what each
//! leaves in the registers, and that each puts the FP trap back.

use {
    crate::objects::{arch::HardwareFpUnit, fp::FpUnit},
    aarch64_cpu::registers::{CPACR_EL1, Read},
    libexception::arch::aarch64::{FpPreserved, FpState},
};

/// Distinct values in every register, and FPCR and FPSR values that read
/// back as written (round towards zero; every cumulative exception flag).
fn pattern(tag: u128) -> FpState {
    let mut state = FpState::ZERO;
    for (index, register) in state.vectors.iter_mut().enumerate() {
        let index = index as u128;
        *register = (tag << 120) | (index << 64) | (0xA5A5_0000 + index);
    }
    state.fpcr = 0x00C0_0000;
    state.fpsr = 0x1F;
    state
}

fn saved() -> FpState {
    let mut state = FpState::ZERO;
    HardwareFpUnit::save(&mut state);
    state
}

#[test_case]
fn load_then_save_round_trips_the_whole_register_file() {
    let values = pattern(1);
    HardwareFpUnit::load(&values);
    assert_eq!(saved(), values);
    HardwareFpUnit::zero();
    assert_eq!(saved(), FpState::ZERO);
}

#[test_case]
fn the_preserved_routines_keep_v8_to_v15_and_the_controls_and_zero_the_rest() {
    let values = pattern(2);
    HardwareFpUnit::load(&values);
    let mut preserved = FpPreserved::ZERO;
    HardwareFpUnit::save_preserved(&mut preserved);
    assert_eq!(preserved, values.preserved());

    HardwareFpUnit::load(&pattern(3));
    HardwareFpUnit::load_preserved(&preserved);
    assert_eq!(saved(), FpState::from_preserved(&preserved));
    HardwareFpUnit::zero();
}

#[test_case]
fn every_routine_puts_the_fp_trap_back_as_it_found_it() {
    let before = CPACR_EL1.read(CPACR_EL1::FPEN);
    let values = pattern(4);
    let mut state = FpState::ZERO;
    let mut preserved = FpPreserved::ZERO;
    HardwareFpUnit::load(&values);
    assert_eq!(CPACR_EL1.read(CPACR_EL1::FPEN), before);
    HardwareFpUnit::save(&mut state);
    assert_eq!(CPACR_EL1.read(CPACR_EL1::FPEN), before);
    HardwareFpUnit::save_preserved(&mut preserved);
    assert_eq!(CPACR_EL1.read(CPACR_EL1::FPEN), before);
    HardwareFpUnit::load_preserved(&preserved);
    assert_eq!(CPACR_EL1.read(CPACR_EL1::FPEN), before);
    HardwareFpUnit::zero();
    assert_eq!(CPACR_EL1.read(CPACR_EL1::FPEN), before);
}

//! The `AArch64` FP/SIMD register file: the only FP instructions in the
//! kernel (`doc/agentic/simd-design.md`, "The hardware interface").
//!
//! The kernel is built soft-float, so the routines are naked functions in
//! assembly under `.arch_extension fp, simd`. `_audit-fp-simd` allows FP/SIMD
//! instructions in the nucleus only inside the `fp_unit_*` functions.
//!
//! Each routine opens `CPACR_EL1.FPEN` around itself and puts back the value
//! it found, so the FP trap is whatever the last installed `AddressSpace`
//! needs (`install_translation_context`).

use {
    super::super::fp::FpUnit,
    aarch64_cpu::{
        asm::barrier,
        registers::{CPACR_EL1, Read, Write},
    },
    core::{arch::naked_asm, mem::offset_of},
    libexception::arch::aarch64::{FpPreserved, FpState},
};

// The routines below address the fields by these offsets.
const _: () = {
    assert!(offset_of!(FpState, vectors) == 0);
    assert!(offset_of!(FpState, fpcr) == 512);
    assert!(offset_of!(FpState, fpsr) == 516);
    assert!(offset_of!(FpPreserved, q8_q15) == 0);
    assert!(offset_of!(FpPreserved, fpcr) == 128);
    assert!(offset_of!(FpPreserved, fpsr) == 132);
};

/// Store V0–V31, FPCR and FPSR into `*state`. Clobbers x1.
///
/// # Safety
///
/// `state` is valid for writes and 16-aligned, and FP/SIMD is open at EL1.
#[unsafe(naked)]
unsafe extern "C" fn fp_unit_save(state: *mut FpState) {
    naked_asm!(
        ".arch_extension fp",
        ".arch_extension simd",
        "stp q0, q1, [x0, #0]",
        "stp q2, q3, [x0, #32]",
        "stp q4, q5, [x0, #64]",
        "stp q6, q7, [x0, #96]",
        "stp q8, q9, [x0, #128]",
        "stp q10, q11, [x0, #160]",
        "stp q12, q13, [x0, #192]",
        "stp q14, q15, [x0, #224]",
        "stp q16, q17, [x0, #256]",
        "stp q18, q19, [x0, #288]",
        "stp q20, q21, [x0, #320]",
        "stp q22, q23, [x0, #352]",
        "stp q24, q25, [x0, #384]",
        "stp q26, q27, [x0, #416]",
        "stp q28, q29, [x0, #448]",
        "stp q30, q31, [x0, #480]",
        "mrs x1, fpcr",
        "str w1, [x0, #512]",
        "mrs x1, fpsr",
        "str w1, [x0, #516]",
        "ret",
    );
}

/// Write V0–V31, FPCR and FPSR from `*state`. Clobbers x1.
///
/// # Safety
///
/// `state` is valid for reads and 16-aligned, and FP/SIMD is open at EL1.
#[unsafe(naked)]
unsafe extern "C" fn fp_unit_load(state: *const FpState) {
    naked_asm!(
        ".arch_extension fp",
        ".arch_extension simd",
        "ldp q0, q1, [x0, #0]",
        "ldp q2, q3, [x0, #32]",
        "ldp q4, q5, [x0, #64]",
        "ldp q6, q7, [x0, #96]",
        "ldp q8, q9, [x0, #128]",
        "ldp q10, q11, [x0, #160]",
        "ldp q12, q13, [x0, #192]",
        "ldp q14, q15, [x0, #224]",
        "ldp q16, q17, [x0, #256]",
        "ldp q18, q19, [x0, #288]",
        "ldp q20, q21, [x0, #320]",
        "ldp q22, q23, [x0, #352]",
        "ldp q24, q25, [x0, #384]",
        "ldp q26, q27, [x0, #416]",
        "ldp q28, q29, [x0, #448]",
        "ldp q30, q31, [x0, #480]",
        "ldr w1, [x0, #512]",
        "msr fpcr, x1",
        "ldr w1, [x0, #516]",
        "msr fpsr, x1",
        "ret",
    );
}

/// Zero every V register but V8–V15: shared by [`fp_unit_zero`] and
/// [`fp_unit_load_preserved`], which reach it with `bl`.
///
/// # Safety
///
/// FP/SIMD is open at EL1. Only an `fp_unit_*` routine calls it.
#[unsafe(naked)]
unsafe extern "C" fn fp_unit_zero_scratch() {
    naked_asm!(
        ".arch_extension fp",
        ".arch_extension simd",
        "movi v0.2d, #0",
        "movi v1.2d, #0",
        "movi v2.2d, #0",
        "movi v3.2d, #0",
        "movi v4.2d, #0",
        "movi v5.2d, #0",
        "movi v6.2d, #0",
        "movi v7.2d, #0",
        "movi v16.2d, #0",
        "movi v17.2d, #0",
        "movi v18.2d, #0",
        "movi v19.2d, #0",
        "movi v20.2d, #0",
        "movi v21.2d, #0",
        "movi v22.2d, #0",
        "movi v23.2d, #0",
        "movi v24.2d, #0",
        "movi v25.2d, #0",
        "movi v26.2d, #0",
        "movi v27.2d, #0",
        "movi v28.2d, #0",
        "movi v29.2d, #0",
        "movi v30.2d, #0",
        "movi v31.2d, #0",
        "ret",
    );
}

/// Write zeros to V0–V31, FPCR and FPSR. Clobbers x9 and x30.
///
/// # Safety
///
/// FP/SIMD is open at EL1.
#[unsafe(naked)]
unsafe extern "C" fn fp_unit_zero() {
    naked_asm!(
        ".arch_extension fp",
        ".arch_extension simd",
        "mov x9, x30",
        "bl {zero_scratch}",
        "movi v8.2d, #0",
        "movi v9.2d, #0",
        "movi v10.2d, #0",
        "movi v11.2d, #0",
        "movi v12.2d, #0",
        "movi v13.2d, #0",
        "movi v14.2d, #0",
        "movi v15.2d, #0",
        "msr fpcr, xzr",
        "msr fpsr, xzr",
        "ret x9",
        zero_scratch = sym fp_unit_zero_scratch,
    );
}

/// Store V8–V15, FPCR and FPSR into `*preserved`. Clobbers x0 and x1.
///
/// `st1` of `.2d` elements needs only the record's 8-byte alignment, and
/// stores each register low half first, as `[low, high]`.
///
/// # Safety
///
/// `preserved` is valid for writes and 8-aligned, and FP/SIMD is open at EL1.
#[unsafe(naked)]
unsafe extern "C" fn fp_unit_save_preserved(preserved: *mut FpPreserved) {
    naked_asm!(
        ".arch_extension fp",
        ".arch_extension simd",
        "st1 {{v8.2d, v9.2d, v10.2d, v11.2d}}, [x0], #64",
        "st1 {{v12.2d, v13.2d, v14.2d, v15.2d}}, [x0], #64",
        "mrs x1, fpcr",
        "str w1, [x0, #0]",
        "mrs x1, fpsr",
        "str w1, [x0, #4]",
        "ret",
    );
}

/// Write V8–V15, FPCR and FPSR from `*preserved`, as
/// [`fp_unit_save_preserved`] stored them, and zero every other V register.
/// Clobbers x0, x1, x9 and x30.
///
/// # Safety
///
/// `preserved` is valid for reads and 8-aligned, and FP/SIMD is open at EL1.
#[unsafe(naked)]
unsafe extern "C" fn fp_unit_load_preserved(preserved: *const FpPreserved) {
    naked_asm!(
        ".arch_extension fp",
        ".arch_extension simd",
        "mov x9, x30",
        "bl {zero_scratch}",
        "ld1 {{v8.2d, v9.2d, v10.2d, v11.2d}}, [x0], #64",
        "ld1 {{v12.2d, v13.2d, v14.2d, v15.2d}}, [x0], #64",
        "ldr w1, [x0, #0]",
        "msr fpcr, x1",
        "ldr w1, [x0, #4]",
        "msr fpsr, x1",
        "ret x9",
        zero_scratch = sym fp_unit_zero_scratch,
    );
}

/// Run `routine` with FP/SIMD open at EL1, then put `CPACR_EL1` back as it
/// was.
fn with_fp_open(routine: impl FnOnce()) {
    let found = CPACR_EL1.get();
    CPACR_EL1.modify(CPACR_EL1::FPEN::TrapNothing);
    barrier::isb(barrier::SY);
    routine();
    CPACR_EL1.set(found);
    barrier::isb(barrier::SY);
}

/// Set the FP trap for the `AddressSpace` about to be entered: open for an
/// FP `AddressSpace`, trapped at EL0 and EL1 otherwise. The caller's context
/// synchronization (the `isb` after the `TTBR0_EL1` write) completes it.
///
/// With `fp_entry_trap` the kernel keeps FP trapped while it runs: this only
/// records what the [`FpEntryGuard`] opens at the exit.
pub fn set_fp_access(fp_simd: bool) {
    #[cfg(feature = "fp_entry_trap")]
    entry_guard::exit_fp_open().store(fp_simd, core::sync::atomic::Ordering::Relaxed);
    #[cfg(not(feature = "fp_entry_trap"))]
    CPACR_EL1.modify(if fp_simd {
        CPACR_EL1::FPEN::TrapNothing
    } else {
        CPACR_EL1::FPEN::TrapEl0El1
    });
}

/// `FPEN` toggles the entry guard made on this core: 0 without `fp_entry_trap`.
pub fn entry_toggles() -> u64 {
    #[cfg(feature = "fp_entry_trap")]
    return entry_guard::toggles().load(core::sync::atomic::Ordering::Relaxed);
    #[cfg(not(feature = "fp_entry_trap"))]
    0
}

/// The F11 entry guard (`doc/agentic/simd-design.md`): with
/// `fp_entry_trap`, a kernel entry from EL0 traps FP/SIMD at once and its
/// exit opens it again for an FP frame, so a stray kernel FP instruction
/// traps at EL1 instead of corrupting the frame's live registers. Without the
/// feature it does nothing, and the kernel runs with the trap of the frame it
/// was entered from.
///
/// Held for the whole of every handler whose entry can return to a Thread
/// (`lower_aarch64_*` from EL0, `current_el0_*` from trusted `EL1t`), first
/// thing in it: the handler's prologue runs before it, but is integer code
/// like all of the soft-float kernel. Entries taken in the kernel itself
/// (`current_elx_*`) return into the kernel and take no guard. The exit value
/// and the toggle count are the core's own, in its record.
pub struct FpEntryGuard;

impl FpEntryGuard {
    /// Trap FP/SIMD for the kernel entry that has just begun.
    #[must_use]
    pub fn enter() -> Self {
        #[cfg(feature = "fp_entry_trap")]
        entry_guard::enter();
        Self
    }
}

#[cfg(feature = "fp_entry_trap")]
impl Drop for FpEntryGuard {
    /// Open FP/SIMD again when the frame this entry returns to is an FP
    /// frame: the one it came from, or the last one installed. The `eret`
    /// that follows synchronizes the change.
    fn drop(&mut self) {
        entry_guard::exit();
    }
}

#[cfg(feature = "fp_entry_trap")]
mod entry_guard {
    use {
        aarch64_cpu::{
            asm::barrier,
            registers::{CPACR_EL1, Read, Write},
        },
        core::sync::atomic::{AtomicBool, AtomicU64, Ordering},
    };

    /// Whether the frame this core's current entry returns to is an FP frame.
    pub fn exit_fp_open() -> &'static AtomicBool {
        &crate::objects::core_record::current().fp_exit_open
    }

    /// `FPEN` writes the guard made on this core, at entries and exits.
    pub fn toggles() -> &'static AtomicU64 {
        &crate::objects::core_record::current().fp_entry_toggles
    }

    pub fn enter() {
        let open = CPACR_EL1.matches_all(CPACR_EL1::FPEN::TrapNothing);
        exit_fp_open().store(open, Ordering::Relaxed);
        if open {
            CPACR_EL1.modify(CPACR_EL1::FPEN::TrapEl0El1);
            barrier::isb(barrier::SY);
            toggles().fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn exit() {
        if exit_fp_open().load(Ordering::Relaxed) {
            CPACR_EL1.modify(CPACR_EL1::FPEN::TrapNothing);
            toggles().fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// The hardware register file.
pub struct HardwareFpUnit;

impl FpUnit for HardwareFpUnit {
    fn save(state: &mut FpState) {
        // SAFETY: `state` is a valid, 16-aligned `FpState`, and FP is open
        // around the routine.
        with_fp_open(|| unsafe { fp_unit_save(state) });
    }

    fn load(state: &FpState) {
        // SAFETY: as in `save`; the routine reads only `state`.
        with_fp_open(|| unsafe { fp_unit_load(state) });
    }

    fn zero() {
        // SAFETY: FP is open around the routine.
        with_fp_open(|| unsafe { fp_unit_zero() });
    }

    fn save_preserved(preserved: &mut FpPreserved) {
        // SAFETY: `preserved` is a valid, 8-aligned `FpPreserved`, and FP is
        // open around the routine.
        with_fp_open(|| unsafe { fp_unit_save_preserved(preserved) });
    }

    fn load_preserved(preserved: &FpPreserved) {
        // SAFETY: as in `save_preserved`; the routine reads only `preserved`.
        with_fp_open(|| unsafe { fp_unit_load_preserved(preserved) });
    }

    fn entry_toggles() -> u64 {
        entry_toggles()
    }
}

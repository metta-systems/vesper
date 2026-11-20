#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! An FP/SIMD component, and `fp-test`'s FP worker. It is built for the
//! hard-float target (it is on the Justfile's `fp_components` list), so the
//! compiler uses FP/SIMD registers freely, and image-build's FP scan flags
//! it, so it is loaded into an `AddressSpace` that allows FP/SIMD.
//!
//! Its argument is its role (see `fp-test-protocol`):
//!
//! - `ROLE_SPIN` fills every V register, FPCR and FPSR with sentinels and
//!   checks them in a loop forever, so only preemption stops it;
//! - `ROLE_SCENARIOS` calls the other FP worker and the integer-only
//!   `fp-probe` with sentinels in place, then takes a fault, and checks what
//!   comes back each time.
//!
//! Its exports check that they are entered with every register zero:
//! `zero_check_entry` (the Call target) and `fault_check_entry` (the fault
//! handler). Both then overwrite all of them before Returning, so a caller
//! gets back only what the kernel restores.
//!
//! The register work is in naked functions: compiled code may use the V
//! registers at any point.

use {
    core::{hint, sync::atomic::Ordering},
    fp_test_protocol::{FpInit, FpReport, ROLE_SCENARIOS, ROLE_SEED_STRIDE, fp_init, fp_report},
    libobject::{
        export::{self, ExportRecord, PpcResult},
        fault::{FaultAction, FaultKind},
        thread::ThreadReturnKey,
    },
    libuser::semihosting as semi,
};

libuser::entry!(main);

// ── Register sentinels ─────────────────────────────────────────────────────
// `v<n>` holds `x19 + n` in its low half and the complement in its high half.
// Each check leaves the failing register number in `x11` and branches to the
// local label `3`.

/// Load `v<n>` with its sentinel.
macro_rules! load_sentinel {
    ($n:literal) => {
        concat!(
            "add x9, x19, #",
            $n,
            "\n",
            "fmov d",
            $n,
            ", x9\n",
            "mvn x10, x9\n",
            "mov v",
            $n,
            ".d[1], x10\n",
        )
    };
}

/// Check that `v<n>` holds its sentinel.
macro_rules! check_sentinel {
    ($n:literal) => {
        concat!(
            "mov x11, #",
            $n,
            "\n",
            "add x9, x19, #",
            $n,
            "\n",
            "fmov x10, d",
            $n,
            "\n",
            "cmp x9, x10\n",
            "b.ne 3f\n",
            "mvn x9, x9\n",
            "mov x10, v",
            $n,
            ".d[1]\n",
            "cmp x9, x10\n",
            "b.ne 3f\n",
        )
    };
}

/// Check that `v<n>` is zero.
macro_rules! check_zero {
    ($n:literal) => {
        concat!(
            "mov x11, #",
            $n,
            "\n",
            "fmov x10, d",
            $n,
            "\n",
            "cbnz x10, 3f\n",
            "mov x10, v",
            $n,
            ".d[1]\n",
            "cbnz x10, 3f\n",
        )
    };
}

/// `$m` for `v8..v15`, the registers a Call preserves.
macro_rules! preserved {
    ($m:ident) => {
        concat!(
            $m!(8),
            $m!(9),
            $m!(10),
            $m!(11),
            $m!(12),
            $m!(13),
            $m!(14),
            $m!(15),
        )
    };
}

/// `$m` for `v0..v7` and `v16..v31`, the registers a Call zeroes.
macro_rules! clobbered {
    ($m:ident) => {
        concat!(
            $m!(0),
            $m!(1),
            $m!(2),
            $m!(3),
            $m!(4),
            $m!(5),
            $m!(6),
            $m!(7),
            $m!(16),
            $m!(17),
            $m!(18),
            $m!(19),
            $m!(20),
            $m!(21),
            $m!(22),
            $m!(23),
            $m!(24),
            $m!(25),
            $m!(26),
            $m!(27),
            $m!(28),
            $m!(29),
            $m!(30),
            $m!(31),
        )
    };
}

/// `$m` for every V register.
macro_rules! all_v {
    ($m:ident) => {
        concat!(clobbered!($m), preserved!($m))
    };
}

/// Check that FPCR equals `x20` and FPSR equals `x21`.
macro_rules! check_control {
    () => {
        concat!(
            "mov x11, #32\n",
            "mrs x10, fpcr\n",
            "cmp x10, x20\n",
            "b.ne 3f\n",
            "mov x11, #33\n",
            "mrs x10, fpsr\n",
            "cmp x10, x21\n",
            "b.ne 3f\n",
        )
    };
}

/// Save the callee-saved registers a routine uses, and FPCR into `x23`.
macro_rules! save_frame {
    () => {
        concat!(
            "stp x29, x30, [sp, #-128]!\n",
            "mov x29, sp\n",
            "stp x19, x20, [sp, #16]\n",
            "stp x21, x22, [sp, #32]\n",
            "stp x23, x24, [sp, #48]\n",
            "stp d8, d9, [sp, #64]\n",
            "stp d10, d11, [sp, #80]\n",
            "stp d12, d13, [sp, #96]\n",
            "stp d14, d15, [sp, #112]\n",
            "mrs x23, fpcr\n",
        )
    };
}

/// Turn the check result into a code in `x0` (label `3` is the failure),
/// restore what [`save_frame`] saved and return.
macro_rules! finish_frame {
    () => {
        concat!(
            "mov x0, #0\n",
            "b 4f\n",
            "3:\n",
            "add x0, x11, #1\n",
            "4:\n",
            "msr fpcr, x23\n",
            "ldp d14, d15, [sp, #112]\n",
            "ldp d12, d13, [sp, #96]\n",
            "ldp d10, d11, [sp, #80]\n",
            "ldp d8, d9, [sp, #64]\n",
            "ldp x23, x24, [sp, #48]\n",
            "ldp x21, x22, [sp, #32]\n",
            "ldp x19, x20, [sp, #16]\n",
            "ldp x29, x30, [sp], #128\n",
            "ret\n",
        )
    };
}

/// An export's entry check: every V register, FPCR and FPSR zero, with the
/// result code in `x0`; then every one of them overwritten, and on to
/// `{inner}` with the call's `x1..x7` unchanged.
macro_rules! checked_entry {
    () => {
        concat!(
            all_v!(check_zero),
            "mov x11, #32\n",
            "mrs x10, fpcr\n",
            "cbnz x10, 3f\n",
            "mov x11, #33\n",
            "mrs x10, fpsr\n",
            "cbnz x10, 3f\n",
            "mov x0, #0\n",
            "b 4f\n",
            "3:\n",
            "add x0, x11, #1\n",
            "4:\n",
            "movz x19, #0xD1D1, lsl #48\n",
            all_v!(load_sentinel),
            // Round toward zero, flush to zero; every cumulative flag set.
            "movz x9, #0x01C0, lsl #16\n",
            "msr fpcr, x9\n",
            "mov x9, #0x1F\n",
            "msr fpsr, x9\n",
            "b {inner}\n",
        )
    };
}

/// Fill every V register from `seed`, set FPCR to `control` and FPSR to
/// `status`, and check them in a loop forever, adding one to `*passes` per
/// pass. On a mismatch, store its code in `*failure` and stop counting.
///
/// # Safety
///
/// `passes` and `failure` are writable for as long as the Thread runs.
#[unsafe(naked)]
unsafe extern "C" fn spin(
    seed: u64,
    control: u64,
    status: u64,
    passes: *mut u64,
    failure: *mut u64,
) -> ! {
    core::arch::naked_asm!(concat!(
        "mov x19, x0\n",
        "mov x20, x1\n",
        "mov x21, x2\n",
        all_v!(load_sentinel),
        "msr fpcr, x20\n",
        "msr fpsr, x21\n",
        "2:\n",
        all_v!(check_sentinel),
        check_control!(),
        "ldr x10, [x3]\n",
        "add x10, x10, #1\n",
        "str x10, [x3]\n",
        "b 2b\n",
        "3:\n",
        "add x11, x11, #1\n",
        "str x11, [x4]\n",
        "5:\n",
        "b 5b\n",
    ))
}

/// Fill every V register from `seed`, set FPCR to `control` and FPSR to
/// `status`, Call `key` with no arguments, store the callee's `r0` in
/// `*callee`, and check what the Return left: `v8..v15`, FPCR and FPSR as
/// before, every other V register zero. Returns the check code.
///
/// # Safety
///
/// `key` names an Invocation whose target Returns; `callee` is writable.
#[unsafe(naked)]
unsafe extern "C" fn call_preserving(
    seed: u64,
    control: u64,
    status: u64,
    key: u64,
    callee: *mut u64,
) -> u64 {
    core::arch::naked_asm!(concat!(
        save_frame!(),
        "mov x19, x0\n",
        "mov x20, x1\n",
        "mov x21, x2\n",
        "mov x22, x4\n",
        "mov x24, x3\n",
        all_v!(load_sentinel),
        "msr fpcr, x20\n",
        "msr fpsr, x21\n",
        // Invocation.Call: the key, operation 0, six zero arguments. The
        // Return restores x19..x30 and SP.
        "mov x0, x24\n",
        "mov x1, xzr\n",
        "mov x2, xzr\n",
        "mov x3, xzr\n",
        "mov x4, xzr\n",
        "mov x5, xzr\n",
        "mov x6, xzr\n",
        "mov x7, xzr\n",
        "svc #0\n",
        "str x1, [x22]\n",
        "mov x11, #34\n",
        "cbnz x0, 3f\n",
        preserved!(check_sentinel),
        clobbered!(check_zero),
        check_control!(),
        finish_frame!(),
    ))
}

/// Fill every V register from `seed`, set FPCR to `control` and FPSR to
/// `status`, take a fault (`brk`, which the fault handler skips), and check
/// that all of them came back. Returns the check code.
///
/// # Safety
///
/// This component's fault handler skips the `brk`.
#[unsafe(naked)]
unsafe extern "C" fn fault_preserving(seed: u64, control: u64, status: u64) -> u64 {
    core::arch::naked_asm!(concat!(
        save_frame!(),
        "mov x19, x0\n",
        "mov x20, x1\n",
        "mov x21, x2\n",
        all_v!(load_sentinel),
        "msr fpcr, x20\n",
        "msr fpsr, x21\n",
        "brk #1\n",
        all_v!(check_sentinel),
        check_control!(),
        finish_frame!(),
    ))
}

/// FPCR for `seed`: default NaN, and a rounding mode from the seed.
fn fpcr_for(seed: u64) -> u64 {
    (1 << 25) | ((seed & 0b11) << 22)
}

/// FPSR for `seed`: a nonzero set of cumulative exception flags.
fn fpsr_for(seed: u64) -> u64 {
    (seed & 0x1E) | 1
}

/// Record the export adapter's `Thread.Return` key (idempotent).
fn setup_return_key(init: &FpInit) {
    export::init_return_key(&ThreadReturnKey::provisioned(
        u32::try_from(init.guard).unwrap_or(0),
        u8::try_from(init.size_bits).unwrap_or(0),
    ));
}

/// Return `r0` from an export.
fn complete(r0: u64) -> ! {
    // SAFETY: only reached from an export entry, entered by Invocation.Call.
    unsafe { export::complete_export(PpcResult { r0, r1: 0 }) }
}

/// The Call target installed in the other worker's table: Returns what its
/// entry check saw.
#[unsafe(naked)]
extern "C" fn zero_check_entry(
    _zero_check: u64,
    _badge: u64,
    _input0: u64,
    _input1: u64,
    _input2: u64,
    _input3: u64,
    _input4: u64,
    _input5: u64,
) -> ! {
    core::arch::naked_asm!(checked_entry!(), inner = sym zero_check_body)
}

extern "C" fn zero_check_body(zero_check: u64) -> ! {
    // SAFETY: the builder fills this component's init page before it runs.
    setup_return_key(unsafe { fp_init() });
    complete(zero_check)
}

/// This component's fault handler, installed at `KeySlot::FAULT_HANDLER`:
/// records what its entry check saw and the syndrome, and skips the
/// instruction.
#[unsafe(naked)]
extern "C" fn fault_check_entry(
    _zero_check: u64,
    _badge: u64,
    _kind: u64,
    _syndrome: u64,
    _fault_address: u64,
    _pc: u64,
    _sp: u64,
    _depth: u64,
) -> ! {
    core::arch::naked_asm!(checked_entry!(), inner = sym fault_check_body)
}

extern "C" fn fault_check_body(zero_check: u64, _badge: u64, kind: u64, syndrome: u64) -> ! {
    // SAFETY: the builder fills and maps both pages before the worker runs.
    let (init, report) = unsafe { (fp_init(), fp_report()) };
    setup_return_key(init);
    report.fault_handler.store(zero_check, Ordering::Relaxed);
    report.fault_syndrome.store(syndrome, Ordering::Relaxed);
    let action = if FaultKind::from_wire(kind) == Some(FaultKind::CpuException) {
        FaultAction::Skip
    } else {
        FaultAction::Terminate
    };
    complete(action as u64)
}

// The image's export table: `vesper-image-build` reads these records from the
// ELF `.exports` section.
#[cfg(target_os = "vesper")]
#[used]
#[unsafe(link_section = ".exports")]
static ZERO_CHECK_EXPORT: ExportRecord = ExportRecord::new("zero_check_entry", zero_check_entry);
#[cfg(target_os = "vesper")]
#[used]
#[unsafe(link_section = ".exports")]
static FAULT_CHECK_EXPORT: ExportRecord = ExportRecord::new("fault_check_entry", fault_check_entry);

/// A floating-point computation the compiler cannot fold away: the sum of
/// `1 / k²` for `k` in `1..=terms`, which approaches `π² / 6`.
#[inline(never)]
fn basel_sum(terms: u32) -> f64 {
    let mut sum = 0.0_f64;
    for k in 1..=terms {
        let k = f64::from(hint::black_box(k));
        sum += 1.0 / (k * k);
    }
    sum
}

/// The Call and fault scenarios, with sentinels from `seed`.
fn scenarios(init: &FpInit, report: &FpReport, seed: u64) {
    setup_return_key(init);
    let (control, status) = (fpcr_for(seed), fpsr_for(seed));
    let mut callee = 0;
    // SAFETY: the builder installed an Invocation of the other worker's
    // `zero_check_entry`, which Returns.
    let call_fp = unsafe { call_preserving(seed, control, status, init.call_fp, &raw mut callee) };
    report.call_fp.store(call_fp, Ordering::Relaxed);
    report.call_fp_callee.store(callee, Ordering::Relaxed);
    // SAFETY: as above, `fp-probe`'s `probe_call_entry`.
    let call_integer =
        unsafe { call_preserving(seed, control, status, init.call_integer, &raw mut callee) };
    report.call_integer.store(call_integer, Ordering::Relaxed);
    report.call_integer_callee.store(callee, Ordering::Relaxed);
    // SAFETY: the builder installed `fault_check_entry` as this
    // component's fault handler; it skips the `brk`.
    let fault = unsafe { fault_preserving(seed, control, status) };
    report.fault.store(fault, Ordering::Relaxed);
    semi::println!(
        "fp-worker: check codes: Call into FP {call_fp}, Call into integer-only {call_integer}, fault {fault}"
    );
    // The compiler's own FP code still works afterwards.
    let sum = basel_sum(1000);
    semi::println!("fp-worker: basel sum {sum}");
    hint::black_box(sum);
    report.done.store(1, Ordering::Release);
}

fn main(role: u64) -> ! {
    // SAFETY: the builder fills and maps both pages before the worker runs.
    let (init, report) = unsafe { (fp_init(), fp_report()) };
    let seed = init.seed.wrapping_add(role.wrapping_mul(ROLE_SEED_STRIDE));
    if role == ROLE_SCENARIOS {
        scenarios(init, report, seed);
        loop {
            hint::spin_loop();
        }
    }
    // SAFETY: the report page stays mapped for as long as the Thread runs.
    unsafe {
        spin(
            seed,
            fpcr_for(seed),
            fpsr_for(seed),
            report.spin_passes.as_ptr(),
            report.spin_failure.as_ptr(),
        )
    }
}

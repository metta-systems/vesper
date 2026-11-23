#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! fp-trap-test's EL0 component. It executes one FP/SIMD instruction (see
//! `fp-trap-protocol`); a trap is delivered to this component's own fault
//! handler, which records the syndrome and skips the instruction. The probe
//! then signals whether it trapped as an FP/SIMD access, and parks forever.
//!
//! Its argument is its role: in an integer-only `AddressSpace`
//! (`ROLE_INTEGER`) the instruction must trap; in an FP `AddressSpace`
//! (`ROLE_FP`) it must execute, and the probe also has the kernel execute it
//! while serving this frame (`kernel_probe`) and reports that outcome too.
//!
//! `probe_call_entry` runs the same probe as a Call target and Returns the
//! outcome; `fp-test` calls it from an FP component. `nop_entry` Returns at
//! once; `fp-bench` times Calls into it.

use {
    core::sync::atomic::{AtomicU64, Ordering},
    fp_trap_protocol::{
        KERNEL_NOT_TRAPPED_BIT, KERNEL_TRAPPED_BIT, NOT_TRAPPED_BIT, ROLE_FP, TRAPPED_BIT,
        kernel_probe, probe, probe_init, trapped,
    },
    libobject::{
        NotificationKey, RawKey,
        export::{self, PpcResult},
        fault::{FaultAction, FaultKind},
        ppc_export,
        thread::ThreadReturnKey,
    },
    libuser::semihosting as semi,
};

libuser::entry!(main);

/// The syndrome of the last CPU-exception fault the handler took.
static LAST_SYNDROME: AtomicU64 = AtomicU64::new(0);

/// Record the export adapter's `Thread.Return` key (idempotent).
fn setup_return_key() {
    // SAFETY: the builder fills this component's init page before it runs.
    let init = unsafe { probe_init() };
    export::init_return_key(&ThreadReturnKey::provisioned(
        u32::try_from(init.guard).unwrap_or(0),
        u8::try_from(init.size_bits).unwrap_or(0),
    ));
}

/// This component's fault handler: record the syndrome, skip the instruction.
extern "C" fn fault_body(
    _dummy0: u64,
    _dummy1: u64,
    kind: u64,
    syndrome: u64,
    _fault_address: u64,
    _pc: u64,
    _sp: u64,
    _depth: u64,
) -> PpcResult {
    setup_return_key();
    if FaultKind::from_wire(kind) == Some(FaultKind::CpuException) {
        LAST_SYNDROME.store(syndrome, Ordering::Release);
    }
    PpcResult::pair(FaultAction::Skip as u64, 0)
}

ppc_export!(
    /// The fault handler entry installed at `KeySlot::FAULT_HANDLER`.
    pub fn fault_entry => fault_body
);

/// The probe as a Call target: execute the instruction and Return
/// [`TRAPPED_BIT`] or [`NOT_TRAPPED_BIT`].
extern "C" fn probe_call_body(
    _dummy0: u64,
    _badge: u64,
    _input0: u64,
    _input1: u64,
    _input2: u64,
    _input3: u64,
    _input4: u64,
    _input5: u64,
) -> PpcResult {
    setup_return_key();
    LAST_SYNDROME.store(0, Ordering::Release);
    probe();
    let bits = if trapped(LAST_SYNDROME.load(Ordering::Acquire)) {
        TRAPPED_BIT
    } else {
        NOT_TRAPPED_BIT
    };
    PpcResult::pair(bits, 0)
}

ppc_export!(
    /// The probe's Call entry (`fp-test` calls it from an FP component).
    pub fn probe_call_entry => probe_call_body
);

/// An integer-only Call target that Returns at once (`fp-bench` times Calls
/// into it from an FP component).
extern "C" fn nop_body(
    _dummy0: u64,
    _badge: u64,
    _input0: u64,
    _input1: u64,
    _input2: u64,
    _input3: u64,
    _input4: u64,
    _input5: u64,
) -> PpcResult {
    setup_return_key();
    PpcResult::pair(0, 0)
}

ppc_export!(
    /// The integer-only Call target of `fp-bench`.
    pub fn nop_entry => nop_body
);

fn main(role: u64) -> ! {
    setup_return_key();
    // SAFETY: as in `setup_return_key`.
    let init = unsafe { probe_init() };
    let result = NotificationKey::from_key(RawKey::from_wire(init.result));
    semi::println!("fp-probe: executing an FP/SIMD instruction at EL0");
    probe();
    let mut bits = if trapped(LAST_SYNDROME.load(Ordering::Acquire)) {
        TRAPPED_BIT
    } else {
        NOT_TRAPPED_BIT
    };
    if role == ROLE_FP {
        semi::println!("fp-probe: asking the kernel to execute it while serving this FP frame");
        bits |= if trapped(kernel_probe()) {
            KERNEL_TRAPPED_BIT
        } else {
            KERNEL_NOT_TRAPPED_BIT
        };
    }
    result
        .signal(bits)
        .unwrap_or_else(|error| panic!("fp-probe: signal failed: {:?}", error.code()));
    match result.await_bits(NotificationKey::WAIT_INFINITE) {
        Ok(bits) => panic!("fp-probe: unexpected wakeup with bits {bits:#x}"),
        Err(error) => panic!("fp-probe: park failed: {:?}", error.code()),
    }
}

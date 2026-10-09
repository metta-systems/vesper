#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! Negative-test EL0 component. It executes one FP/SIMD instruction (see
//! `fp-trap-protocol`); the trap is delivered to this component's own fault
//! handler, which records the syndrome and skips the instruction. The probe
//! then signals whether it trapped as an FP/SIMD access, and parks forever.

use {
    core::sync::atomic::{AtomicU64, Ordering},
    fp_trap_protocol::{NOT_TRAPPED_BIT, TRAPPED_BIT, probe, probe_init, trapped},
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
    PpcResult {
        r0: FaultAction::Skip as u64,
        r1: 0,
    }
}

ppc_export!(
    /// The fault handler entry installed at `KeySlot::FAULT_HANDLER`.
    pub fn fault_entry => fault_body
);

fn main(_argument: u64) -> ! {
    setup_return_key();
    // SAFETY: as in `setup_return_key`.
    let init = unsafe { probe_init() };
    let result = NotificationKey::from_key(RawKey::from_wire(init.result));
    semi::println!("fp-probe: executing an FP/SIMD instruction at EL0");
    probe();
    let bits = if trapped(LAST_SYNDROME.load(Ordering::Acquire)) {
        TRAPPED_BIT
    } else {
        NOT_TRAPPED_BIT
    };
    result
        .signal(bits)
        .unwrap_or_else(|error| panic!("fp-probe: signal failed: {:?}", error.code()));
    match result.wait(NotificationKey::WAIT_INFINITE) {
        Ok(bits) => panic!("fp-probe: unexpected wakeup with bits {bits:#x}"),
        Err(error) => panic!("fp-probe: park failed: {:?}", error.code()),
    }
}

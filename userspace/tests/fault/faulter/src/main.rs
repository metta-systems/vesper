#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! fault-test's component with a fault handler. Each Thread gets its role in
//! `x0` (see `fault-protocol`), sets the handler mode and faults on purpose;
//! the handler (installed at `KeySlot::FAULT_HANDLER`) runs on the faulting
//! Thread, records what it was told and answers as the mode says.

use {
    core::sync::atomic::{AtomicU64, Ordering},
    fault_protocol::{
        BLOCKED_BIT, MODE_BLOCK, MODE_NESTED, MODE_RETRY_ONCE, MODE_SKIP, MODE_TERMINATE,
        ROLE_BLOCKED, ROLE_NESTED, ROLE_SEQUENCE, ROLE_TERMINATE, ROLE_WHILE_BUSY, ROLE_WITNESS,
        SEQUENCE_BIT, WITNESS_BIT, breakpoint, faulter_init, is_breakpoint,
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

/// How the handler answers the next fault (a `MODE_*` value).
static MODE: AtomicU64 = AtomicU64::new(MODE_SKIP);
/// Faults the handler took since the last reset.
static FAULTS: AtomicU64 = AtomicU64::new(0);
/// What the handler was told about the last fault.
static LAST_KIND: AtomicU64 = AtomicU64::new(u64::MAX);
static LAST_SYNDROME: AtomicU64 = AtomicU64::new(0);
static LAST_PC: AtomicU64 = AtomicU64::new(0);
static FIRST_PC: AtomicU64 = AtomicU64::new(0);

fn notification(key: u64) -> NotificationKey {
    NotificationKey::from_key(RawKey::from_wire(key))
}

/// The table's `Thread.Return` key, also recorded for the export adapter.
fn return_key() -> ThreadReturnKey {
    // SAFETY: the builder fills this component's init page before it runs.
    let init = unsafe { faulter_init() };
    let key = ThreadReturnKey::provisioned(
        u32::try_from(init.guard).unwrap_or(0),
        u8::try_from(init.size_bits).unwrap_or(0),
    );
    export::init_return_key(&key);
    key
}

/// The fault handler: runs on the faulting Thread, on its own stack.
extern "C" fn fault_body(
    _dummy0: u64,
    _dummy1: u64,
    kind: u64,
    syndrome: u64,
    _fault_address: u64,
    pc: u64,
    _sp: u64,
    _depth: u64,
) -> PpcResult {
    return_key();
    let count = FAULTS.fetch_add(1, Ordering::AcqRel) + 1;
    if count == 1 {
        FIRST_PC.store(pc, Ordering::Release);
    }
    LAST_KIND.store(kind, Ordering::Release);
    LAST_SYNDROME.store(syndrome, Ordering::Release);
    LAST_PC.store(pc, Ordering::Release);
    let action = match MODE.load(Ordering::Acquire) {
        MODE_RETRY_ONCE if count == 1 => FaultAction::Retry,
        MODE_SKIP | MODE_RETRY_ONCE => FaultAction::Skip,
        MODE_NESTED => {
            // A fault inside the handler: nobody takes it.
            breakpoint();
            panic!("faulter: resumed after a fault inside the handler");
        }
        MODE_BLOCK => {
            // SAFETY: as in `return_key`.
            let init = unsafe { faulter_init() };
            notification(init.block)
                .wait(NotificationKey::WAIT_INFINITE)
                .unwrap_or_else(|error| panic!("faulter: block wait failed: {:?}", error.code()));
            FaultAction::Skip
        }
        // `MODE_TERMINATE`, and anything unknown.
        _ => FaultAction::Terminate,
    };
    PpcResult {
        r0: action as u64,
        r1: 0,
    }
}

ppc_export!(
    /// The fault handler entry installed at `KeySlot::FAULT_HANDLER`.
    pub fn fault_entry => fault_body
);

/// Arm the handler for one fault in `mode`.
fn arm(mode: u64) {
    FAULTS.store(0, Ordering::Release);
    MODE.store(mode, Ordering::Release);
}

fn sequence(return_key: &ThreadReturnKey) {
    // Skip: the handler is told what faulted, and the Thread continues.
    arm(MODE_SKIP);
    breakpoint();
    assert_eq!(FAULTS.load(Ordering::Acquire), 1);
    assert_eq!(
        FaultKind::from_wire(LAST_KIND.load(Ordering::Acquire)),
        Some(FaultKind::CpuException)
    );
    assert!(is_breakpoint(LAST_SYNDROME.load(Ordering::Acquire)));
    semi::println!("faulter: skipped a breakpoint at EL0");

    // Retry: the same instruction faults again, then the handler skips it.
    arm(MODE_RETRY_ONCE);
    breakpoint();
    assert_eq!(FAULTS.load(Ordering::Acquire), 2, "retry must re-execute");
    assert_eq!(
        LAST_PC.load(Ordering::Acquire),
        FIRST_PC.load(Ordering::Acquire),
        "retry must re-execute the same instruction"
    );
    semi::println!("faulter: retried a breakpoint, then skipped it");

    // A Return with nothing to return to is a fault at its `svc`.
    arm(MODE_SKIP);
    // SAFETY: at depth 0 this Return cannot succeed; the fault handler skips
    // the `svc` and this code continues.
    let outcome = unsafe { return_key.return_from_invocation(0, 0) };
    // Skipped, the `svc` "returns" the registers it was issued with, which
    // never decode as success.
    assert!(outcome.is_err(), "a skipped Return cannot report success");
    assert_eq!(FAULTS.load(Ordering::Acquire), 1);
    assert_eq!(
        FaultKind::from_wire(LAST_KIND.load(Ordering::Acquire)),
        Some(FaultKind::IllegalReturn)
    );
    assert_eq!(LAST_SYNDROME.load(Ordering::Acquire), 0);
    semi::println!("faulter: an illegal Return was delivered as a fault and skipped");
}

fn main(role: u64) -> ! {
    let return_key = return_key();
    // SAFETY: as in `return_key`.
    let init = unsafe { faulter_init() };
    let done = notification(init.done);
    match role {
        ROLE_SEQUENCE => {
            sequence(&return_key);
            done.signal(SEQUENCE_BIT)
        }
        ROLE_TERMINATE => {
            arm(MODE_TERMINATE);
            breakpoint();
            panic!("faulter: resumed after the handler terminated it");
        }
        ROLE_NESTED => {
            arm(MODE_NESTED);
            breakpoint();
            panic!("faulter: resumed after an unhandled fault");
        }
        ROLE_BLOCKED => {
            arm(MODE_BLOCK);
            breakpoint();
            assert_eq!(FAULTS.load(Ordering::Acquire), 1);
            semi::println!("faulter: the blocked handler finished and skipped");
            done.signal(BLOCKED_BIT)
        }
        ROLE_WHILE_BUSY => {
            breakpoint();
            panic!("faulter: resumed after a fault during a busy handler");
        }
        ROLE_WITNESS => done.signal(WITNESS_BIT),
        _ => panic!("faulter: unknown role {role}"),
    }
    .unwrap_or_else(|error| panic!("faulter: done signal failed: {:?}", error.code()));
    match notification(init.park).wait(NotificationKey::WAIT_INFINITE) {
        Ok(bits) => panic!("faulter: unexpected wakeup with bits {bits:#x}"),
        Err(error) => panic!("faulter: park failed: {:?}", error.code()),
    }
}

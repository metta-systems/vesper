//! Same-Thread PPC Call/Return end-to-end through the real SVC path.
//!
//! The boot Thread calls an Invocation whose target is the Bounce
//! `AddressSpace`. The target stack is the upper half of Bounce's probe page
//! at `PROBE_VA`: a low user VA whose backing differs in the source root, so
//! reading it proves the migration switched translation contexts. The target
//! captures its entry registers, then issues a real `Thread.Return` through
//! its own table's Slot(1) sentinel.
//!
//! This is a trusted `EL1t` functionality test of the provisional x9 transport,
//! scrubbing, status inheritance and continuation restore; it is not
//! hostile-EL0 confinement or FP/SIMD trap validation.

use {
    crate::translation::{self, PROBE_VA},
    aarch64_cpu::registers::{Readable, TTBR0_EL1},
    core::{
        arch::{asm, naked_asm},
        sync::atomic::{AtomicBool, AtomicU64, Ordering},
    },
    libobject::{
        CapError, DebugConsoleKey, InconsistencyReason, RawKey, decode_syscall_result,
        export::{self, PpcResult},
        invocation::InvocationKey,
        ppc_export,
        thread::ThreadReturnKey,
    },
};

/// Extent `[base, end)` inside Bounce's probe page, clear of its marker word.
pub const STACK_BASE: u64 = PROBE_VA + 0x800;
pub const STACK_END: u64 = PROBE_VA + 0x1000;
pub const MINIMUM_HEADROOM: u64 = 0x400;

/// Captured target-entry words: x0..x30, NZCV, DAIF.
const CAPTURE_WORDS: usize = 33;
const CAPTURE_BYTES: u64 = 272;

/// Bounce-table-local `CurrentReturnOnly` key, recorded by [`component_init`].
static RETURN_KEY: AtomicU64 = AtomicU64::new(0);
/// Bounce-table-local `DebugConsole` key, recorded by [`component_init`].
static DEBUG_CONSOLE_KEY: AtomicU64 = AtomicU64::new(0);
/// Successful `DebugConsole` writes issued by [`export_body`] inside Bounce.
static CONSOLE_WRITES: AtomicU64 = AtomicU64::new(0);
static CAPTURE: [AtomicU64; CAPTURE_WORDS] = [const { AtomicU64::new(0) }; CAPTURE_WORDS];
static ENTRY_SP: AtomicU64 = AtomicU64::new(0);
/// Select the target's Return path: the library helper, or raw asm with junk
/// x4..x7 operands.
static RETURN_VIA_LIBRARY: AtomicBool = AtomicBool::new(false);
/// Junk Return operands x4..x7; Return must ignore them.
const IGNORED: [u64; 4] = [0xDEAD_0004, u64::MAX, 0, 0x7777_0007];

/// Target entry: capture every register and NZCV/DAIF onto the target stack
/// before any compiled code runs, then hand the capture to the body.
#[unsafe(naked)]
pub extern "C" fn target_entry() -> ! {
    naked_asm!(
        "sub sp, sp, #272",
        "stp x0, x1, [sp, #0]",
        "stp x2, x3, [sp, #16]",
        "stp x4, x5, [sp, #32]",
        "stp x6, x7, [sp, #48]",
        "stp x8, x9, [sp, #64]",
        "stp x10, x11, [sp, #80]",
        "stp x12, x13, [sp, #96]",
        "stp x14, x15, [sp, #112]",
        "stp x16, x17, [sp, #128]",
        "stp x18, x19, [sp, #144]",
        "stp x20, x21, [sp, #160]",
        "stp x22, x23, [sp, #176]",
        "stp x24, x25, [sp, #192]",
        "stp x26, x27, [sp, #208]",
        "stp x28, x29, [sp, #224]",
        "str x30, [sp, #240]",
        "mrs x16, nzcv",
        "str x16, [sp, #248]",
        "mrs x16, daif",
        "str x16, [sp, #256]",
        "mov x0, sp",
        "b {body}",
        body = sym target_body,
    )
}

/// Publish the capture, compute `(r0, r1)` from target-only state and Return.
extern "C" fn target_body(capture: *const u64) -> ! {
    for (index, slot) in CAPTURE.iter().enumerate() {
        // SAFETY: target_entry stored CAPTURE_WORDS words at `capture` on the
        // live target stack, which stays mapped for this call.
        slot.store(unsafe { capture.add(index).read() }, Ordering::Release);
    }
    ENTRY_SP.store(capture as u64 + CAPTURE_BYTES, Ordering::Release);
    // SAFETY: the Bounce root maps PROBE_VA to its retained RW probe Frame.
    let r0 = unsafe { (PROBE_VA as *const u64).read_volatile() };
    let r1 = TTBR0_EL1.get();
    let key = RETURN_KEY.load(Ordering::Acquire);
    if RETURN_VIA_LIBRARY.load(Ordering::Acquire) {
        // SAFETY: completing this Call abandons the target stack and context.
        let Err(error) = (unsafe {
            ThreadReturnKey::from_key(RawKey::from_wire(key)).return_from_invocation(r0, r1)
        });
        panic!("PPC target: library Return failed: {:?}", error.code());
    }
    let (status, word1, word2): (u64, u64, u64);
    // SAFETY: raw Thread.Return SVC. Success never returns here: the kernel
    // resumes the source. Every integer register it may clobber is declared.
    unsafe {
        asm!(
            "svc #0",
            inlateout("x0") key => status,
            inlateout("x1") 0_u64 => word1,
            inlateout("x2") r0 => word2,
            inlateout("x3") r1 => _,
            inlateout("x4") IGNORED[0] => _,
            inlateout("x5") IGNORED[1] => _,
            inlateout("x6") IGNORED[2] => _,
            inlateout("x7") IGNORED[3] => _,
            lateout("x8") _, lateout("x9") _, lateout("x10") _, lateout("x11") _,
            lateout("x12") _, lateout("x13") _, lateout("x14") _, lateout("x15") _,
            lateout("x16") _, lateout("x17") _, lateout("x18") _,
        );
    }
    match decode_syscall_result((status, word1, word2)) {
        Ok(_) => panic!("PPC target: Thread.Return returned SUCCESS locally"),
        Err(error) => panic!("PPC target: Thread.Return rejected: {:?}", error.code()),
    }
}

/// Raw source-side outcome of one Call SVC.
pub struct CallOutcome {
    pub status: u64,
    pub word1: u64,
    pub word2: u64,
    /// 1: x19..x30 sentinels, execution SP and source NZCV intact;
    /// 2: a sentinel or SP was corrupted; 4: NZCV was not restored.
    pub check: u64,
    /// OR of x3..x18 after resumption (zero after a scrubbed Return).
    pub volatile_or: u64,
}

/// Invoke `key` with `op`, six inputs in x2..x7 and target SP in x9.
///
/// Spills the compiler's x19..x30, plants sentinels and NZCV = Z|C, then
/// checks them after resumption against a private execution-stack copy.
#[inline(never)]
pub fn call_with_registers(key: RawKey, op: u64, args: [u64; 6], target_sp: u64) -> CallOutcome {
    let (status, word1, word2, check, volatile_or): (u64, u64, u64, u64, u64);
    // SAFETY: the aligned 96-byte execution-stack spill is created and
    // removed in this block; callee-saved registers/LR are restored before
    // Rust resumes. All integer registers the kernel may scrub are declared.
    unsafe {
        asm!(
            "sub sp, sp, #96",
            "stp x19, x20, [sp, #0]", "stp x21, x22, [sp, #16]",
            "stp x23, x24, [sp, #32]", "stp x25, x26, [sp, #48]",
            "stp x27, x28, [sp, #64]", "stp x29, x30, [sp, #80]",
            "mov x19, #0x1919", "mov x20, #0x2020", "mov x21, #0x2121",
            "mov x22, #0x2222", "mov x23, #0x2323", "mov x24, #0x2424",
            "mov x25, #0x2525", "mov x26, #0x2626", "mov x27, #0x2727",
            "mov x28, #0x2828", "mov x29, sp", "mov x30, #0x3030",
            "cmp x19, x19",
            "svc #0",
            "orr x16, x16, x3", "orr x16, x16, x4", "orr x16, x16, x5",
            "orr x16, x16, x6", "orr x16, x16, x7", "orr x16, x16, x8",
            "orr x16, x16, x9", "orr x16, x16, x10", "orr x16, x16, x11",
            "orr x16, x16, x12", "orr x16, x16, x13", "orr x16, x16, x14",
            "orr x16, x16, x15", "orr x16, x16, x17", "orr x16, x16, x18",
            "mrs x9, nzcv", "mov x10, #0x60000000", "cmp x9, x10", "b.ne 4f",
            "mov x9, #0x1919", "cmp x19, x9", "b.ne 2f",
            "mov x9, #0x2020", "cmp x20, x9", "b.ne 2f",
            "mov x9, #0x2121", "cmp x21, x9", "b.ne 2f",
            "mov x9, #0x2222", "cmp x22, x9", "b.ne 2f",
            "mov x9, #0x2323", "cmp x23, x9", "b.ne 2f",
            "mov x9, #0x2424", "cmp x24, x9", "b.ne 2f",
            "mov x9, #0x2525", "cmp x25, x9", "b.ne 2f",
            "mov x9, #0x2626", "cmp x26, x9", "b.ne 2f",
            "mov x9, #0x2727", "cmp x27, x9", "b.ne 2f",
            "mov x9, #0x2828", "cmp x28, x9", "b.ne 2f",
            "mov x9, sp", "cmp x29, x9", "b.ne 2f",
            "mov x9, #0x3030", "cmp x30, x9", "b.ne 2f",
            "mov x8, #1", "b 5f",
            "2:", "mov x8, #2", "b 5f",
            "4:", "mov x8, #4",
            "5:",
            "ldp x19, x20, [sp, #0]", "ldp x21, x22, [sp, #16]",
            "ldp x23, x24, [sp, #32]", "ldp x25, x26, [sp, #48]",
            "ldp x27, x28, [sp, #64]", "ldp x29, x30, [sp, #80]",
            "add sp, sp, #96",
            inlateout("x0") key.to_wire() => status,
            inlateout("x1") op => word1,
            inlateout("x2") args[0] => word2,
            inlateout("x3") args[1] => _,
            inlateout("x4") args[2] => _,
            inlateout("x5") args[3] => _,
            inlateout("x6") args[4] => _,
            inlateout("x7") args[5] => _,
            inlateout("x9") target_sp => _,
            inlateout("x16") 0_u64 => volatile_or,
            lateout("x8") check,
            lateout("x10") _, lateout("x11") _, lateout("x12") _, lateout("x13") _,
            lateout("x14") _, lateout("x15") _, lateout("x17") _, lateout("x18") _,
        );
    }
    CallOutcome {
        status,
        word1,
        word2,
        check,
        volatile_or,
    }
}

fn read_daif() -> u64 {
    let daif: u64;
    // SAFETY: reads the current interrupt masks only.
    unsafe {
        asm!("mrs {daif}, daif", daif = out(reg) daif, options(nomem, nostack));
    }
    daif
}

/// Recoverable Call rejections through real SVC: no migration, no scrub.
pub fn assert_call_rejected(key: RawKey, op: u64, target_sp: u64, expected: CapError) {
    let ttbr_before = TTBR0_EL1.get();
    let outcome = call_with_registers(key, op, [0x22, 0x33, 0x44, 0x55, 0x66, 0x77], target_sp);
    assert_eq!(
        (outcome.status, outcome.word1, outcome.word2),
        expected.code()
    );
    assert_eq!(outcome.check, 1, "rejected Call corrupted source context");
    assert_eq!(TTBR0_EL1.get(), ttbr_before, "rejected Call switched roots");
}

/// The full same-Thread round trip. Returns the delivered `(r0, r1)`.
///
/// `via_library` uses `InvocationKey::call` and the target's
/// `ThreadReturnKey` helper; otherwise the instrumented raw asm pair, which
/// also checks source register preservation and scrubbing.
pub fn round_trip(key: RawKey, via_library: bool) -> (u64, u64) {
    let args = [
        0xA2A2_0002,
        0xA3A3_0003,
        0xA4A4_0004,
        0xA5A5_0005,
        0xA6A6_0006,
        0xA7A7_0007,
    ];
    let source_daif = read_daif();
    assert_eq!(TTBR0_EL1.get(), translation::source_ttbr());
    RETURN_VIA_LIBRARY.store(via_library, Ordering::Release);
    let delivered = if via_library {
        // SAFETY: STACK_END lies in the Bounce probe page reserved for this
        // fixture's Invocation; nothing else uses it during the Call.
        unsafe { InvocationKey::from_key(key).call(args, STACK_END) }
            .unwrap_or_else(|error| panic!("library PPC Call failed: {:?}", error.code()))
    } else {
        let outcome = call_with_registers(key, 0, args, STACK_END);
        assert_eq!(outcome.check, 1, "Return corrupted x19-x30, SP or NZCV");
        assert_eq!(outcome.volatile_or, 0, "Return leaked x3-x18 state");
        decode_syscall_result((outcome.status, outcome.word1, outcome.word2))
            .unwrap_or_else(|error| panic!("PPC Call failed: {:?}", error.code()))
    };
    assert_eq!(TTBR0_EL1.get(), translation::source_ttbr());

    // What the target observed at entry, captured before any compiled code.
    let word = |index: usize| CAPTURE[index].load(Ordering::Acquire);
    assert_eq!((word(0), word(1)), (0, 0), "dummy x0/x1 not zeroed");
    for (index, expected) in args.into_iter().enumerate() {
        assert_eq!(word(2 + index), expected, "Call input x{} moved", 2 + index);
    }
    for index in 8..31 {
        assert_eq!(word(index), 0, "target entry x{index} not scrubbed");
    }
    assert_eq!(word(31), 0, "target entry NZCV not cleared");
    assert_eq!(word(32), source_daif, "target did not inherit DAIF");
    assert_eq!(
        ENTRY_SP.load(Ordering::Acquire),
        STACK_END,
        "x9 not the target SP"
    );

    // r0: Bounce's probe word through the shared low VA; r1: target TTBR0.
    assert_eq!(delivered.0, translation::bounce_probe_word());
    assert_eq!(delivered.1, translation::bounce_ttbr());
    // SAFETY: the source root maps PROBE_VA to the source probe Frame.
    let source_word = unsafe { (PROBE_VA as *const u64).read_volatile() };
    assert_ne!(source_word, delivered.0, "Call did not leave the source AS");
    delivered
}

/// The component's init, run with the keys its `AddressSpace` builder passes
/// in: record the Return key for the raw test target and the export adapter,
/// and the `DebugConsole` key for the export body.
pub fn component_init(return_key: RawKey, debug_console_key: RawKey) {
    RETURN_KEY.store(return_key.to_wire(), Ordering::Release);
    DEBUG_CONSOLE_KEY.store(debug_console_key.to_wire(), Ordering::Release);
    export::init_return_key(&ThreadReturnKey::from_key(return_key));
}

/// The compiled export's body: an ordinary `extern "C"` function returning
/// its two result words in x0/x1. r1 reports the translation it ran under.
extern "C" fn export_body(
    input0: u64,
    input1: u64,
    input2: u64,
    input3: u64,
    input4: u64,
    input5: u64,
) -> PpcResult {
    // A nested ordinary invocation from inside the migrated call: the key
    // resolves in Bounce's own table. The kernel reads the string through
    // the direct map at the pointer's numeric value (the source's image
    // copy); Bounce maps an identical image copy at the same VA.
    DebugConsoleKey::from_key(RawKey::from_wire(DEBUG_CONSOLE_KEY.load(Ordering::Acquire)))
        .write("DEBCON| Bounce: PPC export body writing through its own DebugConsole key\n")
        .unwrap_or_else(|error| panic!("Bounce DebugConsole write failed: {:?}", error.code()));
    CONSOLE_WRITES.fetch_add(1, Ordering::AcqRel);

    PpcResult {
        r0: export_digest([input0, input1, input2, input3, input4, input5]),
        r1: TTBR0_EL1.get(),
    }
}

fn export_digest(inputs: [u64; 6]) -> u64 {
    inputs.iter().enumerate().fold(0, |digest, (index, input)| {
        digest ^ input.rotate_left(u32::try_from(index * 8).unwrap_or(0))
    })
}

ppc_export!(
    /// Entry published by `CreateInvocation` for [`export_body`].
    pub fn export_entry => export_body
);

/// Handler arguments the last `vesper_thread_return_fault` received.
static FAULT_ARGS: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];
static FAULT_COUNT: AtomicU64 = AtomicU64::new(0);
/// Nonzero: the handler repairs by retrying Return with this key.
static REPAIR_KEY: AtomicU64 = AtomicU64::new(0);

/// The image-supplied libOS Return-fault handler (five-word extern-C ABI).
/// Records its arguments, then either repairs with an explicit Return
/// carrying the original words or halts the fixture.
#[unsafe(no_mangle)]
pub extern "C" fn vesper_thread_return_fault(
    status: u64,
    detail1: u64,
    detail2: u64,
    original_r0: u64,
    original_r1: u64,
) -> ! {
    for (slot, value) in FAULT_ARGS
        .iter()
        .zip([status, detail1, detail2, original_r0, original_r1])
    {
        slot.store(value, Ordering::Release);
    }
    FAULT_COUNT.fetch_add(1, Ordering::AcqRel);
    let repair = REPAIR_KEY.load(Ordering::Acquire);
    assert_ne!(
        repair, 0,
        "PPC export: unrepaired Return fault ({status}, {detail1:#x}, {detail2:#x})"
    );
    // SAFETY: still inside the failed export on this Thread; a successful
    // Return abandons this handler's stack.
    let Err(error) = (unsafe {
        ThreadReturnKey::from_key(RawKey::from_wire(repair))
            .return_from_invocation(original_r0, original_r1)
    });
    panic!("PPC export: repair Return failed: {:?}", error.code());
}

/// Call the compiled export. With `stale_init_key`, init records a stale
/// Return key so the adapter's Return is rejected and the handler repairs.
/// Either way the body writes once through Bounce's `DebugConsole` key.
pub fn export_round_trip(
    key: RawKey,
    return_key: RawKey,
    debug_console_key: RawKey,
    stale_init_key: bool,
) {
    let stale = RawKey::new(return_key.slot(), return_key.incarnation() + 1);
    if stale_init_key {
        component_init(stale, debug_console_key);
        REPAIR_KEY.store(return_key.to_wire(), Ordering::Release);
    } else {
        component_init(return_key, debug_console_key);
        REPAIR_KEY.store(0, Ordering::Release);
    }
    FAULT_COUNT.store(0, Ordering::Release);
    let writes_before = CONSOLE_WRITES.load(Ordering::Acquire);
    let args = [
        0x0101_0101,
        0xF0F0_0000_0000_0F0F,
        u64::MAX,
        0,
        0x1234_5678_9ABC_DEF0,
        7,
    ];
    // SAFETY: STACK_END lies in the Bounce probe page reserved for this
    // fixture's Invocations; nothing else uses it during the Call.
    let (r0, r1) = unsafe { InvocationKey::from_key(key).call(args, STACK_END) }
        .unwrap_or_else(|error| panic!("export Call failed: {:?}", error.code()));
    assert_eq!(r0, export_digest(args), "export body result lost");
    assert_eq!(
        r1,
        translation::bounce_ttbr(),
        "export body ran outside Bounce"
    );
    assert_eq!(TTBR0_EL1.get(), translation::source_ttbr());
    assert_eq!(
        CONSOLE_WRITES.load(Ordering::Acquire),
        writes_before + 1,
        "export body did not write through Bounce's DebugConsole key"
    );

    let faults = FAULT_COUNT.load(Ordering::Acquire);
    if stale_init_key {
        assert_eq!(
            faults, 1,
            "rejected adapter Return did not reach the handler"
        );
        let handed: [u64; 5] =
            core::array::from_fn(|index| FAULT_ARGS[index].load(Ordering::Acquire));
        let expected = CapError::InconsistentKey {
            key: stale,
            reason: InconsistencyReason::SlotIncarnationMismatch,
            operand: 0,
        }
        .code();
        assert_eq!(handed, [expected.0, expected.1, expected.2, r0, r1]);
    } else {
        assert_eq!(faults, 0, "successful export reached the fault handler");
    }
    component_init(return_key, debug_console_key);
    REPAIR_KEY.store(0, Ordering::Release);
}

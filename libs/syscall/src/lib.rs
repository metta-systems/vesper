#![no_std]
#![no_main]

// Syscall ABI:
// ┌───────────────────────────────────────────────────────────────────────┐
// │  CAPTBL COPY (cross-domain grant):                                    │
// │  ┌─────────────────────────────────────────────────────────────────┐  │
// │  │ x0 = src_captbl    x3 = dst_captbl                              │  │
// │  │ x1 = COPY op       x4 = dst_slot                                │  │
// │  │ x2 = src_slot      x5 = rights_mask  ← derive+copy in one!      │  │
// │  └─────────────────────────────────────────────────────────────────┘  │
// │                                                                       │
// │  BUFFER MAP (with full control):                                      │
// │  ┌─────────────────────────────────────────────────────────────────┐  │
// │  │ x0 = buffer_cap    x3 = size                                    │  │
// │  │ x1 = MAP op        x4 = offset      ← map partial buffer        │  │
// │  │ x2 = virt_addr     x5 = flags       ← cache policy, etc.        │  │
// │  └─────────────────────────────────────────────────────────────────┘  │
// │                                                                       │
// │  PPC Invocation.Call: x0 = cap, x1 = op 0, x2..x7 = six u64 args,   │
// │  x9 = target SP (provisional). Success: x0 = 0, x1 = r0, x2 = r1.    │
// │  UNTYPED RETYPE (batch creation):                                     │
// │  ┌─────────────────────────────────────────────────────────────────┐  │
// │  │ x0 = untyped_cap   x3 = dest_captbl                             │  │
// │  │ x1 = RETYPE op     x4 = dest_slot_start                         │  │
// │  │ x2 = obj_type      x5 = count       ← create N objects at once! │  │
// │  └─────────────────────────────────────────────────────────────────┘  │
// └───────────────────────────────────────────────────────────────────────┘

// Contract status: CopyDerive uses packed keys in x0/x2/x3, a vacant u32 slot
// in x4, provisional rights in x5, and reserved-zero x6/x7. Success returns
// the destination-local packed key in x1 with x2 zero. Invocation.Call uses
// x0 for the capability, x1 for operation 0, and forwards x2..x7; PPC return
// is the dedicated `ppc_call` transport below (target SP in x9, provisional
// and not frozen until field-tested). Return is
// Thread.Return op 0 on CurrentReturnOnly (packed target-table key in x0,
// r0/r1 in x2/x3) through `ppc_return`.

/// Single syscall ABI
///
/// Entry: SVC #0
///
/// Arguments:
///   x0 = capability slot
///   x1 = operation code
///   x2-x7 = operation arguments (6 args!)
///   x9-x15 are caller-saved, we don't use them
///
/// Contract status: x0 now carries the complete packed key (incarnation high,
/// slot low), not a slot-only selector. x1 remains full width for checked decoding.
///
/// Returns:
///   x0 = error code (0 = success)
///   x1 = return value 0
///   x2 = return value 1 (if needed)
///
/// # Safety
/// - Not safe.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub unsafe fn protected_call6(
    cap: u64,
    op: u64,
    a0: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
    a5: u64,
) -> (u64, u64, u64) {
    let r0: u64;
    let r1: u64;
    let r2: u64;
    // # SAFETY: As safe as possible, duh!
    unsafe {
        core::arch::asm!(
            "svc #0",
            inlateout("x0") cap => r0,
            inlateout("x1") op => r1,
            inlateout("x2") a0 => r2,
            in("x3") a1,
            in("x4") a2,
            in("x5") a3,
            in("x6") a4,
            in("x7") a5,
            options(nostack),
        );
    }
    (r0, r1, r2)
}

/// 5-arg invoke
///
/// # Safety
/// - Not safe.
#[inline(always)]
pub unsafe fn protected_call5(
    cap: u64,
    op: u64,
    a0: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
) -> (u64, u64, u64) {
    let r0: u64;
    let r1: u64;
    let r2: u64;
    // # SAFETY: As safe as possible, duh!
    unsafe {
        core::arch::asm!(
            "svc #0",
            inlateout("x0") cap => r0,
            inlateout("x1") op => r1,
            inlateout("x2") a0 => r2,
            in("x3") a1,
            in("x4") a2,
            in("x5") a3,
            in("x6") a4,
            // The kernel reads every argument register; unused words are
            // transmitted as zero (strict wire-argument convention,
            // selected 2026-09-15).
            in("x7") 0_u64,
            options(nostack),
        );
    }
    (r0, r1, r2)
}

/// 4-arg invoke
///
/// # Safety
/// - Not safe.
#[inline(always)]
pub unsafe fn protected_call4(
    cap: u64,
    op: u64,
    a0: u64,
    a1: u64,
    a2: u64,
    a3: u64,
) -> (u64, u64, u64) {
    let r0: u64;
    let r1: u64;
    let r2: u64;
    // # SAFETY: As safe as possible, duh!
    unsafe {
        core::arch::asm!(
            "svc #0",
            inlateout("x0") cap => r0,
            inlateout("x1") op => r1,
            inlateout("x2") a0 => r2,
            in("x3") a1,
            in("x4") a2,
            in("x5") a3,
            // CopyDerive reserves these argument registers as zero.
            in("x6") 0_u64,
            in("x7") 0_u64,
            options(nostack),
        );
    }
    (r0, r1, r2)
}

/// 3-arg invoke
///
/// # Safety
/// - Not safe.
#[inline(always)]
pub unsafe fn protected_call3(cap: u64, op: u64, a0: u64, a1: u64, a2: u64) -> (u64, u64, u64) {
    let r0: u64;
    let r1: u64;
    let r2: u64;
    // # SAFETY: As safe as possible, duh!
    unsafe {
        core::arch::asm!(
            "svc #0",
            inlateout("x0") cap => r0,
            inlateout("x1") op => r1,
            inlateout("x2") a0 => r2,
            in("x3") a1,
            in("x4") a2,
            // The kernel reads every argument register; unused words are
            // transmitted as zero (strict wire-argument convention,
            // selected 2026-09-15).
            in("x5") 0_u64,
            in("x6") 0_u64,
            in("x7") 0_u64,
            options(nostack),
        );
    }
    (r0, r1, r2)
}

/// 2-arg invoke
///
/// # Safety
/// - Not safe.
#[inline(always)]
pub unsafe fn protected_call2(cap: u64, op: u64, a0: u64, a1: u64) -> (u64, u64, u64) {
    let r0: u64;
    let r1: u64;
    let r2: u64;
    // # SAFETY: As safe as possible, duh!
    unsafe {
        core::arch::asm!(
            "svc #0",
            inlateout("x0") cap => r0,
            inlateout("x1") op => r1,
            inlateout("x2") a0 => r2,
            in("x3") a1,
            // The kernel reads every argument register; unused words are
            // transmitted as zero (strict wire-argument convention,
            // selected 2026-09-15).
            in("x4") 0_u64,
            in("x5") 0_u64,
            in("x6") 0_u64,
            in("x7") 0_u64,
            options(nostack),
        );
    }
    (r0, r1, r2)
}

/// 1-arg invoke
///
/// The unused argument registers (`x3..x7`) are transmitted as zero: the
/// kernel reads every argument register, so an unspecified value would
/// arrive as a defined-but-arbitrary argument (strict wire-argument
/// convention, selected 2026-09-15).
///
/// # Safety
/// - Not safe.
#[inline(always)]
pub unsafe fn protected_call1(cap: u64, op: u64, a0: u64) -> (u64, u64, u64) {
    let r0: u64;
    let r1: u64;
    let r2: u64;
    // # SAFETY: As safe as possible, duh!
    unsafe {
        core::arch::asm!(
            "svc #0",
            inlateout("x0") cap => r0,
            inlateout("x1") op => r1,
            inlateout("x2") a0 => r2,
            in("x3") 0_u64,
            in("x4") 0_u64,
            in("x5") 0_u64,
            in("x6") 0_u64,
            in("x7") 0_u64,
            options(nostack),
        );
    }
    (r0, r1, r2)
}

/// 0-arg invoke (cap + op only)
///
/// The six syscall argument registers (`x2..x7`) are all transmitted as
/// zero: the kernel reads every argument register, so an unspecified value
/// would arrive as a defined-but-arbitrary argument, not as "no argument".
///
/// # Safety
/// - Not safe.
#[inline(always)]
pub unsafe fn protected_call0(cap: u64, op: u64) -> (u64, u64, u64) {
    let r0: u64;
    let r1: u64;
    let r2: u64;
    // # SAFETY: As safe as possible, duh!
    unsafe {
        core::arch::asm!(
            "svc #0",
            inlateout("x0") cap => r0,
            inlateout("x1") op => r1,
            inlateout("x2") 0_u64 => r2,
            in("x3") 0_u64,
            in("x4") 0_u64,
            in("x5") 0_u64,
            in("x6") 0_u64,
            in("x7") 0_u64,
            options(nostack),
        );
    }
    (r0, r1, r2)
}

/// PPC `Invocation.Call` transport: `x0` Invocation key, `x1` operation 0,
/// `x2..x7` the six real inputs, `x9` the target stack pointer. The x9
/// transport is provisional: it works end-to-end but is not frozen ABI.
///
/// Returns the raw `(x0, x1, x2)` response: `SUCCESS` with the target's
/// `(r0, r1)`, or a pre-commit rejection's status and details.
///
/// Register effects follow the selected conservative declarations: x0..x2
/// carry inputs and responses; x3..x7 and x9 are inputs with discarded
/// outputs; x8, x10..x17 and the caller-volatile x18 are discarded clobbers.
/// The kernel restores x19..x30, SP and NZCV, but memory and flags are left
/// to the compiler's conservative defaults (no `nomem`, `readonly`, `pure`,
/// `preserves_flags`), and `nostack` is deliberately omitted: the migrated
/// target runs on its own stack, but the source frame is not asserted
/// untouched for this initial wrapper.
///
/// # Safety
/// The caller supplies a valid Invocation key and a target SP satisfying the
/// Invocation's published stack contract; the target executes arbitrary
/// interface code on the caller's Thread before returning here.
#[inline(always)]
pub unsafe fn ppc_call(key: u64, args: [u64; 6], target_sp: u64) -> (u64, u64, u64) {
    let status: u64;
    let word1: u64;
    let word2: u64;
    // SAFETY: the caller upholds the Invocation contract above; every
    // register the kernel may rewrite on Call/Return is declared.
    unsafe {
        core::arch::asm!(
            "svc #0",
            inlateout("x0") key => status,
            inlateout("x1") 0_u64 => word1,
            inlateout("x2") args[0] => word2,
            inlateout("x3") args[1] => _,
            inlateout("x4") args[2] => _,
            inlateout("x5") args[3] => _,
            inlateout("x6") args[4] => _,
            inlateout("x7") args[5] => _,
            inlateout("x9") target_sp => _,
            lateout("x8") _,
            lateout("x10") _,
            lateout("x11") _,
            lateout("x12") _,
            lateout("x13") _,
            lateout("x14") _,
            lateout("x15") _,
            lateout("x16") _,
            lateout("x17") _,
            // x18 is caller-volatile scratch where the target allows it; on
            // targets that reserve it (Apple) the compiler never allocates it.
            #[cfg(not(target_vendor = "apple"))]
            lateout("x18") _,
        );
    }
    (status, word1, word2)
}

/// `Thread.Return` transport: `x0` the target-table-local packed Slot(1)
/// key, `x1` operation 0, `x2`/`x3` the result words `(r0, r1)`.
///
/// A successful Return never comes back here: the kernel resumes the source.
/// A local response is either an ordinary rejection or, if its status is
/// `SUCCESS`, a protocol anomaly the caller must not treat as success.
/// x4..x7 are ignored by the kernel and not initialized here; x4..x17 and
/// x18 are discarded clobbers. Not `noreturn`: rejection returns locally.
///
/// # Safety
/// Must run on a Thread whose current invocation it intends to complete;
/// a successful Return abandons this execution context entirely.
#[inline(always)]
pub unsafe fn ppc_return(key: u64, r0: u64, r1: u64) -> (u64, u64, u64) {
    let status: u64;
    let word1: u64;
    let word2: u64;
    // SAFETY: the caller upholds the contract above; every register the
    // kernel may rewrite on a local response is declared.
    unsafe {
        core::arch::asm!(
            "svc #0",
            inlateout("x0") key => status,
            inlateout("x1") 0_u64 => word1,
            inlateout("x2") r0 => word2,
            inlateout("x3") r1 => _,
            lateout("x4") _,
            lateout("x5") _,
            lateout("x6") _,
            lateout("x7") _,
            lateout("x8") _,
            lateout("x9") _,
            lateout("x10") _,
            lateout("x11") _,
            lateout("x12") _,
            lateout("x13") _,
            lateout("x14") _,
            lateout("x15") _,
            lateout("x16") _,
            lateout("x17") _,
            // x18 is caller-volatile scratch where the target allows it; on
            // targets that reserve it (Apple) the compiler never allocates it.
            #[cfg(not(target_vendor = "apple"))]
            lateout("x18") _,
        );
    }
    (status, word1, word2)
}

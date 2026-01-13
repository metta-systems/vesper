// Syscall ABI:
// ┌─────────────────────────────────────────────────────────────────────────┐
// │  CAPTBL COPY (cross-domain grant):                                      │
// │  ┌─────────────────────────────────────────────────────────────────┐   │
// │  │ x0 = src_captbl    x3 = dst_captbl                              │   │
// │  │ x1 = COPY op       x4 = dst_slot                                │   │
// │  │ x2 = src_slot      x5 = rights_mask  ← derive+copy in one!      │   │
// │  └─────────────────────────────────────────────────────────────────┘   │
// │                                                                         │
// │  BUFFER MAP (with full control):                                        │
// │  ┌─────────────────────────────────────────────────────────────────┐   │
// │  │ x0 = buffer_cap    x3 = size                                    │   │
// │  │ x1 = MAP op        x4 = offset      ← map partial buffer        │   │
// │  │ x2 = virt_addr     x5 = flags       ← cache policy, etc.        │   │
// │  └─────────────────────────────────────────────────────────────────┘   │
// │                                                                         │
// │  ENDPOINT CALL (with inline payload):                                   │
// │  ┌─────────────────────────────────────────────────────────────────┐   │
// │  │ x0 = endpoint_cap  x3 = msg_word_1                              │   │
// │  │ x1 = CALL op       x4 = msg_word_2                              │   │
// │  │ x2 = msg_word_0    x5 = msg_word_3  ← 4 words inline!           │   │
// │  └─────────────────────────────────────────────────────────────────┘   │
// │                                                                         │
// │  UNTYPED RETYPE (batch creation):                                       │
// │  ┌─────────────────────────────────────────────────────────────────┐   │
// │  │ x0 = untyped_cap   x3 = dest_captbl                             │   │
// │  │ x1 = RETYPE op     x4 = dest_slot_start                         │   │
// │  │ x2 = obj_type      x5 = count       ← create N objects at once! │   │
// │  └─────────────────────────────────────────────────────────────────┘   │
// └─────────────────────────────────────────────────────────────────────────┘

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
/// Returns:
///   x0 = error code (0 = success)
///   x1 = return value 0
///   x2 = return value 1 (if needed)
#[inline(always)]
pub unsafe fn protected_call6(
    cap: u32,
    op: u32,
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

    core::arch::asm!(
        "svc #0",
        inlateout("x0") cap as u64 => r0,
        inlateout("x1") op as u64 => r1,
        inlateout("x2") a0 => r2,
        in("x3") a1,
        in("x4") a2,
        in("x5") a3,
        in("x6") a4,
        in("x7") a5,
        options(nostack),
    );

    (r0, r1, r2)
}

// Most operations don't need all 6 args - provide convenience wrappers
// TODO: don't waste registers to zero them out for a less-than-6-args versions? Might be risky...

/// 0-arg invoke (cap + op only)
#[inline(always)]
pub fn protected_call0(cap: u32, op: u32) -> Result<u64> {
    let (err, val, _) = unsafe { protected_call6(cap, op, 0, 0, 0, 0, 0, 0) };
    if err == 0 { Ok(val) } else { Err(Error(err)) }
}

/// 1-arg invoke
#[inline(always)]
pub fn protected_call1(cap: u32, op: u32, a0: u64) -> Result<u64> {
    let (err, val, _) = unsafe { protected_call6(cap, op, a0, 0, 0, 0, 0, 0) };
    if err == 0 { Ok(val) } else { Err(Error(err)) }
}

/// 2-arg invoke
#[inline(always)]
pub fn protected_call2(cap: u32, op: u32, a0: u64, a1: u64) -> Result<u64> {
    let (err, val, _) = unsafe { protected_call6(cap, op, a0, a1, 0, 0, 0, 0) };
    if err == 0 { Ok(val) } else { Err(Error(err)) }
}

/// 3-arg invoke
#[inline(always)]
pub fn protected_call3(cap: u32, op: u32, a0: u64, a1: u64, a2: u64) -> Result<u64> {
    let (err, val, _) = unsafe { protected_call6(cap, op, a0, a1, a2, 0, 0, 0) };
    if err == 0 { Ok(val) } else { Err(Error(err)) }
}

/// 4-arg invoke
#[inline(always)]
pub fn protected_call4(cap: u32, op: u32, a0: u64, a1: u64, a2: u64, a3: u64) -> Result<u64> {
    let (err, val, _) = unsafe { protected_call6(cap, op, a0, a1, a2, a3, 0, 0) };
    if err == 0 { Ok(val) } else { Err(Error(err)) }
}

/// 5-arg invoke
#[inline(always)]
pub fn protected_call5(
    cap: u32,
    op: u32,
    a0: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
) -> Result<u64> {
    let (err, val, _) = unsafe { protected_call6(cap, op, a0, a1, a2, a3, a4, 0) };
    if err == 0 { Ok(val) } else { Err(Error(err)) }
}

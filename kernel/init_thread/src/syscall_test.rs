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
    unsafe {
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
    }
    (r0, r1, r2)
}

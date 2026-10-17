//! Register-preservation probe for ordinary capability invocations.
//!
//! The contract's ordinary control invocation baseline says an invocation
//! writes only `x0..x2`: every other general-purpose register, SP and the
//! NZCV flags are as the caller left them, whether the call returns at once
//! or blocks and is resumed later. [`invoke`] loads distinct markers into all
//! of them, issues one `svc`, and captures the complete register file
//! immediately afterwards for [`Observation::assert_preserved`] to check.

use {core::arch::asm, libobject::RawKey};

/// NZCV value set before the `svc`: N and C set, Z and V clear.
pub const NZCV_PATTERN: u64 = 0xA000_0000;

/// Sentinel loaded into `x<index>` before the `svc`: the register number's
/// decimal digits, repeated as hex (`x8` = `0x0808`, `x20` = `0x2020`).
/// `x0..x7` carry the invocation, `x19` the key and `x29` the SP instead.
pub const fn marker(index: usize) -> u64 {
    let digits = (((index / 10) << 4) | (index % 10)) as u64;
    digits * 0x101
}

/// The register file observed immediately after an ordinary invocation,
/// in the order the probe stores it.
#[repr(C)]
#[derive(Default)]
pub struct Observation {
    /// `x0..x30`.
    gpr: [u64; 31],
    nzcv: u64,
    sp: u64,
}

const _: () = assert!(size_of::<Observation>() == 33 * 8);

impl Observation {
    /// The status and two result words (`x0..x2`).
    pub fn result(&self) -> (u64, u64, u64) {
        (self.gpr[0], self.gpr[1], self.gpr[2])
    }

    /// Assert that everything outside `x0..x2` survived the invocation:
    /// `x3..x7` still hold the submitted arguments, the markers in
    /// `x8..x18`, `x20..x28` and `x30`, the key copy in `x19`, the SP copy in
    /// `x29` equal to the live SP, and [`NZCV_PATTERN`].
    pub fn assert_preserved(&self, key: RawKey, args: [u64; 6]) {
        for (index, &expected) in args.iter().enumerate().skip(1) {
            assert_eq!(self.gpr[2 + index], expected, "x{} clobbered", 2 + index);
        }
        for index in (8..=18).chain(20..=28).chain([30]) {
            assert_eq!(self.gpr[index], marker(index), "x{index} clobbered");
        }
        assert_eq!(self.gpr[19], key.to_wire(), "x19 clobbered");
        assert_eq!(
            self.gpr[29], self.sp,
            "SP or x29 changed across the invocation"
        );
        assert_eq!(
            self.nzcv, NZCV_PATTERN,
            "NZCV changed across the invocation"
        );
    }
}

/// Issue one ordinary invocation (`x0` key, `x1` op, `x2..x7` args) with
/// markers in every other register, and capture the register file after it.
///
/// Test-only transport. The compiler's `x19..x30` are spilled to the stack
/// and restored before Rust resumes; `x0..x18` and the flags are declared
/// clobbered.
#[inline(never)]
pub fn invoke(key: RawKey, op: u64, args: [u64; 6]) -> Observation {
    let mut observation = Observation::default();
    // SAFETY: the 384-byte frame is created and removed inside this block:
    // `[sp, #0..96)` spills x19..x30, `[sp, #96]` holds the capture pointer,
    // and `[sp, #112..376)` receives the post-`svc` registers before they are
    // copied out. The compiler's callee-saved registers and LR are restored
    // before Rust resumes; every other register is declared clobbered.
    unsafe {
        asm!(
            "sub sp, sp, #384",
            "stp x19, x20, [sp, #0]", "stp x21, x22, [sp, #16]",
            "stp x23, x24, [sp, #32]", "stp x25, x26, [sp, #48]",
            "stp x27, x28, [sp, #64]", "stp x29, x30, [sp, #80]",
            "str x8, [sp, #96]",
            "movz x9, #0xa000, lsl #16", "msr nzcv, x9",
            "mov x8, #0x0808", "mov x9, #0x0909", "mov x10, #0x1010",
            "mov x11, #0x1111", "mov x12, #0x1212", "mov x13, #0x1313",
            "mov x14, #0x1414", "mov x15, #0x1515", "mov x16, #0x1616",
            "mov x17, #0x1717", "mov x18, #0x1818",
            "mov x19, x0", "mov x20, #0x2020", "mov x21, #0x2121",
            "mov x22, #0x2222", "mov x23, #0x2323", "mov x24, #0x2424",
            "mov x25, #0x2525", "mov x26, #0x2626", "mov x27, #0x2727",
            "mov x28, #0x2828", "mov x29, sp", "mov x30, #0x3030",
            "svc #0",
            "stp x0, x1, [sp, #112]", "stp x2, x3, [sp, #128]",
            "stp x4, x5, [sp, #144]", "stp x6, x7, [sp, #160]",
            "stp x8, x9, [sp, #176]", "stp x10, x11, [sp, #192]",
            "stp x12, x13, [sp, #208]", "stp x14, x15, [sp, #224]",
            "stp x16, x17, [sp, #240]", "stp x18, x19, [sp, #256]",
            "stp x20, x21, [sp, #272]", "stp x22, x23, [sp, #288]",
            "stp x24, x25, [sp, #304]", "stp x26, x27, [sp, #320]",
            "stp x28, x29, [sp, #336]", "str x30, [sp, #352]",
            "mrs x9, nzcv", "str x9, [sp, #360]",
            "mov x9, sp", "str x9, [sp, #368]",
            "ldr x9, [sp, #96]", "add x10, sp, #112", "mov x11, #33",
            "1:",
            "ldr x12, [x10], #8", "str x12, [x9], #8",
            "subs x11, x11, #1", "b.ne 1b",
            "ldp x19, x20, [sp, #0]", "ldp x21, x22, [sp, #16]",
            "ldp x23, x24, [sp, #32]", "ldp x25, x26, [sp, #48]",
            "ldp x27, x28, [sp, #64]", "ldp x29, x30, [sp, #80]",
            "add sp, sp, #384",
            inlateout("x0") key.to_wire() => _,
            inlateout("x1") op => _,
            inlateout("x2") args[0] => _,
            inlateout("x3") args[1] => _,
            inlateout("x4") args[2] => _,
            inlateout("x5") args[3] => _,
            inlateout("x6") args[4] => _,
            inlateout("x7") args[5] => _,
            inlateout("x8") core::ptr::from_mut(&mut observation) => _,
            lateout("x9") _, lateout("x10") _, lateout("x11") _,
            lateout("x12") _, lateout("x13") _, lateout("x14") _,
            lateout("x15") _, lateout("x16") _, lateout("x17") _,
            lateout("x18") _,
        );
    }
    observation
}

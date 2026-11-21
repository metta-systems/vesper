//! Fault delivery: what a fault handler receives and how it resumes.
//!
//! A fault (a synchronous non-SVC exception from EL0, or a `Thread.Return`
//! protocol fault) is delivered as a synchronous upcall on the faulting Thread:
//! the kernel performs a forced Call into the `Invocation` held at
//! [`KeySlot::FAULT_HANDLER`](crate::KeySlot::FAULT_HANDLER) in the faulting
//! `AddressSpace`'s table. The handler's six inputs are
//! `(kind, esr, far, pc, sp, depth)` — see [`FaultKind`] — and `x0` holds the
//! address of a [`FaultFrame`] in its stack pool's fault area (see
//! [`stack_pool`](crate::stack_pool)). It finishes with an
//! ordinary `Thread.Return` whose first result word is a [`FaultAction`];
//! Retry and Skip resume from the frame, which the handler may have changed.

/// What faulted: handler input `x2`.
#[repr(u64)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultKind {
    /// A synchronous CPU exception; `esr`/`far` hold `ESR_EL1`/`FAR_EL1`.
    CpuException = 0,
    /// `Thread.Return` with no continuation to return to; `esr`/`far` are 0.
    IllegalReturn = 1,
    /// `Thread.Return` whose saved caller `AddressSpace` is retired;
    /// `esr`/`far` are 0.
    ReturnTargetRetired = 2,
}

impl FaultKind {
    /// Decode handler input `x2`; `None` for a value this ABI does not know.
    pub const fn from_wire(value: u64) -> Option<Self> {
        match value {
            0 => Some(Self::CpuException),
            1 => Some(Self::IllegalReturn),
            2 => Some(Self::ReturnTargetRetired),
            _ => None,
        }
    }
}

/// How the faulting Thread resumes: the handler's first `Thread.Return`
/// result word. Any other value terminates.
#[repr(u64)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultAction {
    /// Re-execute the faulting instruction with the exact faulting state.
    Retry = 0,
    /// Resume at the instruction after the faulting one.
    Skip = 1,
    /// Park the Thread as faulted; it never runs again.
    Terminate = 2,
}

impl FaultAction {
    /// Decode a handler's result word; unknown values terminate.
    pub const fn from_wire(value: u64) -> Self {
        match value {
            0 => Self::Retry,
            1 => Self::Skip,
            _ => Self::Terminate,
        }
    }
}

/// The handler's six inputs, as delivered in `x2..x7`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultInfo {
    pub kind: FaultKind,
    pub esr: u64,
    pub far: u64,
    /// The faulting instruction (for a Return fault, its `svc`).
    pub pc: u64,
    pub sp: u64,
    /// Invocation depth when the fault occurred.
    pub depth: u64,
}

impl FaultInfo {
    /// The six handler inputs in delivery order.
    pub const fn to_arguments(&self) -> [u64; 6] {
        [
            self.kind as u64,
            self.esr,
            self.far,
            self.pc,
            self.sp,
            self.depth,
        ]
    }
}

/// [`FaultFrame::version`] of this layout.
pub const FAULT_FRAME_VERSION: u32 = 1;

/// [`FaultFrame::flags`] bit: the frame holds the faulting frame's FP/SIMD
/// state (it ran in an `AddressSpace` that allows FP).
pub const FAULT_FRAME_FP: u32 = 1 << 0;

/// The faulting state, as the kernel writes it into the fault-area slot of
/// the stack the fault handler runs on. The handler gets its address in `x0`
/// and starts with `SP` at the top of its stack.
///
/// The handler may change it: a Retry or Skip resumes from the frame as it
/// is at the handler's `Thread.Return`. The kernel accepts a changed
/// `pstate` only if it differs from the faulting one in the condition flags
/// (NZCV); anything else, or a frame it cannot read back, terminates the
/// Thread. The FP part is used only when [`FAULT_FRAME_FP`] was set by the
/// kernel.
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultFrame {
    /// [`FAULT_FRAME_BYTES`].
    pub size: u32,
    /// [`FAULT_FRAME_VERSION`].
    pub version: u32,
    /// `FAULT_FRAME_*` bits.
    pub flags: u32,
    pub reserved: u32,
    /// `x0..x30` of the faulting frame.
    pub gpr: [u64; 31],
    pub sp: u64,
    /// The faulting instruction.
    pub pc: u64,
    /// The faulting `SPSR_EL1`.
    pub pstate: u64,
    pub tpidr_el0: u64,
    pub reserved_word: u64,
    /// `V0..V31`, zero without [`FAULT_FRAME_FP`].
    pub vectors: [u128; 32],
    pub fpcr: u32,
    pub fpsr: u32,
    pub reserved_tail: u64,
}

/// Bytes of a [`FaultFrame`]; it fits one fault-area slot.
pub const FAULT_FRAME_BYTES: usize = 832;

const _: () = {
    use core::mem::{align_of, offset_of, size_of};
    assert!(size_of::<FaultFrame>() == FAULT_FRAME_BYTES);
    assert!(align_of::<FaultFrame>() == 16);
    assert!(offset_of!(FaultFrame, gpr) == 16);
    assert!(offset_of!(FaultFrame, sp) == 264);
    assert!(offset_of!(FaultFrame, pc) == 272);
    assert!(offset_of!(FaultFrame, pstate) == 280);
    assert!(offset_of!(FaultFrame, tpidr_el0) == 288);
    assert!(offset_of!(FaultFrame, vectors) == 304);
    assert!(offset_of!(FaultFrame, fpcr) == 816);
    assert!(offset_of!(FaultFrame, fpsr) == 820);
};

impl FaultFrame {
    /// A frame with every field zero but the header.
    pub const EMPTY: Self = Self {
        #[allow(clippy::cast_possible_truncation, reason = "832 fits")]
        size: FAULT_FRAME_BYTES as u32,
        version: FAULT_FRAME_VERSION,
        flags: 0,
        reserved: 0,
        gpr: [0; 31],
        sp: 0,
        pc: 0,
        pstate: 0,
        tpidr_el0: 0,
        reserved_word: 0,
        vectors: [0; 32],
        fpcr: 0,
        fpsr: 0,
        reserved_tail: 0,
    };

    /// The frame's bytes, as they are written to the fault slot.
    pub const fn to_bytes(&self) -> [u8; FAULT_FRAME_BYTES] {
        // SAFETY: the frame is `repr(C)` with integer fields only and no
        // padding (the size asserts above), so every byte is initialized.
        unsafe { core::mem::transmute::<Self, [u8; FAULT_FRAME_BYTES]>(*self) }
    }

    /// The frame `bytes` hold.
    pub const fn from_bytes(bytes: [u8; FAULT_FRAME_BYTES]) -> Self {
        // SAFETY: every bit pattern is a valid frame: integer fields only.
        unsafe { core::mem::transmute::<[u8; FAULT_FRAME_BYTES], Self>(bytes) }
    }
}

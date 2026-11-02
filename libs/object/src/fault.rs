//! Fault delivery: what a fault handler receives and how it resumes.
//!
//! A fault (a synchronous non-SVC exception from EL0, or a `Thread.Return`
//! protocol fault) is delivered as a synchronous upcall on the faulting Thread:
//! the kernel performs a forced Call into the `Invocation` held at
//! [`KeySlot::FAULT_HANDLER`](crate::KeySlot::FAULT_HANDLER) in the faulting
//! `AddressSpace`'s table. The handler's six inputs are
//! `(kind, esr, far, pc, sp, depth)` — see [`FaultKind`] — and it finishes
//! with an ordinary `Thread.Return` whose first result word is a
//! [`FaultAction`].

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

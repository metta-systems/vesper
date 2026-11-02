//! The line a client's fault handler logs when one of its Threads faults:
//! `FAULT: <component> thread <n>: <cause> at pc 0x…, far 0x…, sp 0x…, esr 0x…`.
//!
//! The handler receives what the kernel delivers (`libobject::fault`): the
//! fault kind, `ESR_EL1`, `FAR_EL1`, the faulting PC and SP. With the
//! component's Thread stacks (from its init record) the report names the
//! Thread and tells a stack overflow, a translation fault on a guard page of
//! the stack region, from any other data abort.

use core::fmt;

/// Bytes of a page.
const PAGE: u64 = 4096;

/// Fault kind 0: a CPU exception (`libobject::fault::FaultKind`).
const KIND_CPU_EXCEPTION: u64 = 0;
/// Fault kind 1: `Thread.Return` with nothing to return to.
const KIND_ILLEGAL_RETURN: u64 = 1;
/// Fault kind 2: `Thread.Return` to a retired `AddressSpace`.
const KIND_RETURN_TARGET_RETIRED: u64 = 2;

/// What faulted, as the report names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// A translation fault on an unmapped page of the stack region: a stack
    /// ran into its guard page.
    StackOverflow,
    DataAbort,
    InstructionAbort,
    PcAlignment,
    SpAlignment,
    UndefinedInstruction,
    /// `brk`
    Breakpoint,
    /// FP/SIMD use, which traps
    FpSimdTrap,
    /// An `msr`/`mrs` of a register EL0 may not touch
    SystemRegisterTrap,
    IllegalReturn,
    ReturnTargetRetired,
    /// Another exception class (`ESR_EL1.EC`)
    Exception(u8),
    /// A fault kind this report does not know
    UnknownKind(u64),
}

impl fmt::Display for Cause {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StackOverflow => formatter.write_str("stack overflow (guard page)"),
            Self::DataAbort => formatter.write_str("data abort"),
            Self::InstructionAbort => formatter.write_str("instruction abort"),
            Self::PcAlignment => formatter.write_str("PC alignment fault"),
            Self::SpAlignment => formatter.write_str("SP alignment fault"),
            Self::UndefinedInstruction => formatter.write_str("undefined instruction"),
            Self::Breakpoint => formatter.write_str("breakpoint"),
            Self::FpSimdTrap => formatter.write_str("FP/SIMD trap"),
            Self::SystemRegisterTrap => formatter.write_str("system register trap"),
            Self::IllegalReturn => formatter.write_str("Return with no call to return from"),
            Self::ReturnTargetRetired => formatter.write_str("Return to a retired AddressSpace"),
            Self::Exception(class) => write!(formatter, "exception class {class:#x}"),
            Self::UnknownKind(kind) => write!(formatter, "fault kind {kind}"),
        }
    }
}

/// A fault as delivered, with what the component knows of its stacks.
#[derive(Clone, Copy, Debug)]
pub struct FaultReport<'a> {
    /// The component's manifest name
    pub component: &'a str,
    /// The fault kind (`libobject::fault::FaultKind` on the wire)
    pub kind: u64,
    /// `ESR_EL1` (0 for a Return fault)
    pub syndrome: u64,
    /// `FAR_EL1` (0 for a Return fault)
    pub fault_address: u64,
    pub pc: u64,
    pub sp: u64,
    /// `[bottom, top)` of each Thread's stack, by Thread number; each has an
    /// unmapped guard page right below `bottom`
    pub thread_stacks: &'a [(u64, u64)],
    /// `[start, end)` of the span holding every stack of the component; its
    /// pages that are not stacks are unmapped
    pub stack_region: (u64, u64),
}

impl FaultReport<'_> {
    /// What faulted.
    pub fn cause(&self) -> Cause {
        match self.kind {
            KIND_CPU_EXCEPTION => {}
            KIND_ILLEGAL_RETURN => return Cause::IllegalReturn,
            KIND_RETURN_TARGET_RETIRED => return Cause::ReturnTargetRetired,
            other => return Cause::UnknownKind(other),
        }
        // ESR_EL1.EC, bits 31:26; for an abort, the fault status code in
        // bits 5:0, a translation fault being 0b0001_xx (levels 0 to 3).
        let class = ((self.syndrome >> 26) & 0x3f) as u8;
        let status = self.syndrome & 0x3f;
        match class {
            0x24 if (0x04..=0x07).contains(&status) && self.in_stack_region() => {
                Cause::StackOverflow
            }
            0x24 => Cause::DataAbort,
            0x20 => Cause::InstructionAbort,
            0x22 => Cause::PcAlignment,
            0x26 => Cause::SpAlignment,
            0x00 => Cause::UndefinedInstruction,
            0x3c => Cause::Breakpoint,
            0x07 => Cause::FpSimdTrap,
            0x18 => Cause::SystemRegisterTrap,
            other => Cause::Exception(other),
        }
    }

    /// The faulting Thread's number: the one whose guard page the fault hit,
    /// else the one whose stack (or guard page) holds SP. `None` when SP is
    /// on none of them: the Thread was in a call into one of the component's
    /// exports, on a stack of its export pool.
    pub fn thread(&self) -> Option<usize> {
        let guarded = |address: u64| {
            self.thread_stacks
                .iter()
                .position(|&(bottom, top)| (bottom.saturating_sub(PAGE)..top).contains(&address))
        };
        if self.cause() == Cause::StackOverflow
            && let Some(thread) = guarded(self.fault_address)
        {
            return Some(thread);
        }
        guarded(self.sp)
    }

    fn in_stack_region(&self) -> bool {
        (self.stack_region.0..self.stack_region.1).contains(&self.fault_address)
    }
}

impl fmt::Display for FaultReport<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "FAULT: {} ", self.component)?;
        match self.thread() {
            Some(thread) => write!(formatter, "thread {thread}")?,
            None => formatter.write_str("in an export call")?,
        }
        write!(
            formatter,
            ": {} at pc {:#x}, far {:#x}, sp {:#x}, esr {:#x}",
            self.cause(),
            self.pc,
            self.fault_address,
            self.sp,
            self.syndrome
        )
    }
}

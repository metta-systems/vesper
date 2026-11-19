//! Fault delivery: a synchronous upcall on the faulting Thread.
//!
//! A fault is delivered by performing a forced `Invocation.Call` on the
//! faulting Thread itself into the `Invocation` held at
//! `KeySlot::FAULT_HANDLER` in the table of the `AddressSpace` it is executing
//! in, so the handler runs with the faulting Thread's priority and budget.
//! Before the Call commits, the faulting state is written as a `FaultFrame`
//! at the top of the handler's stack, whose address the handler gets in `x0`.
//! The handler finishes with an ordinary `Thread.Return`; when that Return
//! pops the fault continuation, `commit_return` reads the frame back,
//! validates it and resumes from it according to the handler's `FaultAction`
//! instead of the continuation.
//!
//! A fault nobody takes — no handler, a busy handler, a fault inside the
//! handler, a forced Call the ordinary Call rules reject, or a handler stack
//! the frame does not fit in or cannot be written to — parks the Thread as
//! `Faulted` and counts the fault on its `AddressSpace`.

use {
    crate::objects::{
        ArchObjects, ExecutionContext, Nucleus, ThreadFault,
        access::Access,
        arch_objects::{AddressSpaceId, AddressSpaceObject},
        fp::FpUnit,
        invocation::{CallTarget, CommittedCall, PreparedCall},
        key_table::KeyTable,
        resume::Switch,
        thread::ThreadId,
    },
    libexception::arch::aarch64::{FpState, SavedContext},
    libobject::{
        CapError,
        fault::{FAULT_FRAME_BYTES, FAULT_FRAME_FP, FAULT_FRAME_VERSION, FaultFrame, FaultInfo},
    },
};

/// `SPSR_EL1` condition flags: the only part of the faulting state's
/// `pstate` a fault handler may change.
const NZCV: u64 = 0xF000_0000;

/// Copy the general registers of `saved` into `frame`.
const fn fill_registers(frame: &mut FaultFrame, saved: &SavedContext) {
    let mut index = 0;
    while index < 30 {
        frame.gpr[index] = saved.gpr[index];
        index += 1;
    }
    frame.gpr[30] = saved.lr;
    frame.sp = saved.sp;
    frame.pc = saved.elr_el1;
    frame.pstate = saved.spsr_el1;
    frame.tpidr_el0 = saved.tpidr_el0;
}

/// The state to resume from `frame`, if the kernel accepts it: `faulting`
/// with the frame's registers, as long as the frame's `pstate` differs from
/// the faulting one only in the condition flags.
fn resume_state(frame: &FaultFrame, faulting: &SavedContext) -> Option<SavedContext> {
    if frame.size as usize != FAULT_FRAME_BYTES || frame.version != FAULT_FRAME_VERSION {
        return None;
    }
    if (frame.pstate ^ faulting.spsr_el1) & !NZCV != 0 {
        return None;
    }
    let mut resumed = *faulting;
    resumed.gpr.copy_from_slice(&frame.gpr[..30]);
    resumed.lr = frame.gpr[30];
    resumed.sp = frame.sp;
    resumed.elr_el1 = frame.pc;
    resumed.spsr_el1 = frame.pstate;
    resumed.tpidr_el0 = frame.tpidr_el0;
    Some(resumed)
}

/// The outcome of delivering a fault on the current Thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultDelivery {
    /// The handler was entered: entry installs the translation and restores
    /// `target`, exactly like a committed Call.
    Handler(CommittedCall),
    /// Nobody took the fault: the Thread is parked as `Faulted`; entry resumes
    /// the selected next Thread.
    Unhandled(Switch),
}

impl<A: ArchObjects> Nucleus<A> {
    /// Deliver a fault described by `info` on the current Thread, whose
    /// complete faulting state is `faulting` (for a Return fault, with PC at
    /// its `svc`).
    ///
    /// An error means even the unhandled path failed (no runnable Thread to
    /// switch to, or corrupted scheduling state); nothing is left to run.
    pub fn deliver_fault(
        &mut self,
        access: &Access,
        faulting: SavedContext,
        info: FaultInfo,
    ) -> Result<FaultDelivery, CapError> {
        let current = self.current_thread_id()?;
        let thread = self.thread(current).ok_or(CapError::InvalidThread)?;
        if thread.context != ExecutionContext::Running {
            return Err(CapError::InvalidOperation);
        }
        let faulting_address_space = thread.address_space;
        let depth = thread.invocation_stack.len();

        // One fault level per Thread: a fault inside its handler is unhandled.
        let handler = if thread.fault.is_some() {
            None
        } else {
            Self::available_fault_handler(access, faulting_address_space)
        };
        if let Some(target) = handler {
            let mut call_frame = faulting;
            call_frame.gpr[2..8].copy_from_slice(&info.to_arguments());
            if let Ok(prepared) = self.prepare_call(access, &call_frame, target, None) {
                // The forced call zeroes the registers for an FP handler, so
                // the faulting FP state is taken first.
                let fp = self.address_space_fp(faulting_address_space);
                let mut frame = FaultFrame::EMPTY;
                fill_registers(&mut frame, &faulting);
                if fp {
                    let mut state = FpState::ZERO;
                    A::Fp::save(&mut state);
                    frame.flags |= FAULT_FRAME_FP;
                    frame.vectors = state.vectors;
                    frame.fpcr = state.fpcr;
                    frame.fpsr = state.fpsr;
                }
                // The frame goes at the top of the stack the call will take,
                // before anything commits: a stack it does not fit in, or
                // cannot be written, leaves the fault unhandled.
                if let Some(frame_address) = self.write_fault_frame(&prepared, &frame) {
                    let mut committed = self.commit_call(access, prepared)?;
                    committed.target.sp = frame_address;
                    committed.target.gpr[0] = frame_address;
                    let thread = self.thread_mut(current).ok_or(CapError::InvalidThread)?;
                    thread.fault = Some(ThreadFault {
                        frame: faulting,
                        address_space: faulting_address_space,
                        depth: depth + 1,
                        frame_address,
                    });
                    if fp {
                        thread.fp_stats.fp_saves = thread.fp_stats.fp_saves.wrapping_add(1);
                    }
                    self.set_fault_handler_busy(faulting_address_space, true);
                    return Ok(FaultDelivery::Handler(committed));
                }
            }
        }

        if let Some(space) = self.address_space_mut(faulting_address_space) {
            space.count_unhandled_fault();
        }
        self.park_faulted(access, current, faulting)
            .map(FaultDelivery::Unhandled)
    }

    /// Write `frame` at the top of the stack the forced call `prepared` will
    /// take from the handler's pool; its address, or `None` when the stack is
    /// too small for it or the handler cannot write there. Nothing commits.
    fn write_fault_frame(&self, prepared: &PreparedCall, frame: &FaultFrame) -> Option<u64> {
        let translation = prepared.translation();
        let (end, stack_bytes) = self
            .address_space(translation.address_space())?
            .stack_pools()
            .peek(prepared.stack_pool())
            .ok()?;
        let bytes = u64::try_from(FAULT_FRAME_BYTES).ok()?;
        if stack_bytes < bytes {
            return None;
        }
        let address = end - bytes;
        A::copy_to_user(translation.root(), address, &frame.to_bytes()).then_some(address)
    }

    /// Read back the fault frame of `fault` from the handler's stack in
    /// `handler_address_space`: the state to resume, and the FP state to load
    /// when the faulting frame is an FP frame. `None` when the frame cannot
    /// be read or the kernel refuses it.
    pub(crate) fn read_fault_frame(
        &self,
        fault: &ThreadFault,
        handler_address_space: AddressSpaceId,
    ) -> Option<(SavedContext, Option<FpState>)> {
        let root = self
            .address_space(handler_address_space)?
            .translation_root()?;
        let mut bytes = [0; FAULT_FRAME_BYTES];
        if !A::copy_from_user(root, fault.frame_address, &mut bytes) {
            return None;
        }
        let frame = FaultFrame::from_bytes(bytes);
        let resumed = resume_state(&frame, &fault.frame)?;
        let fp = self
            .address_space_fp(fault.address_space)
            .then_some(FpState {
                vectors: frame.vectors,
                fpcr: frame.fpcr,
                fpsr: frame.fpsr,
            });
        Some((resumed, fp))
    }

    /// Park the current Thread as `Faulted` with `faulting` as its state,
    /// releasing any fault it was handling, and select the next Thread.
    /// Used for unhandled faults and for a handler's terminate action.
    pub fn park_faulted(
        &mut self,
        access: &Access,
        current: ThreadId,
        faulting: SavedContext,
    ) -> Result<Switch, CapError> {
        self.release_thread_fault(current);
        self.release_thread_placements(access, current);
        self.park_faulted_and_select(access, faulting)
    }

    /// Drop the fault a Thread is handling, freeing its handler. Called when
    /// the Thread stops for good (parked as faulted, or retired).
    pub fn release_thread_fault(&mut self, thread: ThreadId) {
        let Some(thread) = self.thread_mut(thread) else {
            return;
        };
        if let Some(fault) = thread.fault.take() {
            self.set_fault_handler_busy(fault.address_space, false);
        }
    }

    /// The fault handler of `address_space`, if it has one and it is not
    /// already running a fault.
    fn available_fault_handler(
        access: &Access,
        address_space: AddressSpaceId,
    ) -> Option<CallTarget> {
        let space = access
            .resolve_object::<A::AddressSpace>(address_space)
            .ok()?;
        if space.fault_handler_busy() {
            return None;
        }
        let table = access
            .resolve_object::<KeyTable>(space.keytable()?.id())
            .ok()?;
        table.fault_handler()
    }

    pub(crate) fn set_fault_handler_busy(&mut self, address_space: AddressSpaceId, busy: bool) {
        // A retired AddressSpace has no handler left to free.
        if let Some(space) = self.address_space_mut(address_space) {
            space.set_fault_handler_busy(busy);
        }
    }
}

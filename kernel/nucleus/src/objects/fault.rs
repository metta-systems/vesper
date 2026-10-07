//! Fault delivery: a synchronous upcall on the faulting Thread.
//!
//! A fault is delivered by performing a forced `Invocation.Call` on the
//! faulting Thread itself into the `Invocation` held at
//! `KeySlot::FAULT_HANDLER` in the table of the `AddressSpace` it is executing
//! in, so the handler runs with the faulting Thread's priority and budget.
//! The handler finishes with an ordinary `Thread.Return`; when that Return
//! pops the fault continuation, `commit_return` resumes from the saved fault
//! frame according to the handler's `FaultAction` instead of the continuation.
//!
//! A fault nobody takes — no handler, a busy handler, a fault inside the
//! handler, or a forced Call the ordinary Call rules reject — parks the Thread
//! as `Faulted` and counts the fault on its `AddressSpace`.

use {
    crate::objects::{
        ArchObjects, ExecutionContext, Nucleus, ThreadFault,
        access::{Access, ObjectId},
        arch_objects::AddressSpaceObject,
        invocation::{CallTarget, CommittedCall},
        key_table::KeyTable,
        resume::PreparedResume,
    },
    libexception::arch::aarch64::SavedContext,
    libobject::{CapError, fault::FaultInfo},
};

/// The outcome of delivering a fault on the current Thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultDelivery {
    /// The handler was entered: entry installs the translation and restores
    /// `target`, exactly like a committed Call.
    Handler(CommittedCall),
    /// Nobody took the fault: the Thread is parked as `Faulted`; entry resumes
    /// the selected next Thread.
    Unhandled(PreparedResume),
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
        let current_index = self.current_index()?;
        let thread = self
            .pools
            .threads
            .get_live(current_index)
            .ok_or(CapError::InvalidDomain)?;
        if thread.context != ExecutionContext::Running {
            return Err(CapError::InvalidOperation);
        }
        let faulting_address_space = thread.address_space;
        let depth = thread.invocation_stack.len();

        // One fault level per Thread: a fault inside its handler is unhandled.
        let handler = if thread.fault.is_some() {
            None
        } else {
            self.available_fault_handler(access, faulting_address_space)
        };
        if let Some(target) = handler {
            let mut call_frame = faulting;
            call_frame.gpr[2..8].copy_from_slice(&info.to_arguments());
            // The handler starts at the top of its Invocation's stack.
            call_frame.gpr[9] = target.stack_extent.end();
            if let Ok(prepared) = self.prepare_call(access, &call_frame, target) {
                let committed = self.commit_call(prepared)?;
                let thread = self
                    .pools
                    .threads
                    .get_live_mut(current_index)
                    .ok_or(CapError::InvalidDomain)?;
                thread.fault = Some(ThreadFault {
                    frame: faulting,
                    address_space: faulting_address_space,
                    depth: depth + 1,
                });
                self.set_fault_handler_busy(faulting_address_space, true);
                return Ok(FaultDelivery::Handler(committed));
            }
        }

        if let Some(space) = self.live_address_space_mut(faulting_address_space) {
            space.count_unhandled_fault();
        }
        self.park_faulted(access, current_index, faulting)
            .map(FaultDelivery::Unhandled)
    }

    /// Park the current Thread as `Faulted` with `faulting` as its state,
    /// releasing any fault it was handling, and select the next Thread.
    /// Used for unhandled faults and for a handler's terminate action.
    pub fn park_faulted(
        &mut self,
        access: &Access,
        current_index: usize,
        faulting: SavedContext,
    ) -> Result<PreparedResume, CapError> {
        self.release_thread_fault(current_index);
        self.park_faulted_and_select(access, faulting)
    }

    /// Drop the fault a Thread is handling, freeing its handler. Called when
    /// the Thread stops for good (parked as faulted, or retired).
    pub fn release_thread_fault(&mut self, thread_index: usize) {
        let Some(thread) = self.pools.threads.get_live_mut(thread_index) else {
            return;
        };
        if let Some(fault) = thread.fault.take() {
            self.set_fault_handler_busy(fault.address_space, false);
        }
    }

    /// The fault handler of `address_space`, if it has one and it is not
    /// already running a fault.
    fn available_fault_handler(
        &self,
        access: &Access,
        address_space: ObjectId,
    ) -> Option<CallTarget> {
        let space = access
            .resolve(&self.pools.arch.address_spaces, address_space)
            .ok()?;
        if space.fault_handler_busy() {
            return None;
        }
        let table = access
            .resolve_carved_mut::<KeyTable>(space.keytable().address())
            .ok()?;
        table.fault_handler()
    }

    pub(crate) fn set_fault_handler_busy(&mut self, address_space: ObjectId, busy: bool) {
        // A retired AddressSpace has no handler left to free.
        if let Some(space) = self.live_address_space_mut(address_space) {
            space.set_fault_handler_busy(busy);
        }
    }

    /// The live `AddressSpace` named by `id` (generation-checked), if any.
    fn live_address_space_mut(&mut self, id: ObjectId) -> Option<&mut A::AddressSpace> {
        let pool = &mut self.pools.arch.address_spaces;
        pool.validate(id).ok()?;
        pool.get_live_mut(usize::from(id.index))
    }

    fn current_index(&self) -> Result<usize, CapError> {
        let current = self.current_thread.ok_or(CapError::InvalidDomain)?;
        usize::try_from(current).map_err(|_invalid_index| CapError::InvalidDomain)
    }
}

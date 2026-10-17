use {
    crate::objects::{
        NucleusObject,
        access::ObjectId,
        arch_objects::AddressSpaceId,
        carved::{CarvedId, CarvedObject},
    },
    core::mem::{align_of, size_of},
    libexception::arch::aarch64::{ExceptionOrigin, SavedContext},
    libobject::{INVOCATION_STACK_DEPTH, ObjectType},
};

// ====================
// == Nucleus object ==
// ====================

/// Kernel-private execution context of a Thread
///
/// This records only what the kernel needs to stop and later resume the
/// thread's execution; the saved register state itself lives in the
/// exception frame on the thread's kernel stack (see `vectors.S`).
///
/// Implementation status: the preceding stack-carried description is superseded
/// by the fully interrupt-kernel contract. First-start and parked state now live
/// inline in the Thread; a trap frame is transient and owns no continuation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionContext {
    /// Created by `Untyped.Retype` and not configured: no `AddressSpace`, no
    /// execution state, never runnable.
    Inactive,
    /// Configured by the root Scheduler and not started yet: it first runs
    /// `saved` once `Scheduler.StartThread` makes it runnable.
    Configured { saved: SavedContext },
    /// The thread has never run: start it at `pc` with a full-descending
    /// kernel stack whose top is `stack_top`.
    /// Implementation status: `saved` supplies the initial PC and execution SP,
    /// not a per-Thread kernel stack. Trusted `EL1t` fixtures execute on `SP_EL0`.
    NotStarted { saved: SavedContext },
    /// Blocked: the saved exception frame lives at `frame_addr` on the
    /// thread's kernel stack, and the invocation is parked on the
    /// pending-invocation `record`.
    /// Implementation status: `saved` owns the complete execution state, including
    /// SP, raw SPSR and exception origin; no kernel-stack frame address survives.
    /// The checked pending-record identity remains the wait's completion carrier.
    Parked {
        saved: SavedContext,
        record: ObjectId,
    },
    /// Runnable, stopped by the preemption timer: `saved` is the complete
    /// interrupted state, restored as-is (no completion is owed).
    Preempted { saved: SavedContext },
    /// Currently executing, or a fixture thread with no execution context:
    /// no saved state to restore.
    Running,
    /// Stopped by an unhandled fault (or a handler's terminate action) and
    /// never runnable again; `saved` is the faulting state, kept for
    /// inspection until `Thread.Retire`.
    Faulted { saved: SavedContext },
}

/// A fault being handled on this Thread: the one fault level a Thread has.
///
/// Delivery pushes an ordinary continuation for the handler Call; `depth` is
/// the invocation-stack length with that continuation on top, so the Return
/// that pops it resumes from `frame` instead of the continuation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThreadFault {
    /// The complete state of the faulting instruction.
    pub frame: SavedContext,
    /// The `AddressSpace` whose handler took the fault (and is busy).
    pub address_space: AddressSpaceId,
    /// Stack length while the fault continuation is on top.
    pub depth: usize,
}

/// Where `Invocation.CallWithKey` placed a borrowed entry: the slot and the
/// incarnation issued to it in the callee's table. Incarnations start at 1, so
/// a zero incarnation means the call lent no key.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    /// Slot of the borrowed entry in the callee's table.
    pub slot: u32,
    /// The incarnation issued when the entry was installed.
    pub incarnation: u32,
}

impl Placement {
    /// No key was lent.
    pub const NONE: Self = Self {
        slot: 0,
        incarnation: 0,
    };

    /// Whether a borrowed entry was placed.
    pub const fn is_some(self) -> bool {
        self.incarnation != 0
    }
}

/// Kernel-owned continuation captured at a successful PPC Call.
///
/// The record is deliberately independent of the transient exception frame and
/// target stack. Its exact shape is part of the storage contract; Call/Return
/// admission and migration are separate work.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvocationContinuation {
    pub source_address_space: AddressSpaceId,
    pub source_pc: u64,
    pub source_sp: u64,
    pub stamp: u64,
    pub source_spsr: u64,
    pub source_origin: ExceptionOrigin,
    pub source_x19_x30: [u64; 12],
    /// The source's EL0 TLS: the target is entered with zero, Return restores this.
    pub source_tpidr_el0: u64,
    /// The borrowed entry `CallWithKey` placed in the callee's table for this
    /// call, to be removed when the call returns or its Thread is torn down.
    /// [`Placement::NONE`] for a plain Call.
    pub placement: Placement,
}

impl InvocationContinuation {
    pub const fn empty() -> Self {
        Self {
            source_address_space: AddressSpaceId::UNBOUND,
            source_pc: 0,
            source_sp: 0,
            stamp: 0,
            source_spsr: 0,
            source_origin: ExceptionOrigin::CurrentSp0,
            source_x19_x30: [0; 12],
            source_tpidr_el0: 0,
            placement: Placement::NONE,
        }
    }

    pub fn from_saved(
        source_address_space: AddressSpaceId,
        saved: SavedContext,
        stamp: u64,
    ) -> Self {
        let mut source_x19_x30 = [0; 12];
        source_x19_x30[..11].copy_from_slice(&saved.gpr[19..30]);
        source_x19_x30[11] = saved.lr;
        Self {
            source_address_space,
            source_pc: saved.elr_el1,
            source_sp: saved.sp,
            stamp,
            source_spsr: saved.spsr_el1,
            source_origin: saved.origin,
            source_x19_x30,
            source_tpidr_el0: saved.tpidr_el0,
            placement: Placement::NONE,
        }
    }
}

/// Fixed-capacity, inline PPC continuation storage for one Thread.
///
/// It never allocates and a full push leaves the stack unchanged.
/// This primitive is intentionally not wired to Call/Return yet.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvocationStack {
    records: [InvocationContinuation; INVOCATION_STACK_DEPTH],
    len: u8,
}

impl InvocationStack {
    pub const fn new() -> Self {
        Self {
            records: [InvocationContinuation::empty(); INVOCATION_STACK_DEPTH],
            len: 0,
        }
    }

    pub const fn len(&self) -> usize {
        self.len as usize
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub const fn is_full(&self) -> bool {
        self.len as usize == INVOCATION_STACK_DEPTH
    }

    pub fn push(&mut self, record: InvocationContinuation) -> Result<(), ()> {
        if self.is_full() {
            return Err(());
        }
        self.records[self.len as usize] = record;
        self.len += 1;
        Ok(())
    }

    pub fn pop(&mut self) -> Option<InvocationContinuation> {
        if self.is_empty() {
            return None;
        }
        self.len -= 1;
        let index = self.len as usize;
        let record = self.records[index];
        self.records[index] = InvocationContinuation::empty();
        Some(record)
    }

    /// The saved continuations, oldest first.
    pub fn records(&self) -> &[InvocationContinuation] {
        &self.records[..self.len as usize]
    }

    pub const fn top(&self) -> Option<&InvocationContinuation> {
        if self.len == 0 {
            None
        } else {
            Some(&self.records[self.len as usize - 1])
        }
    }
}

impl Default for InvocationStack {
    fn default() -> Self {
        Self::new()
    }
}

/// The schedulable execution entity: a core kind that holds a capability
/// to its `AddressSpace` (the protection/mapping context) and to its `KeyTable` (the `CSpace`
/// equivalent), plus the kernel-private execution state.
///
/// Implementation status: the carved `KeyTable` is associated with the
/// `AddressSpace`, not stored separately in each Thread. It is resolved through
/// the guarded `Access` context (see `doc/capabilities-design.md` §3).
/// The fully interrupt-kernel saved-context conversion remains separate work.
/// Implementation status: this Thread now owns its saved context; trap-frame
/// capture/restore and the shared per-core stack are entry/backend responsibilities.
/// The bounded PPC invocation stack is inline below; Call/Return admission
/// and migration remain separate work.
pub struct Thread {
    // ═══════════════════════════════════════════════════════════
    // PRIVATE SECTION (kernel only, NOT mapped to userspace)
    // ═══════════════════════════════════════════════════════════
    //
    // This would be in a separate structure or after a page boundary
    // - Saved register context
    // - Capability space (keytable)
    // - Kernel stack pointer
    // - Etc.
    // Implementation status: the preceding private-section sketch records design
    // intent, not a per-Thread kernel-stack allocation. The kernel trap stack is
    // per-core; persistent register state is carried by `context` below.
    /// The checked identity of this thread's `AddressSpace` (the
    /// protection/mapping context). A thread executes in exactly one address
    /// space;
    /// resolution validates the identity against the carved record's header,
    /// so a retired address space fails here with a defined error.
    pub address_space: AddressSpaceId,
    /// Execution context for stopping and resuming this Thread (blocked
    /// callers park here; never-run threads carry their first-start entry).
    pub context: ExecutionContext,
    /// Inline bounded PPC continuations: `commit_call` pushes and
    /// `commit_return` pops here, both through dispatched Call/Return.
    pub invocation_stack: InvocationStack,
    /// The fault being handled, if any (one level per Thread).
    pub fault: Option<ThreadFault>,
    /// The next Thread in the runnable queue while this one is queued: the
    /// queue links through the Thread objects, so it needs no storage of its
    /// own however many Threads exist.
    pub run_next: Option<ThreadId>,
    /// Whether this Thread is in the runnable queue.
    pub queued: bool,
    /// The Thread's own Time, recorded by `Scheduler.ConfigureThread`. Kernel
    /// bookkeeping, not authority: a Thread migrates between `AddressSpace`s
    /// and owns no `KeyTable`, so its binding cannot be a key. A retired Time
    /// fails validation, leaving the Thread unbound.
    pub time: Option<CarvedId>,
    /// The Scheduler that configured the Thread (bookkeeping, as `time`).
    pub scheduler: Option<CarvedId>,
    /// Nanoseconds this Thread has executed, charged each time it is
    /// switched away from. Kernel-private bookkeeping.
    pub run_time_ns: u64,
}

/// Kernel identity of a Thread: its carved identity. Holding one grants
/// nothing; authority over a Thread is a key in a `KeyTable`.
pub type ThreadId = CarvedId;

impl Thread {
    /// A Thread in `address_space` with execution state `context`, an empty
    /// invocation stack, no fault and not queued.
    pub const fn new(address_space: AddressSpaceId, context: ExecutionContext) -> Self {
        Self {
            address_space,
            context,
            invocation_stack: InvocationStack::new(),
            fault: None,
            run_next: None,
            queued: false,
            time: None,
            scheduler: None,
            run_time_ns: 0,
        }
    }
}

// The record and array sizes are part of the storage/accounting contract.
const _: () = {
    assert!(size_of::<InvocationContinuation>() == 168);
    assert!(align_of::<InvocationContinuation>() == 8);
    assert!(size_of::<InvocationStack>() >= 168 * INVOCATION_STACK_DEPTH);
};
// Thread backing is type-derived and is not required to fit in one page.
const _: () = {
    assert!(size_of::<SavedContext>() == 288);
    assert!(align_of::<SavedContext>() == 8);
    assert!(align_of::<Thread>() == 8);
    assert!(size_of::<ExecutionContext>() >= size_of::<SavedContext>() + size_of::<ObjectId>());
    assert!(
        size_of::<ExecutionContext>()
            <= size_of::<SavedContext>() + size_of::<ObjectId>() + align_of::<SavedContext>()
    );
    assert!(size_of::<Thread>() >= size_of::<InvocationStack>());
};

impl NucleusObject for Thread {
    const TYPE: ObjectType = ObjectType::THREAD;
}

impl CarvedObject for Thread {}

#[cfg(test)]
mod tests {
    use {
        super::{ExecutionContext, InvocationContinuation, InvocationStack, Thread},
        crate::objects::{
            access::{ObjectId, PoolTag},
            arch_objects::AddressSpaceId,
            carved::{self, Carved},
        },
        libexception::arch::aarch64::{ExceptionContext, ExceptionOrigin, SavedContext},
    };

    fn address_space() -> AddressSpaceId {
        carved::test_id(libobject::ObjectType::ADDRESS_SPACE, 0x1000)
    }

    fn saved_fixture(seed: u64) -> SavedContext {
        let mut saved = SavedContext::el1t(0x80_0000 + seed * 4, 0x90_0000 + seed * 16);
        for (index, register) in saved.gpr.iter_mut().enumerate() {
            *register = (seed << 32) | u64::try_from(index).unwrap();
        }
        saved.lr = 0xA0_0000 + seed * 4;
        saved.spsr_el1 |= 0xA000_0000;
        saved
    }

    #[test_case]
    fn first_start_owns_an_initial_el1t_execution_context() {
        const INITIAL: SavedContext = SavedContext::el1t(0x80_0000, 0x90_0000);
        let thread = Thread {
            address_space: address_space(),
            context: ExecutionContext::NotStarted { saved: INITIAL },
            invocation_stack: super::InvocationStack::new(),
            fault: None,
            run_next: None,
            queued: false,
            time: None,
            scheduler: None,
            run_time_ns: 0,
        };
        let ExecutionContext::NotStarted { saved } = thread.context else {
            panic!("new Thread has no initial execution context")
        };
        assert_eq!(saved.gpr, [0; 30]);
        assert_eq!(saved.lr, 0);
        assert_eq!(saved.spsr_el1, 0x3c4);
        assert_eq!(saved.elr_el1, 0x80_0000);
        assert_eq!(saved.sp, 0x90_0000);
        assert_eq!(saved.origin, ExceptionOrigin::CurrentSp0);
    }

    #[test_case]
    fn parked_context_survives_transient_trap_frame_reuse() {
        let expected = saved_fixture(1);
        let record = ObjectId {
            pool: PoolTag::Pending,
            index: 2,
            generation: 7,
        };
        let mut frame = ExceptionContext::from(expected);
        let thread = Thread {
            address_space: address_space(),
            context: ExecutionContext::Parked {
                saved: frame.save(),
                record,
            },
            invocation_stack: super::InvocationStack::new(),
            fault: None,
            run_next: None,
            queued: false,
            time: None,
            scheduler: None,
            run_time_ns: 0,
        };

        // Reuse every byte of the transient frame for another execution context.
        // Neither the saved registers nor the wait identity may depend on it.
        let mut unrelated = saved_fixture(2);
        unrelated.origin = ExceptionOrigin::LowerAarch64;
        unrelated.spsr_el1 = 0x5000_0340;
        frame.restore(unrelated);
        let ExecutionContext::Parked {
            saved,
            record: retained_record,
        } = thread.context
        else {
            panic!("Thread lost its parked context")
        };
        assert_eq!(saved, expected);
        assert_eq!(retained_record, record);
        assert_eq!(thread.address_space, address_space());
        assert_ne!(frame.save(), saved);
        frame.restore(saved);
        assert_eq!(frame.save(), expected);
    }

    #[test_case]
    fn invocation_continuation_captures_source_state_and_registers() {
        let source = address_space();
        let saved = saved_fixture(7);
        let record = InvocationContinuation::from_saved(source, saved, 99);
        assert_eq!(record.source_address_space, source);
        assert_eq!(record.source_pc, saved.elr_el1);
        assert_eq!(record.source_sp, saved.sp);
        assert_eq!(record.stamp, 99);
        assert_eq!(record.source_spsr, saved.spsr_el1);
        assert_eq!(record.source_origin, saved.origin);
        assert_eq!(&record.source_x19_x30[..11], &saved.gpr[19..30]);
        assert_eq!(record.source_x19_x30[11], saved.lr);
    }

    #[test_case]
    fn invocation_stack_is_bounded_and_lifo_without_allocation() {
        let mut stack = InvocationStack::new();
        assert!(stack.is_empty());
        for stamp in 0..libobject::INVOCATION_STACK_DEPTH {
            let mut record = InvocationContinuation::empty();
            record.stamp = u64::try_from(stamp).unwrap();
            assert!(stack.push(record).is_ok());
            assert_eq!(stack.len(), stamp + 1);
        }
        assert!(stack.is_full());
        let rejected = InvocationContinuation {
            stamp: 0xfeed,
            ..InvocationContinuation::empty()
        };
        assert!(stack.push(rejected).is_err());
        assert_eq!(stack.top().unwrap().stamp, 15);
        assert_eq!(stack.len(), libobject::INVOCATION_STACK_DEPTH);
        for stamp in (0..libobject::INVOCATION_STACK_DEPTH).rev() {
            assert_eq!(stack.top().unwrap().stamp, u64::try_from(stamp).unwrap());
            assert_eq!(stack.pop().unwrap().stamp, u64::try_from(stamp).unwrap());
        }
        assert!(stack.is_empty());
        assert!(stack.pop().is_none());
    }

    #[test_case]
    fn carved_threads_keep_parked_contexts_isolated_and_retire_alone() {
        #[repr(C, align(64))]
        struct Backing([u8; 2 * STRIDE]);
        const STRIDE: usize = Carved::<Thread>::SIZE.next_multiple_of(64);
        let mut backing = Backing([0; 2 * STRIDE]);
        let base = backing.0.as_mut_ptr();
        let contexts = [1, 2].map(|seed| ExecutionContext::Parked {
            saved: saved_fixture(seed),
            record: ObjectId {
                pool: PoolTag::Pending,
                index: u16::try_from(seed).unwrap(),
                generation: 10,
            },
        });
        // SAFETY: the backing is exclusively owned, 64-byte aligned and holds
        // two strides.
        let ids = unsafe {
            [
                Carved::<Thread>::initialize(base, Thread::new(address_space(), contexts[0])),
                Carved::<Thread>::initialize(
                    base.add(STRIDE),
                    Thread::new(address_space(), contexts[1]),
                ),
            ]
        };
        for (id, expected) in ids.iter().zip(contexts) {
            // SAFETY: the identities were issued for this backing.
            let thread = unsafe { &*Carved::<Thread>::validate(*id).ok().unwrap() };
            assert_eq!(thread.context, expected);
            assert!(!thread.queued);
        }

        // SAFETY: as above; no reference is live.
        unsafe { Carved::<Thread>::retire(ids[0]) }.ok().unwrap();
        // SAFETY: as above.
        assert!(unsafe { Carved::<Thread>::validate(ids[0]) }.is_err());
        // SAFETY: as above.
        let survivor = unsafe { &*Carved::<Thread>::validate(ids[1]).ok().unwrap() };
        assert_eq!(survivor.context, contexts[1]);
    }
}

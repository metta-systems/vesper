use {
    crate::objects::{NucleusObject, access::ObjectId},
    core::mem::{align_of, size_of},
    libexception::arch::aarch64::SavedContext,
    libobject::ObjectType,
};

// ====================
// == Nucleus object ==
// ====================

/// Kernel-private execution context of a Thread (completion foundation,
/// 2026-09-16).
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
    /// Currently executing, or a fixture thread with no execution context:
    /// no saved state to restore.
    Running,
}

/// The schedulable execution entity: a core kind that holds a capability
/// to its `AddressSpace` (the protection/mapping context) and to its `KeyTable` (the `CSpace`
/// equivalent), plus the kernel-private execution state.
///
/// Implementation status: the carved `KeyTable` is associated with the
/// `AddressSpace`, not stored separately in each Thread. It is resolved through
/// the guarded `Access` context (see `doc/lifetime-and-authority.md` §3).
/// Placement of its userspace capability in the DCB's fixed slots remains D5.
/// The fully interrupt-kernel saved-context conversion remains separate work.
/// Implementation status: this Thread now owns its saved context; trap-frame
/// capture/restore and the shared per-core stack are entry/backend responsibilities.
/// The bounded PPC invocation stack remains separate, unimplemented work.
///
/// The `DomainControlBlock` is user-visible and is defined in libobject.
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
    /// resolution validates the identity against the address-space pool, so a
    /// retired address space fails here with a defined error.
    pub address_space: ObjectId,
    /// Execution context for stopping and resuming this Thread (blocked
    /// callers park here; never-run threads carry their first-start entry).
    pub context: ExecutionContext,
}

// Verify size for cache alignment
// TODO const _: () = assert!(core::mem::size_of::<Thread>() == 4096);
// Implementation status: page-fitting private state, not a page-sized object or
// scheduler-shared record ABI. Pool accounting includes its own slot metadata.
const _: () = {
    assert!(size_of::<SavedContext>() == 280);
    assert!(align_of::<SavedContext>() == 8);
    assert!(align_of::<Thread>() == 8);
    assert!(size_of::<ExecutionContext>() >= size_of::<SavedContext>() + size_of::<ObjectId>());
    assert!(
        size_of::<ExecutionContext>()
            <= size_of::<SavedContext>() + size_of::<ObjectId>() + align_of::<SavedContext>()
    );
    assert!(
        size_of::<Thread>()
            <= size_of::<SavedContext>() + 2 * size_of::<ObjectId>() + align_of::<SavedContext>()
    );
    assert!(size_of::<Thread>() <= 4096);
    assert!(crate::objects::ObjectPool::<Thread>::carve_size(1) <= 4096);
};

impl NucleusObject for Thread {
    const TYPE: ObjectType = ObjectType::THREAD;
    const POOL: crate::objects::access::PoolTag = crate::objects::access::PoolTag::Thread;
}

#[cfg(test)]
mod tests {
    use {
        super::{ExecutionContext, Thread},
        crate::objects::{
            ObjectPool,
            access::{ObjectId, PoolTag},
        },
        core::mem::{MaybeUninit, align_of, size_of},
        libexception::arch::aarch64::{ExceptionContext, ExceptionOrigin, SavedContext},
    };

    fn address_space() -> ObjectId {
        ObjectId {
            pool: PoolTag::AddressSpace,
            index: 0,
            generation: 1,
        }
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
    fn thread_pool_keeps_parked_contexts_isolated_across_slot_reuse() {
        const CAPACITY: usize = 4;
        const WORDS: usize = ObjectPool::<Thread>::carve_size(CAPACITY).div_ceil(size_of::<u128>());
        let mut backing = MaybeUninit::<[u128; WORDS]>::uninit();
        assert!(align_of::<[u128; WORDS]>() >= ObjectPool::<Thread>::ALIGN);
        assert!(size_of::<[u128; WORDS]>() >= ObjectPool::<Thread>::carve_size(CAPACITY));
        assert!(ObjectPool::<Thread>::carve_size(CAPACITY) <= 4096);
        // SAFETY: type-derived, carve-aligned backing remains exclusively owned
        // here for the pool's lifetime. No object references escape the test.
        let mut pool =
            unsafe { ObjectPool::<Thread>::initialize(backing.as_mut_ptr().cast(), CAPACITY) };
        let mut identities = [address_space(); CAPACITY];
        let mut contexts = [ExecutionContext::Running; CAPACITY];
        for index in 0..CAPACITY {
            let context = ExecutionContext::Parked {
                saved: saved_fixture(u64::try_from(index + 1).unwrap()),
                record: ObjectId {
                    pool: PoolTag::Pending,
                    index: u16::try_from(index).unwrap(),
                    generation: 10 + u32::try_from(index).unwrap(),
                },
            };
            identities[index] = pool
                .allocate(Thread {
                    address_space: address_space(),
                    context,
                })
                .expect("type-derived pool backing lost capacity")
                .0;
            contexts[index] = context;
        }
        assert_eq!(pool.capacity(), CAPACITY);
        assert!(
            pool.allocate(Thread {
                address_space: address_space(),
                context: ExecutionContext::Running,
            })
            .is_none()
        );
        for (index, expected) in contexts.iter().enumerate() {
            assert_eq!(pool.get_live(index).unwrap().context, *expected);
        }

        pool.deallocate(identities[0])
            .unwrap_or_else(|error| panic!("Thread deallocation failed: {:?}", error.code()));
        let initial = SavedContext::el1t(0xB0_0000, 0xC0_0000);
        let (replacement, _) = pool
            .allocate(Thread {
                address_space: address_space(),
                context: ExecutionContext::NotStarted { saved: initial },
            })
            .unwrap();
        assert_eq!(replacement.index, identities[0].index);
        assert_ne!(replacement.generation, identities[0].generation);
        assert!(pool.validate(identities[0]).is_err());
        assert_eq!(
            pool.get_live(usize::from(replacement.index))
                .unwrap()
                .context,
            ExecutionContext::NotStarted { saved: initial }
        );
        for (index, expected) in contexts.iter().enumerate().skip(1) {
            assert_eq!(pool.get_live(index).unwrap().context, *expected);
        }
        pool.deallocate(replacement)
            .unwrap_or_else(|error| panic!("replacement deallocation failed: {:?}", error.code()));
        for identity in identities.iter().skip(1) {
            pool.deallocate(*identity)
                .unwrap_or_else(|error| panic!("Thread deallocation failed: {:?}", error.code()));
        }
    }
}

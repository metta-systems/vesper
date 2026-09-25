use {
    crate::objects::{NucleusObject, access::ObjectId},
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionContext {
    /// The thread has never run: start it at `pc` with a full-descending
    /// kernel stack whose top is `stack_top`.
    NotStarted { pc: u64, stack_top: u64 },
    /// Blocked: the saved exception frame lives at `frame_addr` on the
    /// thread's kernel stack, and the invocation is parked on the
    /// pending-invocation `record`.
    Parked { frame_addr: u64, record: ObjectId },
    /// Currently executing, or a fixture thread with no execution context:
    /// no saved state to restore.
    Running,
}

/// The schedulable execution entity: a core kind that holds a capability
/// to its `AddressSpace` (the protection/mapping context) and to its `KeyTable` (the `CSpace`
/// equivalent), plus the kernel-private execution state.
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
    /// Kernel address of this thread's capability table (a carved `KeyTable`).
    ///
    /// The table is a Retype-created carved object, resolved through the
    /// guarded `Access` context (see `doc/lifetime-and-authority.md` §3).
    /// Placement of the table capability in the DCB's fixed slots is follow-up
    /// (D5).
    pub keytable_addr: u64,
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

impl NucleusObject for Thread {
    const TYPE: ObjectType = ObjectType::THREAD;
    const POOL: crate::objects::access::PoolTag = crate::objects::access::PoolTag::Thread;
}

//! Guarded kernel object access (D3 concrete guarded-access selection, 2026-09-07).
//!
//! Capabilities store only a checked object identity — pool tag, pool index,
//! and allocation generation — never a raw object pointer. The owning access
//! context computes object addresses from pool bases after validating
//! authoritative per-pool slot metadata (allocation state + generation), so
//! stale pointers cannot be dereferenced. Validation always precedes
//! dereference.
//!
//! The context is constructed once per invocation under the kernel lock and is
//! `!Send`; guards borrow the context, so the borrow checker enforces that
//! guards end before scheduling. Multi-operand operations resolve same-object
//! aliases through explicit pair forms that reject aliased mutable operands up
//! front.

use {
    crate::objects::{NucleusObject, object_pool::ObjectPool},
    core::marker::PhantomData,
    libobject::CapError,
};

// ═══════════════════════════════════════════════════════════════════
// POOL TAGS
// ═══════════════════════════════════════════════════════════════════

/// Identifies which typed pool an object lives in.
///
/// This is kernel-internal identity, not wire ABI; it never crosses the
/// syscall boundary. Each `NucleusObject` type maps to exactly one pool tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PoolTag {
    /// Marker for inline region types (Untyped, Frame) that are never pooled.
    Region = 255,
    Domain = 0,
    KeyTable = 1,
    // Arch pool tags are appended after core tags. Values are kernel-internal
    // and may be renumbered between builds; they are never serialized.
    PageTable = 16,
    VSpace = 17,
    ASIDPool = 18,
    #[expect(clippy::upper_case_acronyms, reason = "matches the ASID object name")]
    ASID = 19,
}

impl PoolTag {
    /// Reconstruct a tag from its stored byte. Unknown bytes map to `Region`,
    /// which no pooled type uses, so validation against `T::POOL` rejects them.
    pub fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Domain,
            1 => Self::KeyTable,
            16 => Self::PageTable,
            17 => Self::VSpace,
            18 => Self::ASIDPool,
            19 => Self::ASID,
            _ => Self::Region,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// OBJECT IDENTITY
// ═══════════════════════════════════════════════════════════════════

/// Checked identity of a pool-allocated kernel object.
///
/// This is a persistent handle: capabilities and pending operations carry
/// `ObjectId`, never raw pointers or Rust references. Dereferencing requires
/// the owning access context, which validates the identity against
/// authoritative pool metadata first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectId {
    /// Which typed pool owns the object.
    pub pool: PoolTag,
    /// Slot index within the pool.
    pub index: u16,
    /// Expected allocation generation of that slot.
    pub generation: u32,
}

// ═══════════════════════════════════════════════════════════════════
// GUARDS
// ═══════════════════════════════════════════════════════════════════

/// Short-lived shared guard for a validated object.
///
/// Borrows the access context, so it cannot outlive the locked invocation
/// section. Constructed only by `Access` after metadata validation.
pub struct Guard<'ctx, T: NucleusObject> {
    ptr: *const T,
    _ctx: PhantomData<&'ctx T>,
}

impl<'ctx, T: NucleusObject> Guard<'ctx, T> {
    /// # Safety
    /// Caller must have validated the identity against authoritative pool
    /// metadata and ensured the pool backing outlives `'ctx`.
    unsafe fn new(ptr: *const T) -> Self {
        Self {
            ptr,
            _ctx: PhantomData,
        }
    }
}

impl<T: NucleusObject> core::ops::Deref for Guard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: identity was validated by Access before construction; the
        // guard cannot outlive the locked invocation section.
        unsafe { &*self.ptr }
    }
}

/// Short-lived exclusive guard for a validated object.
///
/// Only one `GuardMut` for a given object may exist within an invocation;
/// aliased operands are rejected by the pair-resolution forms.
pub struct GuardMut<'ctx, T: NucleusObject> {
    ptr: *mut T,
    _ctx: PhantomData<&'ctx mut T>,
}

impl<'ctx, T: NucleusObject> GuardMut<'ctx, T> {
    /// # Safety
    /// Same contract as `Guard::new`, plus caller guarantees exclusivity.
    unsafe fn new(ptr: *mut T) -> Self {
        Self {
            ptr,
            _ctx: PhantomData,
        }
    }
}

impl<T: NucleusObject> core::ops::Deref for GuardMut<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: see Guard::deref.
        unsafe { &*self.ptr }
    }
}

impl<T: NucleusObject> core::ops::DerefMut for GuardMut<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: see Guard::deref; exclusivity established by Access.
        unsafe { &mut *self.ptr }
    }
}

// ═══════════════════════════════════════════════════════════════════
// ACCESS CONTEXT
// ═══════════════════════════════════════════════════════════════════

/// Owning kernel access context for one invocation.
///
/// Constructed once per syscall entry while the kernel lock is held. Not
/// `Send` (via `PhantomData<*mut ()>`), so it cannot cross cores or threads.
/// All object dereference goes through this context; handlers receive guards,
/// not raw pointers.
///
/// Mutable resolution exclusively borrows the pool for the invocation, so the
/// borrow checker prevents holding two mutable guards into one pool. Clients
/// requiring two objects from the same pool must request them through
/// `resolve_pair_mut` rather than sequential `resolve_mut` calls. One pool
/// per type tag is the invariant making this sufficient; introducing multiple
/// pools per type requires revisiting cross-pool alias enforcement.
pub struct Access<'ctx> {
    // !Send / !Sync: the context is bound to the current locked invocation.
    _not_send: PhantomData<*mut ()>,
    _lifetime: PhantomData<&'ctx ()>,
}

impl<'ctx> Access<'ctx> {
    /// Construct the access context for the current invocation.
    ///
    /// # Safety
    /// Caller must hold the kernel lock for the whole `'ctx` lifetime and must
    /// not construct a second overlapping context.
    pub unsafe fn new() -> Self {
        Self {
            _not_send: PhantomData,
            _lifetime: PhantomData,
        }
    }

    /// Resolve an identity to a shared guard after validation.
    #[expect(
        clippy::unused_self,
        reason = "the context receiver ties guard lifetimes to the locked invocation"
    )]
    pub fn resolve<T: NucleusObject>(
        &self,
        pool: &'ctx ObjectPool<T>,
        id: ObjectId,
    ) -> Result<Guard<'ctx, T>, CapError> {
        let obj = pool.validate(id)?;
        // SAFETY: validate() checked allocation state and generation; the
        // pool borrow ties backing lifetime to 'ctx.
        Ok(unsafe { Guard::new(obj) })
    }

    /// Resolve an identity to an exclusive guard after validation.
    #[expect(
        clippy::unused_self,
        reason = "the context receiver ties guard lifetimes to the locked invocation"
    )]
    pub fn resolve_mut<T: NucleusObject>(
        &self,
        pool: &'ctx mut ObjectPool<T>,
        id: ObjectId,
    ) -> Result<GuardMut<'ctx, T>, CapError> {
        let obj = pool.validate_mut(id)?;
        // SAFETY: validate_mut() checked metadata and yields the only mutable
        // pointer to this slot; the exclusive pool borrow prevents a second
        // overlapping guard through this pool reference.
        Ok(unsafe { GuardMut::new(obj) })
    }

    /// Resolve two operands of the same pool, rejecting same-object aliases.
    ///
    /// Two operands within one execution may name the same object; an outer
    /// lock does not make two simultaneous mutable references disjoint. This
    /// form rejects the alias before constructing any reference.
    #[expect(
        clippy::unused_self,
        reason = "the context receiver ties guard lifetimes to the locked invocation"
    )]
    pub fn resolve_pair_mut<T: NucleusObject>(
        &self,
        pool: &'ctx mut ObjectPool<T>,
        first: ObjectId,
        second: ObjectId,
    ) -> Result<(GuardMut<'ctx, T>, Guard<'ctx, T>), CapError> {
        if first == second {
            // Kernel-internal alias rejection: not a wire status of its own.
            return Err(CapError::InvalidOperation);
        }
        let (first_ptr, second_ptr) = pool.validate_pair(first, second)?;
        // SAFETY: validate_pair checked both identities and established that
        // the slots are distinct, so the pointers are disjoint.
        Ok(unsafe { (GuardMut::new(first_ptr), Guard::new(second_ptr)) })
    }
}

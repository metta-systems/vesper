use {
    crate::objects::{NucleusObject, access::ObjectId},
    libobject::CapError,
};

// ═══════════════════════════════════════════════════════════════════
// OBJECT POOLS
// ═══════════════════════════════════════════════════════════════════

// FIXME: allocate a whole pool (X objects of same type) via untyped retype and then get objects from pool as needed

/// Authoritative lifecycle state of one pool slot.
///
/// This metadata has stable backing independent of the object storage it
/// describes, so validation never dereferences potentially-freed object
/// memory. `Retired` is distinct from `Free`: retirement is recorded before
/// backing reuse, enabling the `ObjectRetired` inconsistency diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotState {
    Free,
    Live,
    Retired,
}

/// Per-slot authoritative metadata: allocation state plus allocation
/// generation. The generation is retained after deallocation and never
/// silently wraps; exhaustion prohibits further reuse of the slot.
#[derive(Clone, Copy, Debug)]
pub struct SlotMeta {
    pub state: SlotState,
    pub generation: u32,
}

/// Maximum slots per pool. Metadata is inline, so this bounds pool size.
pub const MAX_POOL_SLOTS: usize = 256;

/// A pool of kernel objects of type T, backed by untyped memory.
///
/// Objects are allocated via Untyped.Retype and live until revoked.
/// Allocation/generation/retirement metadata lives here, authoritatively;
/// capabilities carry only `ObjectId` handles validated against this metadata
/// before any dereference.
pub struct ObjectPool<T: NucleusObject> {
    /// Base address of the pool
    base: *mut T,
    /// Per-slot authoritative metadata
    meta: [SlotMeta; MAX_POOL_SLOTS],
    /// Number of live objects
    count: u16,
    /// Total capacity
    capacity: u16,
    /// Next-fit cursor: the slot to start scanning from on the next
    /// allocation. Advances monotonically (wrapping around the pool) so
    /// allocation is amortized O(1) rather than always scanning from slot 0.
    next_free: u16,
}

impl<T: NucleusObject> ObjectPool<T> {
    /// Maximum slots per pool. Metadata is inline, so this bounds pool size.
    pub const MAX_SLOTS: usize = MAX_POOL_SLOTS;

    /// Create a new pool backed by untyped memory
    ///
    /// # Safety
    /// The untyped memory must be properly sized and aligned for T
    pub unsafe fn new(memory: *mut u8, size: usize) -> Self {
        let capacity = u16::try_from(size / core::mem::size_of::<T>()).unwrap();
        assert!(capacity as usize <= MAX_POOL_SLOTS);

        Self {
            base: memory.cast(),
            meta: [SlotMeta {
                state: SlotState::Free,
                generation: 0,
            }; MAX_POOL_SLOTS],
            count: 0,
            capacity,
            next_free: 0,
        }
    }

    /// Allocate an object in the pool, returning its checked identity.
    ///
    /// Advances the slot's allocation generation; the first generation is 1.
    /// Returns `None` when the pool is full or every free slot's generation
    /// is exhausted.
    ///
    /// Uses a next-fit cursor: allocation scans forward from the last cursor
    /// position (wrapping around the pool) and records the slot after the one
    /// taken, so allocation is amortized O(1) rather than always scanning from
    /// slot 0. Slots whose generation is exhausted are skipped during the scan.
    pub fn allocate(&mut self, init: T) -> Option<(ObjectId, &mut T)> {
        let capacity = usize::from(self.capacity);
        let start = usize::from(self.next_free);
        // Find the next free slot with a non-exhausted generation, scanning
        // forward from the cursor and wrapping around the pool. Skipping
        // exhausted slots here avoids reporting pool-full while a later slot
        // is still usable.
        let slot = (0..capacity)
            .map(|i| (start + i) % capacity)
            .find(|&slot| {
                let m = &self.meta[slot];
                m.state == SlotState::Free && m.generation != u32::MAX
            })?;
        // Record the next-fit cursor: the slot after the one we take.
        self.next_free = u16::try_from((slot + 1) % capacity).ok()?;

        let meta = &mut self.meta[slot];
        debug_assert_eq!(meta.state, SlotState::Free);
        // Exhausted generations were filtered above, so this cannot overflow.
        meta.generation += 1;
        meta.state = SlotState::Live;
        self.count += 1;

        // Initialize the object
        // SAFETY: slot < capacity, backing is valid for T, and the slot was
        // Free so no live references exist into it.
        let obj = unsafe {
            let ptr = self.base.add(slot);
            ptr.write(init);
            &mut *ptr
        };
        let id = ObjectId {
            pool: T::POOL,
            index: u16::try_from(slot).ok()?,
            generation: meta.generation,
        };
        Some((id, obj))
    }

    /// Validate an identity and return a shared pointer to the live object.
    ///
    /// Checks allocation state and generation against authoritative metadata
    /// before computing the address; never inspects the object storage.
    pub fn validate(&self, id: ObjectId) -> Result<*const T, CapError> {
        self.check(id)?;
        // SAFETY: check() proved the slot is Live with matching generation,
        // so the backing holds a live T.
        Ok(unsafe { self.base.add(usize::from(id.index)) })
    }

    /// Validate an identity and return an exclusive pointer to the live object.
    pub fn validate_mut(&mut self, id: ObjectId) -> Result<*mut T, CapError> {
        self.check(id)?;
        // SAFETY: check() proved liveness; the &mut self borrow guarantees no
        // other guard into this pool is constructed through this reference.
        Ok(unsafe { self.base.add(usize::from(id.index)) })
    }

    /// Validate two distinct identities, returning mutable and shared pointers.
    ///
    /// Rejects same-slot aliases: two operands within one execution may name
    /// the same object, and an outer lock does not make two simultaneous
    /// mutable references disjoint.
    pub fn validate_pair(
        &mut self,
        first: ObjectId,
        second: ObjectId,
    ) -> Result<(*mut T, *const T), CapError> {
        if first.index == second.index {
            return Err(CapError::InvalidOperation);
        }
        self.check(first)?;
        self.check(second)?;
        // SAFETY: both identities are Live and name distinct slots, so the
        // computed pointers are disjoint.
        Ok(unsafe {
            (
                self.base.add(usize::from(first.index)),
                self.base.add(usize::from(second.index)),
            )
        })
    }

    /// Deallocate an object, retaining its generation for stale-handle
    /// rejection. The slot becomes Free and may be reallocated with an
    /// advanced generation.
    pub fn deallocate(&mut self, id: ObjectId) -> Result<(), CapError> {
        self.check(id)?;
        let slot = usize::from(id.index);
        self.meta[slot].state = SlotState::Free;
        self.count -= 1;

        // Drop the object
        // SAFETY: the slot was Live with a matching generation, so the
        // backing holds a live T being dropped exactly once.
        unsafe {
            core::ptr::drop_in_place(self.base.add(slot));
        }
        Ok(())
    }

    /// Shared access to a live object by pool index, without identity
    /// validation. Kernel-internal bootstrap/fixture paths use this where no
    /// capability-carried identity exists yet; capability resolution must go
    /// through `validate` instead.
    pub fn get_live(&self, index: usize) -> Option<&T> {
        let meta = self.meta.get(index)?;
        if index >= usize::from(self.capacity) || meta.state != SlotState::Live {
            return None;
        }
        // SAFETY: slot is Live and in bounds.
        Some(unsafe { &*self.base.add(index) })
    }

    /// Exclusive access to a live object by pool index; see `get_live`.
    pub fn get_live_mut(&mut self, index: usize) -> Option<&mut T> {
        let meta = self.meta.get(index)?;
        if index >= usize::from(self.capacity) || meta.state != SlotState::Live {
            return None;
        }
        // SAFETY: slot is Live and in bounds; &mut self guarantees exclusivity.
        Some(unsafe { &mut *self.base.add(index) })
    }

    /// The current allocation generation of a live slot by index, without
    /// constructing an object reference. Kernel-internal callers use this to
    /// build incarnation-checked identities for the current domain, where no
    /// capability-carried identity exists yet.
    pub fn generation_of(&self, index: usize) -> Option<u32> {
        let meta = self.meta.get(index)?;
        if index >= usize::from(self.capacity) || meta.state != SlotState::Live {
            return None;
        }
        Some(meta.generation)
    }

    /// Number of live objects.
    pub fn len(&self) -> usize {
        usize::from(self.count)
    }

    /// Whether the pool has no live objects.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Shared metadata check: pool tag, slot bounds, liveness, generation.
    fn check(&self, id: ObjectId) -> Result<(), CapError> {
        if id.pool != T::POOL {
            return Err(CapError::InvalidOperation);
        }
        let meta = self
            .meta
            .get(usize::from(id.index))
            .filter(|_| id.index < self.capacity)
            .ok_or(CapError::InvalidOperation)?;
        // A Live slot with a matching generation is the only success case.
        // Retired, Free, and stale-generation identities are all rejected;
        // distinct inconsistency diagnostics are follow-up work.
        match meta.state {
            SlotState::Live if meta.generation == id.generation => Ok(()),
            _ => Err(CapError::InvalidOperation),
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::{ObjectPool, SlotState},
        crate::objects::{NucleusObject, access::PoolTag},
        core::mem::{MaybeUninit, size_of},
        libobject::ObjectType,
    };

    struct Dummy(u32);

    impl NucleusObject for Dummy {
        const TYPE: ObjectType = ObjectType::THREAD;
        const POOL: PoolTag = PoolTag::Thread;
    }

    fn pool<const N: usize>(backing: &mut MaybeUninit<[Dummy; N]>) -> ObjectPool<Dummy> {
        // SAFETY: the backing is aligned for N Dummys and exclusively owned
        // by the test for the pool's lifetime.
        unsafe { ObjectPool::new(backing.as_mut_ptr().cast::<u8>(), size_of::<[Dummy; N]>()) }
    }

    fn alloc(pool: &mut ObjectPool<Dummy>, value: u32) -> super::ObjectId {
        match pool.allocate(Dummy(value)) {
            Some((id, _)) => id,
            None => panic!("allocation failed"),
        }
    }

    fn dealloc(pool: &mut ObjectPool<Dummy>, id: super::ObjectId) {
        if pool.deallocate(id).is_err() {
            panic!("deallocation failed");
        }
    }

    #[test_case]
    fn allocate_starts_at_generation_one_and_validates() {
        let mut backing = MaybeUninit::<[Dummy; 2]>::uninit();
        let mut pool = pool(&mut backing);
        let id = alloc(&mut pool, 7);
        assert_eq!(id.pool, PoolTag::Thread);
        assert_eq!(id.index, 0);
        assert_eq!(id.generation, 1);
        assert!(pool.validate(id).is_ok());
        assert_eq!(pool.len(), 1);
        assert!(!pool.is_empty());
        dealloc(&mut pool, id);
        assert!(pool.is_empty());
    }

    #[test_case]
    fn stale_identity_is_rejected_after_reuse() {
        let mut backing = MaybeUninit::<[Dummy; 1]>::uninit();
        let mut pool = pool(&mut backing);
        let first = alloc(&mut pool, 1);
        dealloc(&mut pool, first);
        // The stale identity names a Free slot and must be rejected.
        assert!(pool.validate(first).is_err());
        let second = alloc(&mut pool, 2);
        assert_eq!(second.index, first.index);
        assert_eq!(second.generation, 2);
        // The stale identity must not resolve to the replacement occupant.
        assert!(pool.validate(first).is_err());
        assert!(pool.validate(second).is_ok());
        dealloc(&mut pool, second);
    }

    #[test_case]
    fn wrong_pool_tag_and_out_of_bounds_are_rejected() {
        let mut backing = MaybeUninit::<[Dummy; 1]>::uninit();
        let mut pool = pool(&mut backing);
        let id = alloc(&mut pool, 1);
        let mut wrong_pool = id;
        wrong_pool.pool = PoolTag::KeyTable;
        assert!(pool.validate(wrong_pool).is_err());
        let mut out_of_bounds = id;
        out_of_bounds.index = 5;
        assert!(pool.validate(out_of_bounds).is_err());
        dealloc(&mut pool, id);
    }

    #[test_case]
    fn pool_exhaustion_and_generation_exhaustion() {
        let mut backing = MaybeUninit::<[Dummy; 1]>::uninit();
        let mut pool = pool(&mut backing);
        let id = alloc(&mut pool, 1);
        // No free slots remain.
        assert!(pool.allocate(Dummy(2)).is_none());
        dealloc(&mut pool, id);
        // Seed the retained generation to the exhaustion boundary.
        pool.meta[usize::from(id.index)].generation = u32::MAX;
        assert_eq!(pool.meta[usize::from(id.index)].state, SlotState::Free);
        // Generation exhaustion prohibits reuse of the allocation identity.
        assert!(pool.allocate(Dummy(3)).is_none());
    }

    #[test_case]
    fn next_fit_allocates_forward_from_the_cursor() {
        let mut backing = MaybeUninit::<[Dummy; 2]>::uninit();
        let mut pool = pool(&mut backing);
        let first = alloc(&mut pool, 1);
        assert_eq!(first.index, 0);
        dealloc(&mut pool, first);
        // The cursor has advanced past slot 0, so the next allocation wraps to
        // slot 1 rather than restarting at slot 0.
        let second = alloc(&mut pool, 2);
        assert_eq!(second.index, 1);
        dealloc(&mut pool, second);
        // The cursor wraps around and finds the freed slot 0 again.
        let third = alloc(&mut pool, 3);
        assert_eq!(third.index, 0);
        dealloc(&mut pool, third);
    }

    #[test_case]
    fn exhausted_free_slot_is_skipped_in_favor_of_a_usable_one() {
        let mut backing = MaybeUninit::<[Dummy; 2]>::uninit();
        let mut pool = pool(&mut backing);
        let first = alloc(&mut pool, 1);
        let second = alloc(&mut pool, 2);
        dealloc(&mut pool, first);
        dealloc(&mut pool, second);
        // Exhaust slot 0's generation; slot 1 remains usable.
        pool.meta[0].generation = u32::MAX;
        assert_eq!(pool.meta[0].state, SlotState::Free);
        // Allocation must skip the exhausted slot 0 and take slot 1.
        let third = alloc(&mut pool, 3);
        assert_eq!(third.index, 1);
        dealloc(&mut pool, third);
    }

    #[test_case]
    fn pair_resolution_rejects_same_slot_alias() {
        let mut backing = MaybeUninit::<[Dummy; 2]>::uninit();
        let mut pool = pool(&mut backing);
        let first = alloc(&mut pool, 1);
        let second = alloc(&mut pool, 2);
        // Same-slot aliases are rejected before constructing references.
        assert!(pool.validate_pair(first, first).is_err());
        // Distinct live slots resolve to disjoint pointers.
        let (mut_ptr, shared_ptr) = match pool.validate_pair(first, second) {
            Ok(pair) => pair,
            Err(_) => panic!("pair resolution failed"),
        };
        assert_ne!(mut_ptr as usize, shared_ptr as usize);
        dealloc(&mut pool, first);
        dealloc(&mut pool, second);
    }
}

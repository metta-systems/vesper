use {
    crate::{
        api::key_entry::{MIN_ALIGN, RegionPayload},
        objects::{NucleusObject, access::ObjectId},
    },
    libaddress::PhysAddr,
    libobject::CapError,
};

// ═══════════════════════════════════════════════════════════════════
// OBJECT POOLS
// ═══════════════════════════════════════════════════════════════════

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

/// A pool of kernel objects of type T, carved from an Untyped region.
///
/// The pool's carve holds the per-slot authoritative metadata followed by
/// the object storage — `capacity` slots of each, sized solely by the
/// creation capacity (the same variable-size carve pattern as `KeyTable`).
/// This descriptor lives in kernel state (the carved `Nucleus`); `meta`
/// and `base` point into the pool's carve.
///
/// Objects are allocated from the pool as needed and live until revoked.
/// Allocation/generation/retirement metadata lives in the carve,
/// authoritatively; capabilities carry only `ObjectId` handles validated
/// against this metadata before any dereference.
///
/// Pools are created kernel-privately by [`ObjectPool::carve`] — the same
/// internal unused-watermark allocation `Untyped.Retype` performs, not a
/// public capability invocation. Today only bootstrap creates pools (the
/// capacity is specified there, via `PoolCapacities`); runtime pool
/// creation remains future work.
pub struct ObjectPool<T: NucleusObject> {
    /// Per-slot authoritative metadata array (`capacity` entries) at the
    /// carve start.
    meta: *mut SlotMeta,
    /// Object storage array (`capacity` entries) after the metadata.
    base: *mut T,
    /// Number of live objects
    count: u16,
    /// Total carved capacity
    capacity: u16,
    /// Next-fit cursor: the slot to start scanning from on the next
    /// allocation. Advances monotonically (wrapping around the pool) so
    /// allocation is amortized O(1) rather than always scanning from slot 0.
    next_free: u16,
}

impl<T: NucleusObject> ObjectPool<T> {
    /// Carve alignment: the stricter of the metadata/object element
    /// alignments and the watermark encoding granularity (the committed
    /// carve end must stay encodable).
    pub const ALIGN: usize = {
        let element_align = if core::mem::align_of::<T>() > core::mem::align_of::<SlotMeta>() {
            core::mem::align_of::<T>()
        } else {
            core::mem::align_of::<SlotMeta>()
        };
        if element_align > MIN_ALIGN {
            element_align
        } else {
            MIN_ALIGN
        }
    };

    /// Maximum representable capacity: slot indices are `u16`.
    pub const MAX_CAPACITY: usize = u16::MAX as usize;

    /// Byte offset of the object array within a carve of `capacity` slots:
    /// the metadata array, padded to the object alignment.
    const fn objects_offset(capacity: usize) -> usize {
        let meta_bytes = capacity * core::mem::size_of::<SlotMeta>();
        // Element alignments are powers of two.
        (meta_bytes + core::mem::align_of::<T>() - 1) & !(core::mem::align_of::<T>() - 1)
    }

    /// Full carve size of a pool of `capacity` objects: the metadata array,
    /// padded to the object alignment, then the object array, rounded up to
    /// [`Self::ALIGN`] so the committed carve end stays watermark-encodable.
    pub const fn carve_size(capacity: usize) -> usize {
        let objects_end = Self::objects_offset(capacity) + capacity * core::mem::size_of::<T>();
        (objects_end + Self::ALIGN - 1) & !(Self::ALIGN - 1)
    }

    /// Carve a pool of `capacity` objects from an Untyped's unused
    /// watermark range and initialize it kernel-privately: the same
    /// internal allocation `Untyped.Retype` performs, invoked directly
    /// rather than through a public capability operation.
    ///
    /// The carve holds the metadata array and the object storage; the
    /// metadata is zeroed here (every slot `Free`, generation 0). Object
    /// slots are written on `allocate` and never read before that. On any
    /// rejection the Untyped's accounting is unchanged.
    pub fn carve(untyped: &mut RegionPayload, capacity: usize) -> Result<Self, CapError> {
        if capacity > Self::MAX_CAPACITY {
            return Err(CapError::InvalidSize(capacity));
        }
        let total = u64::try_from(Self::carve_size(capacity))
            .map_err(|_unrepresentable| CapError::InvalidSize(capacity))?;
        let reservation = untyped.reserve(u64::try_from(Self::ALIGN).unwrap(), total)?;
        // Commit: the reservation is validated above and initialization
        // cannot fail, so the watermark advance and the carve happen
        // together.
        let paddr = untyped.paddr + u64::try_from(reservation.start).unwrap();
        untyped.set_watermark_bytes(reservation.end);
        let carve = PhysAddr::new(paddr).user_to_kernel().as_mut_ptr::<u8>();
        // SAFETY: `carve` names the freshly reserved, exclusively-owned
        // region of carve_size(capacity) ALIGN-aligned bytes inside the
        // kernel window, with no outstanding access.
        Ok(unsafe { Self::initialize(carve, capacity) })
    }

    /// Initialize a pool over an aligned, exclusively-owned carve of
    /// [`Self::carve_size(capacity)`] kernel-dereferenceable bytes: zero
    /// the metadata array and write this descriptor's pointers into it.
    ///
    /// # Safety
    /// `carve` must be aligned to [`Self::ALIGN`] and hold
    /// `carve_size(capacity)` exclusively-owned bytes with no outstanding
    /// access, for the pool's lifetime.
    #[expect(
        clippy::cast_ptr_alignment,
        reason = "the carve's ALIGN alignment is the caller's documented SAFETY obligation"
    )]
    pub unsafe fn initialize(carve: *mut u8, capacity: usize) -> Self {
        debug_assert!(capacity <= Self::MAX_CAPACITY);
        let meta_bytes = capacity * core::mem::size_of::<SlotMeta>();
        // SAFETY: the caller guaranteed the aligned, exclusively-owned carve
        // of carve_size(capacity) bytes; the arrays lie inside it.
        let (meta, base) = unsafe {
            (
                carve.cast::<SlotMeta>(),
                carve.add(Self::objects_offset(capacity)).cast::<T>(),
            )
        };
        // Zero the metadata array: every slot Free (discriminant 0),
        // generation 0.
        // SAFETY: the metadata array lies within the caller's carve.
        unsafe {
            core::ptr::write_bytes(carve, 0, meta_bytes);
        }
        Self {
            meta,
            base,
            count: 0,
            capacity: u16::try_from(capacity).unwrap(),
            next_free: 0,
        }
    }

    /// The pool's carved slot capacity.
    pub fn capacity(&self) -> usize {
        usize::from(self.capacity)
    }

    /// Shared metadata access. The caller must have bounds-checked the slot.
    #[inline]
    fn meta(&self, slot: usize) -> &SlotMeta {
        debug_assert!(slot < self.capacity());
        // SAFETY: the slot is within the carved capacity, so the pointer
        // lies within the metadata array.
        unsafe { &*self.meta.add(slot) }
    }

    /// Exclusive metadata access. The caller must have bounds-checked the slot.
    #[inline]
    fn meta_mut(&mut self, slot: usize) -> &mut SlotMeta {
        debug_assert!(slot < self.capacity());
        // SAFETY: the slot is within the carved capacity, and `&mut self`
        // guarantees exclusivity.
        unsafe { &mut *self.meta.add(slot) }
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
        let capacity = self.capacity();
        let start = usize::from(self.next_free);
        // Find the next free slot with a non-exhausted generation, scanning
        // forward from the cursor and wrapping around the pool. Skipping
        // exhausted slots here avoids reporting pool-full while a later slot
        // is still usable.
        let slot = (0..capacity)
            .map(|i| (start + i) % capacity)
            .find(|&slot| {
                let m = self.meta(slot);
                m.state == SlotState::Free && m.generation != u32::MAX
            })?;
        // Record the next-fit cursor: the slot after the one we take.
        self.next_free = u16::try_from((slot + 1) % capacity).ok()?;

        let meta = self.meta_mut(slot);
        debug_assert_eq!(meta.state, SlotState::Free);
        // Exhausted generations were filtered above, so this cannot overflow.
        meta.generation += 1;
        meta.state = SlotState::Live;
        let generation = meta.generation;
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
            generation,
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
        self.meta_mut(slot).state = SlotState::Free;
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
        if index >= self.capacity() {
            return None;
        }
        let meta = self.meta(index);
        if meta.state != SlotState::Live {
            return None;
        }
        // SAFETY: slot is Live and in bounds.
        Some(unsafe { &*self.base.add(index) })
    }

    /// Exclusive access to a live object by pool index; see `get_live`.
    pub fn get_live_mut(&mut self, index: usize) -> Option<&mut T> {
        if index >= self.capacity() {
            return None;
        }
        if self.meta(index).state != SlotState::Live {
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
        if index >= self.capacity() {
            return None;
        }
        let meta = self.meta(index);
        (meta.state == SlotState::Live).then_some(meta.generation)
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
        // Bounds precede any pointer arithmetic into the carve.
        let slot = usize::from(id.index);
        if slot >= self.capacity() {
            return Err(CapError::InvalidOperation);
        }
        let meta = self.meta(slot);
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
        crate::{
            api::key_entry::RegionPayload,
            objects::{
                NucleusObject,
                access::{ObjectId, PoolTag},
            },
        },
        core::mem::MaybeUninit,
        libobject::{CapError, ObjectType},
    };

    struct Dummy(u32);

    impl NucleusObject for Dummy {
        const TYPE: ObjectType = ObjectType::THREAD;
        const POOL: PoolTag = PoolTag::Thread;
    }

    /// A 16-byte-aligned backing (the minimum carve alignment) large enough
    /// for any pool carved below; `pool` asserts the carve fits.
    struct Backing([u128; 32]);

    fn pool(backing: &mut MaybeUninit<Backing>, capacity: usize) -> ObjectPool<Dummy> {
        assert!(ObjectPool::<Dummy>::carve_size(capacity) <= core::mem::size_of::<Backing>());
        // SAFETY: the backing is ALIGN-aligned (a u128 array) and exclusively
        // owned by the test for the pool's lifetime.
        unsafe { ObjectPool::initialize(backing.as_mut_ptr().cast::<u8>(), capacity) }
    }

    fn alloc(pool: &mut ObjectPool<Dummy>, value: u32) -> ObjectId {
        match pool.allocate(Dummy(value)) {
            Some((id, _)) => id,
            None => panic!("allocation failed"),
        }
    }

    fn dealloc(pool: &mut ObjectPool<Dummy>, id: ObjectId) {
        if pool.deallocate(id).is_err() {
            panic!("deallocation failed");
        }
    }

    #[test_case]
    fn carve_size_is_alignment_padded_and_meta_precedes_objects() {
        let meta = core::mem::size_of::<super::SlotMeta>();
        let object = core::mem::size_of::<Dummy>();
        for capacity in [0_usize, 1, 2, 3, 8] {
            let size = ObjectPool::<Dummy>::carve_size(capacity);
            // The committed end is carve-aligned (watermark-encodable).
            assert_eq!(size % ObjectPool::<Dummy>::ALIGN, 0);
            // The carve holds the metadata array and the object array.
            assert!(size >= capacity * (meta + object));
        }
        // The object array starts after the metadata array, padded to the
        // object alignment.
        let objects_offset = ObjectPool::<Dummy>::objects_offset(3);
        assert_eq!(objects_offset, 3 * meta);
        assert!(objects_offset % core::mem::align_of::<Dummy>() == 0);
    }

    #[test_case]
    fn initialize_zeroes_the_meta_array() {
        let mut backing = MaybeUninit::<Backing>::uninit();
        // Smear the backing so zeroing is observable.
        // SAFETY: exclusive test-owned memory.
        unsafe {
            core::ptr::write_bytes(
                backing.as_mut_ptr().cast::<u8>(),
                0xFF,
                core::mem::size_of::<Backing>(),
            );
        }
        let pool = pool(&mut backing, 2);
        assert_eq!(pool.capacity(), 2);
        assert!(pool.is_empty());
        for slot in 0..2 {
            assert_eq!(pool.meta(slot).state, SlotState::Free);
            assert_eq!(pool.meta(slot).generation, 0);
        }
    }

    #[test_case]
    fn allocate_starts_at_generation_one_and_validates() {
        let mut backing = MaybeUninit::<Backing>::uninit();
        let mut pool = pool(&mut backing, 2);
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
        let mut backing = MaybeUninit::<Backing>::uninit();
        let mut pool = pool(&mut backing, 1);
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
        let mut backing = MaybeUninit::<Backing>::uninit();
        let mut pool = pool(&mut backing, 1);
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
        let mut backing = MaybeUninit::<Backing>::uninit();
        let mut pool = pool(&mut backing, 1);
        let id = alloc(&mut pool, 1);
        // No free slots remain.
        assert!(pool.allocate(Dummy(2)).is_none());
        dealloc(&mut pool, id);
        // Seed the retained generation to the exhaustion boundary.
        pool.meta_mut(usize::from(id.index)).generation = u32::MAX;
        assert_eq!(pool.meta(usize::from(id.index)).state, SlotState::Free);
        // Generation exhaustion prohibits reuse of the allocation identity.
        assert!(pool.allocate(Dummy(3)).is_none());
    }

    #[test_case]
    fn zero_capacity_pool_rejects_everything() {
        let mut backing = MaybeUninit::<Backing>::uninit();
        let mut pool = pool(&mut backing, 0);
        assert_eq!(pool.capacity(), 0);
        assert!(pool.allocate(Dummy(1)).is_none());
        assert!(pool.get_live(0).is_none());
        assert!(pool.generation_of(0).is_none());
    }

    #[test_case]
    fn next_fit_allocates_forward_from_the_cursor() {
        let mut backing = MaybeUninit::<Backing>::uninit();
        let mut pool = pool(&mut backing, 2);
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
        let mut backing = MaybeUninit::<Backing>::uninit();
        let mut pool = pool(&mut backing, 2);
        let first = alloc(&mut pool, 1);
        let second = alloc(&mut pool, 2);
        dealloc(&mut pool, first);
        dealloc(&mut pool, second);
        // Exhaust slot 0's generation; slot 1 remains usable.
        pool.meta_mut(0).generation = u32::MAX;
        assert_eq!(pool.meta(0).state, SlotState::Free);
        // Allocation must skip the exhausted slot 0 and take slot 1.
        let third = alloc(&mut pool, 3);
        assert_eq!(third.index, 1);
        dealloc(&mut pool, third);
    }

    #[test_case]
    fn pair_resolution_rejects_same_slot_alias() {
        let mut backing = MaybeUninit::<Backing>::uninit();
        let mut pool = pool(&mut backing, 2);
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

    #[test_case]
    fn carve_rejects_unrepresentable_capacity_and_insufficient_memory() {
        let mut untyped = RegionPayload {
            paddr: 0x4000_0000,
            state: 0,
            size_bits: 12,
            is_device: false,
            _pad: 0,
        };
        // Capacity beyond the u16 slot index space is rejected before any
        // reservation.
        assert!(matches!(
            ObjectPool::<Dummy>::carve(&mut untyped, ObjectPool::<Dummy>::MAX_CAPACITY + 1),
            Err(CapError::InvalidSize(_))
        ));
        // A carve larger than the region leaves the Untyped unchanged.
        assert!(matches!(
            ObjectPool::<Dummy>::carve(&mut untyped, 512),
            Err(CapError::InsufficientMemory)
        ));
        assert_eq!(untyped.watermark_bytes(), 0);
    }

    #[test_case]
    fn reserve_folds_base_misalignment_into_the_watermark() {
        let untyped = RegionPayload {
            paddr: 0x1000_0024,
            state: 0,
            size_bits: 20,
            is_device: false,
            _pad: 0,
        };
        // A 32-aligned carve from a base misaligned by 4: the absolute carve
        // address (paddr + start) is aligned, and the committed end stays
        // watermark-granular.
        let reservation = untyped.reserve(32, 64).ok().expect("reserve failed");
        assert_eq!(
            untyped.paddr + u64::try_from(reservation.start).unwrap(),
            0x1000_0040
        );
        assert_eq!(reservation.end % 16, 0);
    }

    #[test_case]
    fn reserve_rejects_unrepresentable_extents() {
        // paddr + region size overflows the address space.
        let untyped = RegionPayload {
            paddr: u64::MAX,
            state: 0,
            size_bits: 12,
            is_device: false,
            _pad: 0,
        };
        assert!(matches!(
            untyped.reserve(16, 16),
            Err(CapError::InvalidSize(12))
        ));
        // size_bits beyond the address width cannot represent the region.
        let untyped = RegionPayload {
            paddr: 0,
            state: 0,
            size_bits: 64,
            is_device: false,
            _pad: 0,
        };
        assert!(matches!(
            untyped.reserve(16, 16),
            Err(CapError::InvalidSize(64))
        ));
    }
}

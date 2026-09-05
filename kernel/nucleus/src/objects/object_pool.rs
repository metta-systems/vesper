use crate::objects::NucleusObject;

// ═══════════════════════════════════════════════════════════════════
// OBJECT POOLS
// ═══════════════════════════════════════════════════════════════════

// FIXME: allocate a whole pool (X objects of same type) via untyped retype and then get objects from pool as needed

/// A pool of kernel objects of type T, backed by untyped memory.
///
/// Objects are allocated via Untyped.Retype and live until revoked.
pub struct ObjectPool<T: NucleusObject> {
    /// Base address of the pool
    base: *mut T,
    /// Bitmap of allocated slots
    allocated: [u64; 4], // 256 objects max per pool
    /// Number of allocated objects
    count: u16,
    /// Total capacity
    capacity: u16,
}

impl<T: NucleusObject> ObjectPool<T> {
    /// Create a new pool backed by untyped memory
    ///
    /// # Safety
    /// The untyped memory must be properly sized and aligned for T
    pub unsafe fn new(memory: *mut u8, size: usize) -> Self {
        let capacity = u16::try_from(size / core::mem::size_of::<T>()).unwrap();
        assert!(capacity <= 256);

        Self {
            base: memory.cast(),
            allocated: [0; 4],
            count: 0,
            capacity,
        }
    }

    /// Allocate an object in the pool, returning a reference
    pub fn allocate(&mut self, init: T) -> Option<&mut T> {
        // Find free slot
        let slot = self.find_free_slot()?;

        // Mark as allocated
        let word = slot / 64;
        let bit = slot % 64;
        self.allocated[word] |= 1 << bit;
        self.count += 1;

        // Initialize the object
        // SAFETY: Unsafe
        unsafe {
            let ptr = self.base.add(slot);
            ptr.write(init);
            Some(&mut *ptr)
        }
    }

    /// Get a reference to an allocated object by index
    pub fn get(&self, index: usize) -> Option<&T> {
        if index >= self.capacity as usize {
            return None;
        }

        let word = index / 64;
        let bit = index % 64;
        if self.allocated[word] & (1 << bit) == 0 {
            return None;
        }

        // SAFETY: Unsafe
        unsafe { Some(&*self.base.add(index)) }
    }

    /// Get a mutable reference to an allocated object
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        if index >= self.capacity as usize {
            return None;
        }

        let word = index / 64;
        let bit = index % 64;
        if self.allocated[word] & (1 << bit) == 0 {
            return None;
        }

        // SAFETY: Unsafe
        unsafe { Some(&mut *self.base.add(index)) }
    }

    /// Deallocate an object
    pub fn deallocate(&mut self, index: usize) -> bool {
        if index >= self.capacity as usize {
            return false;
        }

        let word = index / 64;
        let bit = index % 64;
        if self.allocated[word] & (1 << bit) == 0 {
            return false;
        }

        self.allocated[word] &= !(1 << bit);
        self.count -= 1;

        // Drop the object
        // SAFETY: Unsafe
        unsafe {
            core::ptr::drop_in_place(self.base.add(index));
        }

        true
    }

    fn find_free_slot(&self) -> Option<usize> {
        for (word_idx, &word) in self.allocated.iter().enumerate() {
            if word != !0 {
                let bit = word.trailing_ones() as usize;
                let slot = word_idx * 64 + bit;
                if slot < self.capacity as usize {
                    return Some(slot);
                }
            }
        }
        None
    }
}

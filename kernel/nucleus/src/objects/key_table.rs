// ====================
// == Nucleus object ==
// ====================

/// A capability table for a domain.
///
/// This is what seL4 calls a CNode. Each domain has one.
/// The table itself is a kernel object that can be referenced
/// by capabilities (for capability space manipulation).
pub struct KeyTable {
    /// The actual capability entries
    entries: [KeyEntry; Self::NUM_SLOTS],
    /// Domain that owns this table
    owner: DomainId,
    /// Number of valid entries (for iteration)
    count: u16,
}

impl KeyTable {
    /// Number of slots per table (power of 2 for fast indexing)
    pub const NUM_SLOTS: usize = 256;

    /// Create a new empty capability table
    pub fn new(owner: DomainId) -> Self {
        Self {
            entries: [const { KeyEntry::null() }; Self::NUM_SLOTS],
            owner,
            count: 0,
        }
    }

    /// Lookup a capability by slot index
    #[inline]
    pub fn lookup(&self, slot: KeySlot) -> Result<&KeyEntry, CapError> {
        let idx = slot.0 as usize;
        if idx >= Self::NUM_SLOTS {
            return Err(CapError::InvalidSlot(slot));
        }

        let entry = &self.entries[idx];
        if !entry.is_valid() {
            return Err(CapError::EmptySlot(slot));
        }

        Ok(entry)
    }

    /// Lookup a capability mutably
    #[inline]
    pub fn lookup_mut(&mut self, slot: KeySlot) -> Result<&mut KeyEntry, CapError> {
        let idx = slot.0 as usize;
        if idx >= Self::NUM_SLOTS {
            return Err(CapError::InvalidSlot(slot));
        }

        let entry = &mut self.entries[idx];
        if !entry.is_valid() {
            return Err(CapError::EmptySlot(slot));
        }

        Ok(entry)
    }

    /// Insert a capability at a specific slot
    pub fn insert(&mut self, slot: KeySlot, entry: KeyEntry) -> Result<(), CapError> {
        let idx = slot.0 as usize;
        if idx >= Self::NUM_SLOTS {
            return Err(CapError::InvalidSlot(slot));
        }

        if self.entries[idx].is_valid() {
            return Err(CapError::SlotOccupied(slot));
        }

        self.entries[idx] = entry;
        self.count += 1;
        Ok(())
    }

    /// Remove a capability from a slot
    pub fn remove(&mut self, slot: KeySlot) -> Result<KeyEntry, CapError> {
        let idx = slot.0 as usize;
        if idx >= Self::NUM_SLOTS {
            return Err(CapError::InvalidSlot(slot));
        }

        let entry = core::mem::replace(&mut self.entries[idx], KeyEntry::null());
        if entry.is_valid() {
            self.count -= 1;
            Ok(entry)
        } else {
            Err(CapError::EmptySlot(slot))
        }
    }
}

// KeyTable is itself a kernel object
impl NucleusObject for KeyTable {
    const TYPE: ObjectType = ObjectType::KeyTable;
}

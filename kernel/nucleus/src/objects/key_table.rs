use {
    crate::{api::key_entry::KeyEntry, objects::NucleusObject},
    libobject::{
        CapError, InconsistencyReason, InvalidKeyReason, KeySlot, ObjectType, RawKey,
        domain::DomainId,
    },
};

// ====================
// == Nucleus object ==
// ====================

/// A capability table for a domain.
///
/// This is what seL4 calls a `CNode`. Each domain has one.
/// The table itself is a kernel object that can be referenced
/// by capabilities (for capability space manipulation).
pub struct KeyTable {
    /// The actual capability entries
    entries: [KeyEntry; Self::NUM_SLOTS],
    /// Last issued identity, retained even while its entry is vacant.
    incarnations: [u32; Self::NUM_SLOTS],
    /// Domain that owns this table
    owner: DomainId,
    /// Number of valid entries (for iteration)
    count: usize,
}

/// Pre-commit insertion failure retains ownership of the submitted capability.
pub struct InsertError {
    pub error: CapError,
    pub entry: KeyEntry,
}

impl KeyTable {
    /// Number of slots per table (power of 2 for fast indexing)
    pub const NUM_SLOTS: usize = 256;

    /// Create a new empty capability table
    pub fn new(owner: DomainId) -> Self {
        Self {
            entries: [const { KeyEntry::null() }; Self::NUM_SLOTS],
            incarnations: [0; Self::NUM_SLOTS],
            owner,
            count: 0,
        }
    }

    /// Lookup a capability by slot index
    ///
    /// The selector now includes the expected incarnation. This checks slot
    /// identity, not the referenced object's lifetime; payload access requires
    /// the separate guarded object-identity foundation.
    #[inline]
    pub fn lookup(&self, key: RawKey) -> Result<&KeyEntry, CapError> {
        let idx = self.validate_key(key)?;
        Ok(&self.entries[idx])
    }

    // Lookup a capability mutably
    // Implementation status: unrestricted mutable entry access is intentionally
    // unavailable. It could replace authority without advancing incarnation or
    // updating occupancy; future object operations need guarded transitions.

    fn validate_key(&self, key: RawKey) -> Result<usize, CapError> {
        let invalid = |reason| CapError::InvalidKey {
            key,
            reason,
            operand: 0,
        };
        let inconsistent = |reason| CapError::InconsistentKey {
            key,
            reason,
            operand: 0,
        };
        if key.incarnation() == 0 {
            return Err(invalid(InvalidKeyReason::ZeroIncarnation));
        }
        let idx = usize::try_from(key.slot().0)
            .ok()
            .filter(|&idx| idx < Self::NUM_SLOTS)
            .ok_or_else(|| invalid(InvalidKeyReason::SlotOutOfRange))?;
        if self.incarnations[idx] == 0 {
            return Err(invalid(InvalidKeyReason::NeverIssued));
        }
        if self.incarnations[idx] != key.incarnation() {
            return Err(inconsistent(InconsistencyReason::SlotIncarnationMismatch));
        }
        if !self.entries[idx].is_valid() {
            return Err(inconsistent(InconsistencyReason::CapabilityInvalidated));
        }
        Ok(idx)
    }

    pub fn len(&self) -> usize {
        self.count
    }

    /// The domain that owns this table.
    pub fn owner(&self) -> DomainId {
        self.owner
    }

    /// Pre-validate that a new valid entry may be installed at `slot`,
    /// returning the same error `insert` would return for it: range,
    /// vacancy, and remaining incarnation capacity.
    ///
    /// Used by Retype to validate a run of destination slots before
    /// committing to any of them, so the subsequent installs cannot fail.
    pub fn check_insert(&self, slot: KeySlot) -> Result<(), CapError> {
        let idx = usize::try_from(slot.0)
            .ok()
            .filter(|&idx| idx < Self::NUM_SLOTS)
            .ok_or(CapError::InvalidSlot(slot))?;
        if self.entries[idx].is_valid() {
            return Err(CapError::SlotOccupied(slot));
        }
        self.incarnations[idx]
            .checked_add(1)
            .map(|_| ())
            .ok_or(CapError::KeySlotExhausted(slot))
    }

    /// Advance the watermark of the validated Untyped entry at `key`.
    ///
    /// This is the commit step of Retype: the entry's identity, rights, badge
    /// and incarnation are unchanged; only the region's allocation watermark
    /// moves. Targeted mutation only — unrestricted mutable entry access
    /// remains unavailable.
    pub fn advance_untyped_watermark(
        &mut self,
        key: RawKey,
        new_watermark: usize,
    ) -> Result<(), CapError> {
        let idx = self.validate_key(key)?;
        let entry = &mut self.entries[idx];
        if entry.object_type() != ObjectType::UNTYPED {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::UNTYPED,
                found: entry.object_type(),
            });
        }
        entry.as_untyped_mut()?.set_watermark_bytes(new_watermark);
        Ok(())
    }

    /// Insert a capability at a specific slot
    pub fn insert(&mut self, slot: KeySlot, entry: KeyEntry) -> Result<RawKey, InsertError> {
        let reservation = (|| {
            let idx = usize::try_from(slot.0)
                .ok()
                .filter(|&idx| idx < Self::NUM_SLOTS)
                .ok_or(CapError::InvalidSlot(slot))?;
            if !entry.is_valid() {
                return Err(CapError::NullCapability);
            }
            if self.entries[idx].is_valid() {
                return Err(CapError::SlotOccupied(slot));
            }
            let next = self.incarnations[idx]
                .checked_add(1)
                .ok_or(CapError::KeySlotExhausted(slot))?;
            Ok((idx, next))
        })();
        let (idx, next) = match reservation {
            Ok(reservation) => reservation,
            Err(error) => return Err(InsertError { error, entry }),
        };

        self.entries[idx] = entry;
        self.incarnations[idx] = next;
        self.count += 1;
        Ok(RawKey::new(slot, next))
    }

    /// Remove a capability from a slot
    pub fn remove(&mut self, key: RawKey) -> Result<KeyEntry, CapError> {
        let idx = self.validate_key(key)?;
        let entry = core::mem::replace(&mut self.entries[idx], KeyEntry::null());
        self.count -= 1;
        Ok(entry)
    }
}

// KeyTable is itself a kernel object
impl NucleusObject for KeyTable {
    const TYPE: ObjectType = ObjectType::KEY_TABLE;
    const POOL: crate::objects::access::PoolTag = crate::objects::access::PoolTag::KeyTable;
}

#[cfg(test)]
mod tests {
    use {
        super::{InsertError, KeyTable},
        crate::api::KeyEntry,
        core::mem::{align_of, size_of, size_of_val},
        libobject::{
            CapError, InconsistencyReason, InvalidKeyReason, KeySlot, ObjectType, RawKey, Rights,
            domain::DomainId,
        },
    };

    // Inline Frames are inert metadata fixtures here: no mapping, physical-memory
    // access, object dereference, or claim that Frame lifecycle APIs are enabled.
    // ObjectRetired is schema-only at this stage; these tests cover slot identity.
    fn frame(marker: u32) -> KeyEntry {
        let mut entry = KeyEntry::new_frame(
            u64::from(marker) << 12,
            12,
            marker & 1 != 0,
            Rights(Rights::READ),
        );
        entry
            .as_frame_mut()
            .unwrap_or_else(|_| panic!("not a frame"))
            .state = marker;
        entry
    }

    #[derive(Debug, PartialEq, Eq)]
    struct EntryState {
        kind: ObjectType,
        rights: Rights,
        badge: u16,
        region: Option<(u64, u32, u8, bool, u16)>,
    }

    fn entry_state(entry: &KeyEntry) -> EntryState {
        EntryState {
            kind: entry.object_type(),
            rights: entry.rights(),
            badge: entry.badge(),
            region: if entry.is_valid() {
                let region = entry
                    .as_frame()
                    .unwrap_or_else(|_| panic!("not a frame fixture"));
                Some((
                    region.paddr,
                    region.state,
                    region.size_bits,
                    region.is_device,
                    region._pad,
                ))
            } else {
                None
            },
        }
    }

    struct TableState {
        entries: [EntryState; KeyTable::NUM_SLOTS],
        incarnations: [u32; KeyTable::NUM_SLOTS],
        owner: DomainId,
        count: usize,
    }

    fn table_state(table: &KeyTable) -> TableState {
        TableState {
            entries: core::array::from_fn(|index| entry_state(&table.entries[index])),
            incarnations: table.incarnations,
            owner: table.owner,
            count: table.count,
        }
    }

    fn assert_unchanged(table: &KeyTable, before: &TableState) {
        assert_eq!(table.owner, before.owner);
        assert_eq!(table.count, before.count);
        assert_eq!(table.len(), before.count);
        assert_eq!(table.incarnations, before.incarnations);
        for (entry, expected) in table.entries.iter().zip(&before.entries) {
            assert_eq!(entry_state(entry), *expected);
        }
    }

    fn install(table: &mut KeyTable, slot: KeySlot, entry: KeyEntry) -> RawKey {
        table
            .insert(slot, entry)
            .unwrap_or_else(|failure| panic!("installation failed: {:?}", failure.error.code()))
    }

    fn remove(table: &mut KeyTable, key: RawKey) -> KeyEntry {
        table
            .remove(key)
            .unwrap_or_else(|error| panic!("removal failed: {:?}", error.code()))
    }

    fn reject_install(
        table: &mut KeyTable,
        slot: KeySlot,
        entry: KeyEntry,
        expected: CapError,
    ) -> KeyEntry {
        let before = table_state(table);
        let submitted = entry_state(&entry);
        let InsertError { error, entry } = match table.insert(slot, entry) {
            Err(failure) => failure,
            Ok(_) => panic!("invalid installation succeeded"),
        };
        assert_eq!(error.code(), expected.code());
        assert_eq!(entry_state(&entry), submitted);
        assert_unchanged(table, &before);
        entry
    }

    fn reject_lookup_and_remove(table: &mut KeyTable, key: RawKey, expected: CapError) {
        let before = table_state(table);
        let words = expected.code();
        let error = match table.lookup(key) {
            Err(error) => error,
            Ok(_) => panic!("invalid lookup succeeded"),
        };
        assert_eq!(error.code(), words);
        assert_unchanged(table, &before);
        let error = match table.remove(key) {
            Err(error) => error,
            Ok(_) => panic!("invalid removal succeeded"),
        };
        assert_eq!(error.code(), words);
        assert_unchanged(table, &before);
    }

    fn invalid(key: RawKey, reason: InvalidKeyReason) -> CapError {
        CapError::InvalidKey {
            key,
            reason,
            operand: 0,
        }
    }

    fn inconsistent(key: RawKey, reason: InconsistencyReason) -> CapError {
        CapError::InconsistentKey {
            key,
            reason,
            operand: 0,
        }
    }

    fn slot(index: usize) -> KeySlot {
        KeySlot(u32::try_from(index).unwrap())
    }

    #[test_case]
    fn table_layout_and_initial_accounting_follow_capacity() {
        let table = KeyTable::new(DomainId(42));
        assert_eq!(table.owner, DomainId(42));
        assert_eq!(table.count, 0);
        assert_eq!(table.len(), 0);
        assert_eq!(table.entries.len(), KeyTable::NUM_SLOTS);
        assert_eq!(table.incarnations.len(), KeyTable::NUM_SLOTS);
        assert!(table.entries.iter().all(|entry| !entry.is_valid()));
        assert!(
            table
                .incarnations
                .iter()
                .all(|&incarnation| incarnation == 0)
        );
        assert!(KeyTable::NUM_SLOTS > 0);
        assert!(KeyTable::NUM_SLOTS.is_power_of_two());
        assert!(usize::try_from(KeySlot::DEBUG_CONSOLE.0).unwrap() < KeyTable::NUM_SLOTS);
        assert_eq!(
            size_of_val(&table.entries),
            size_of::<KeyEntry>() * KeyTable::NUM_SLOTS
        );
        assert_eq!(
            size_of_val(&table.incarnations),
            size_of::<u32>() * KeyTable::NUM_SLOTS
        );
        assert_eq!(size_of_val(&table.count), size_of::<usize>());
        // Account for both arrays, owner, count and alignment without freezing the
        // prototype's 32-byte KeyEntry or relying on Rust's private field ordering.
        let fields = size_of_val(&table.entries)
            + size_of_val(&table.incarnations)
            + size_of_val(&table.owner)
            + size_of_val(&table.count);
        let alignment = align_of::<KeyTable>();
        assert!(alignment >= align_of::<KeyEntry>());
        assert_eq!(size_of::<KeyTable>(), fields.next_multiple_of(alignment));
    }

    #[test_case]
    fn null_installation_is_atomic_in_every_slot_state() {
        let mut table = KeyTable::new(DomainId(42));
        for index in 0..KeyTable::NUM_SLOTS {
            let slot = slot(index);
            reject_install(&mut table, slot, KeyEntry::null(), CapError::NullCapability);
            let key = install(&mut table, slot, frame(slot.0 + 1));
            assert_eq!(key, RawKey::new(slot, 1));
            reject_install(&mut table, slot, KeyEntry::null(), CapError::NullCapability);
            remove(&mut table, key);
            reject_install(&mut table, slot, KeyEntry::null(), CapError::NullCapability);
            // Seed only the retained counter to reach otherwise impractical limits.
            table.incarnations[index] = u32::MAX;
            reject_install(&mut table, slot, KeyEntry::null(), CapError::NullCapability);
            assert_eq!(table.len(), 0);
        }
    }

    #[test_case]
    fn out_of_bounds_installation_returns_the_submitted_entry_unchanged() {
        let mut table = KeyTable::new(DomainId(42));
        let guard = install(&mut table, slot(0), frame(7));
        let capacity = u32::try_from(KeyTable::NUM_SLOTS).unwrap();
        // Include the exact boundary and each high-bit slot alias, not just MAX.
        for value in [capacity, capacity + 1, u32::MAX].into_iter().chain(
            (0..32)
                .map(|bit| 1_u32 << bit)
                .filter(|&value| value >= capacity),
        ) {
            let bad = KeySlot(value);
            for entry in [KeyEntry::null(), frame(99)] {
                let returned = reject_install(&mut table, bad, entry, CapError::InvalidSlot(bad));
                if returned.is_valid() {
                    // Ownership is usable after rejection, not merely observable.
                    let key = install(&mut table, slot(1), returned);
                    assert_eq!(
                        entry_state(&remove(&mut table, key)),
                        entry_state(&frame(99))
                    );
                }
            }
        }
        assert_eq!(table.len(), 1);
        assert_eq!(
            entry_state(table.lookup(guard).unwrap_or_else(|_| panic!("guard lost"))),
            entry_state(&frame(7))
        );
    }

    #[test_case]
    fn occupied_installation_preserves_both_entries_and_all_counters() {
        let mut table = KeyTable::new(DomainId(42));
        for index in 0..KeyTable::NUM_SLOTS {
            let slot = slot(index);
            assert_eq!(
                install(&mut table, slot, frame(slot.0 + 1)),
                RawKey::new(slot, 1)
            );
        }
        assert_eq!(table.len(), KeyTable::NUM_SLOTS);
        for index in 0..KeyTable::NUM_SLOTS {
            let slot = slot(index);
            let returned = reject_install(
                &mut table,
                slot,
                frame(u32::MAX),
                CapError::SlotOccupied(slot),
            );
            assert_eq!(
                entry_state(&remove(&mut table, RawKey::new(slot, 1))),
                entry_state(&frame(slot.0 + 1))
            );
            let replacement = install(&mut table, slot, returned);
            assert_eq!(replacement, RawKey::new(slot, 2));
            assert_eq!(
                entry_state(
                    table
                        .lookup(replacement)
                        .unwrap_or_else(|_| panic!("lost entry"))
                ),
                entry_state(&frame(u32::MAX))
            );
            assert_eq!(table.len(), KeyTable::NUM_SLOTS);
        }
    }

    #[test_case]
    fn lookup_and_remove_use_canonical_validation_precedence() {
        let mut table = KeyTable::new(DomainId(42));
        let capacity = u32::try_from(KeyTable::NUM_SLOTS).unwrap();
        for value in [0, capacity - 1, capacity, u32::MAX] {
            let key = RawKey::new(KeySlot(value), 0);
            reject_lookup_and_remove(
                &mut table,
                key,
                invalid(key, InvalidKeyReason::ZeroIncarnation),
            );
        }
        for value in [capacity, capacity + 1, u32::MAX] {
            for incarnation in [1, u32::MAX] {
                let key = RawKey::new(KeySlot(value), incarnation);
                reject_lookup_and_remove(
                    &mut table,
                    key,
                    invalid(key, InvalidKeyReason::SlotOutOfRange),
                );
            }
        }
        for index in 0..KeyTable::NUM_SLOTS {
            let slot = slot(index);
            for incarnation in [1, 2, u32::MAX] {
                let key = RawKey::new(slot, incarnation);
                reject_lookup_and_remove(
                    &mut table,
                    key,
                    invalid(key, InvalidKeyReason::NeverIssued),
                );
            }
            let key = install(&mut table, slot, frame(slot.0 + 1));
            let zero = RawKey::new(slot, 0);
            reject_lookup_and_remove(
                &mut table,
                zero,
                invalid(zero, InvalidKeyReason::ZeroIncarnation),
            );
            for incarnation in [2, u32::MAX] {
                let bad = RawKey::new(slot, incarnation);
                reject_lookup_and_remove(
                    &mut table,
                    bad,
                    inconsistent(bad, InconsistencyReason::SlotIncarnationMismatch),
                );
            }
            remove(&mut table, key);
            reject_lookup_and_remove(
                &mut table,
                zero,
                invalid(zero, InvalidKeyReason::ZeroIncarnation),
            );
            for incarnation in [2, u32::MAX] {
                let bad = RawKey::new(slot, incarnation);
                reject_lookup_and_remove(
                    &mut table,
                    bad,
                    inconsistent(bad, InconsistencyReason::SlotIncarnationMismatch),
                );
            }
            reject_lookup_and_remove(
                &mut table,
                key,
                inconsistent(key, InconsistencyReason::CapabilityInvalidated),
            );
        }
    }

    #[test_case]
    fn same_type_replacement_rejects_stale_removal_without_consuming_incarnations() {
        let mut table = KeyTable::new(DomainId(42));
        let slot = slot(KeyTable::NUM_SLOTS - 1);
        let first = install(&mut table, slot, frame(7));
        let returned = reject_install(&mut table, slot, frame(99), CapError::SlotOccupied(slot));
        assert_eq!(
            entry_state(&remove(&mut table, first)),
            entry_state(&frame(7))
        );
        reject_install(&mut table, slot, KeyEntry::null(), CapError::NullCapability);
        let second = install(&mut table, slot, returned);
        assert_eq!(first, RawKey::new(slot, 1));
        assert_eq!(second, RawKey::new(slot, 2));
        assert_eq!(table.len(), 1);
        reject_lookup_and_remove(
            &mut table,
            first,
            inconsistent(first, InconsistencyReason::SlotIncarnationMismatch),
        );
        assert_eq!(
            entry_state(
                table
                    .lookup(second)
                    .unwrap_or_else(|_| panic!("replacement lost"))
            ),
            entry_state(&frame(99))
        );
        let entry = remove(&mut table, second);
        assert_eq!(table.len(), 0);
        assert_eq!(table.incarnations[usize::try_from(slot.0).unwrap()], 2);
        reject_lookup_and_remove(
            &mut table,
            first,
            inconsistent(first, InconsistencyReason::SlotIncarnationMismatch),
        );
        reject_lookup_and_remove(
            &mut table,
            second,
            inconsistent(second, InconsistencyReason::CapabilityInvalidated),
        );
        let third = install(&mut table, slot, entry);
        assert_eq!(third, RawKey::new(slot, 3));
    }

    #[test_case]
    fn exhaustion_is_slot_local_and_the_final_live_key_remains_usable_and_removable() {
        let mut table = KeyTable::new(DomainId(42));
        let exhausted = slot(0);
        table.incarnations[0] = u32::MAX - 1;
        let previous = RawKey::new(exhausted, u32::MAX - 1);
        reject_lookup_and_remove(
            &mut table,
            previous,
            inconsistent(previous, InconsistencyReason::CapabilityInvalidated),
        );
        reject_install(
            &mut table,
            exhausted,
            KeyEntry::null(),
            CapError::NullCapability,
        );
        let last = install(&mut table, exhausted, frame(7));
        assert_eq!(last, RawKey::new(exhausted, u32::MAX));
        assert_eq!(table.len(), 1);
        assert_eq!(
            entry_state(
                table
                    .lookup(last)
                    .unwrap_or_else(|_| panic!("final key unusable"))
            ),
            entry_state(&frame(7))
        );
        reject_lookup_and_remove(
            &mut table,
            previous,
            inconsistent(previous, InconsistencyReason::SlotIncarnationMismatch),
        );
        reject_install(
            &mut table,
            exhausted,
            KeyEntry::null(),
            CapError::NullCapability,
        );
        let submitted = reject_install(
            &mut table,
            exhausted,
            frame(99),
            CapError::SlotOccupied(exhausted),
        );
        assert_eq!(
            entry_state(&remove(&mut table, last)),
            entry_state(&frame(7))
        );
        assert_eq!(table.len(), 0);
        assert_eq!(table.incarnations[0], u32::MAX);
        reject_lookup_and_remove(
            &mut table,
            last,
            inconsistent(last, InconsistencyReason::CapabilityInvalidated),
        );
        let returned = reject_install(
            &mut table,
            exhausted,
            submitted,
            CapError::KeySlotExhausted(exhausted),
        );
        assert_eq!(CapError::KeySlotExhausted(exhausted).code(), (28, 0, 0));
        let other = install(&mut table, slot(1), returned);
        assert_eq!(other, RawKey::new(slot(1), 1));
        assert_eq!(table.len(), 1);
        assert_eq!(
            entry_state(
                table
                    .lookup(other)
                    .unwrap_or_else(|_| panic!("other slot unusable"))
            ),
            entry_state(&frame(99))
        );
        let returned = remove(&mut table, other);
        // Neither using another slot nor retrying can reset the exhausted identity.
        reject_install(
            &mut table,
            exhausted,
            returned,
            CapError::KeySlotExhausted(exhausted),
        );
        assert_eq!(table.len(), 0);
        assert_eq!(table.incarnations[0], u32::MAX);
        assert_eq!(table.incarnations[1], 1);
    }

    #[test_case]
    fn full_capacity_count_and_cleanup_follow_the_arrays_without_wrapping() {
        let mut table = KeyTable::new(DomainId(42));
        for incarnation in [1, 2] {
            for index in 0..KeyTable::NUM_SLOTS {
                let slot = slot(index);
                assert_eq!(
                    install(&mut table, slot, frame(slot.0 + 1)),
                    RawKey::new(slot, incarnation)
                );
                assert_eq!(table.len(), index + 1);
                assert_eq!(table.count, index + 1);
            }
            assert_eq!(table.len(), table.entries.len());
            assert!(table.entries.iter().all(KeyEntry::is_valid));
            assert!(table.incarnations.iter().all(|&value| value == incarnation));
            for index in (0..KeyTable::NUM_SLOTS).rev() {
                let slot = slot(index);
                assert_eq!(
                    entry_state(&remove(&mut table, RawKey::new(slot, incarnation))),
                    entry_state(&frame(slot.0 + 1))
                );
                assert_eq!(table.len(), index);
                assert_eq!(table.count, index);
            }
            assert!(table.entries.iter().all(|entry| !entry.is_valid()));
            assert!(table.incarnations.iter().all(|&value| value == incarnation));
        }
    }
}

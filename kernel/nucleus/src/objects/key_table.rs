use {
    crate::{
        api::key_entry::KeyEntry,
        objects::{NucleusObject, access::ObjectId},
    },
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
/// This is what seL4 calls a `CNode`. Each domain has one. The table itself is
/// a kernel object that can be referenced by capabilities (for capability
/// space manipulation).
///
/// Guarded key-space package (selected 2026-09-23): the table is a carved,
/// variable-size object — this fixed-size header followed, within the same
/// carve, by `2^size_bits` `KeyEntry` slots and `2^size_bits` `u32`
/// incarnation counters. `size_of::<KeyTable>()` is the header size only;
/// the full carve size is [`KeyTable::carve_size`], and the header is
/// authoritative for the capacity. The table's guard is not stored here: it
/// lives in the capabilities naming the table (`KeyTablePayload`), and every
/// key minted into the table packs it above the slot index.
#[repr(C, align(32))]
pub struct KeyTable {
    /// Domain that owns this table
    owner: DomainId,
    /// Number of valid entries (for iteration)
    count: u32,
    /// Capacity exponent: the table holds `2^size_bits` entries.
    size_bits: u8,
    /// Padding to the 32-byte header size (`KeyEntry` alignment).
    _pad: [u8; 23],
}

/// The error payload of a failed installation: the rejection reason plus the
/// submitted entry, returned so ownership is preserved on failure.
pub struct InsertError {
    pub error: CapError,
    pub entry: KeyEntry,
}

/// The caller's own table for one invocation (guarded key-space package,
/// selected 2026-09-23).
///
/// Resolved once at the syscall entry: the address comes from the current
/// Thread, and the guard comes from the `SELF_KEYTABLE` capability, which the
/// entry validates to name this very table. Every presented key is resolved
/// through this context; keys minted into other tables carry those tables'
/// guards and are rejected here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallerTable {
    /// Kernel address of the caller's carved table.
    pub addr: u64,
    /// The table's guard, from the validated `SELF_KEYTABLE` capability.
    pub guard: u32,
}

impl KeyTable {
    /// Header size in bytes; the entry array starts at this offset within the
    /// carve. Equal to `size_of::<KeyTable>()`.
    pub const HEADER_SIZE: usize = core::mem::size_of::<Self>();

    /// Capacity-exponent bounds (selected 2026-09-23): a table holds
    /// `2^size_bits` entries with `size_bits` from 1 to 20.
    pub const MIN_SIZE_BITS: u8 = 1;
    pub const MAX_SIZE_BITS: u8 = 20;

    /// Number of entries of a table carved with `size_bits`.
    #[inline]
    pub const fn capacity_for(size_bits: u8) -> usize {
        1_usize << size_bits
    }

    /// Full carve size of a table carved with `size_bits`: the header, then
    /// `2^size_bits` entries and incarnation counters, rounded up to the
    /// 32-byte entry alignment.
    pub const fn carve_size(size_bits: u8) -> usize {
        let entries = Self::capacity_for(size_bits);
        let total = Self::HEADER_SIZE
            + entries * (core::mem::size_of::<KeyEntry>() + core::mem::size_of::<u32>());
        (total + 31) & !31_usize
    }

    /// The table's capacity in entries (from its own header).
    #[inline]
    pub fn capacity(&self) -> usize {
        Self::capacity_for(self.size_bits)
    }

    /// The table's capacity exponent.
    #[inline]
    pub fn size_bits(&self) -> u8 {
        self.size_bits
    }

    /// Initialize a freshly carved table kernel-privately: write the header
    /// and zero the entry and incarnation arrays (a null `KeyEntry` is the
    /// all-zero value, so this both initializes and sanitizes the carve).
    ///
    /// # Safety
    /// `carve` must name an exclusively-owned, 32-byte-aligned region of at
    /// least [`KeyTable::carve_size(size_bits)`] bytes inside the kernel
    /// window, with no outstanding access.
    #[expect(
        clippy::cast_ptr_alignment,
        reason = "the carve's 32-byte alignment is the caller's documented SAFETY obligation"
    )]
    pub unsafe fn initialize(carve: *mut u8, owner: DomainId, size_bits: u8) {
        debug_assert!((Self::MIN_SIZE_BITS..=Self::MAX_SIZE_BITS).contains(&size_bits));
        let capacity = Self::capacity_for(size_bits);
        // SAFETY: the caller guaranteed the aligned, exclusively-owned carve
        // of carve_size(size_bits) bytes; the arrays lie inside it.
        unsafe {
            carve.cast::<Self>().write(Self {
                owner,
                count: 0,
                size_bits,
                _pad: [0; 23],
            });
            let entries = carve.add(Self::HEADER_SIZE).cast::<KeyEntry>();
            core::ptr::write_bytes(entries, 0, capacity);
            let counters = carve
                .add(Self::HEADER_SIZE + capacity * core::mem::size_of::<KeyEntry>())
                .cast::<u32>();
            core::ptr::write_bytes(counters, 0, capacity);
        }
    }

    // ── Carve-layout accessors (private; bounds-checked by every caller) ──

    /// Base of the entry array within the carve.
    #[expect(
        clippy::cast_ptr_alignment,
        reason = "the arrays inherit the carve's 32-byte alignment (the caller's SAFETY obligation)"
    )]
    fn entries_base(&self) -> *mut KeyEntry {
        // SAFETY: the entry array is part of the same never-freed carve as
        // the header, starting at the 32-byte boundary; `&self` methods only
        // read through it, and `&mut self` methods hold exclusivity.
        unsafe {
            core::ptr::from_ref::<Self>(self)
                .cast_mut()
                .cast::<u8>()
                .add(Self::HEADER_SIZE)
                .cast::<KeyEntry>()
        }
    }

    /// Base of the incarnation-counter array within the carve.
    #[expect(
        clippy::cast_ptr_alignment,
        reason = "the arrays inherit the carve's 32-byte alignment (the caller's SAFETY obligation)"
    )]
    fn counters_base(&self) -> *mut u32 {
        // SAFETY: the counter array follows the entries inside the same
        // never-freed carve; the header's size_bits bounds its extent.
        unsafe {
            core::ptr::from_ref::<Self>(self)
                .cast_mut()
                .cast::<u8>()
                .add(Self::HEADER_SIZE + self.capacity() * core::mem::size_of::<KeyEntry>())
                .cast::<u32>()
        }
    }

    /// Shared reference to the entry at `idx` (bounds: `idx < capacity`).
    fn entry(&self, idx: usize) -> &KeyEntry {
        debug_assert!(idx < self.capacity());
        // SAFETY: the entry array is part of the same never-freed carve as
        // the header; `idx` is bounds-checked by every caller, and the
        // guarded access context ties this reference to the locked invocation.
        unsafe { &*self.entries_base().add(idx) }
    }

    /// Exclusive reference to the entry at `idx` (bounds: `idx < capacity`).
    fn entry_mut(&mut self, idx: usize) -> &mut KeyEntry {
        debug_assert!(idx < self.capacity());
        // SAFETY: see `entry`; exclusivity is established by the guarded
        // access context (`&mut self`).
        unsafe { &mut *self.entries_base().add(idx) }
    }

    /// The retained incarnation counter of slot `idx`.
    fn incarnation_of(&self, idx: usize) -> u32 {
        debug_assert!(idx < self.capacity());
        // SAFETY: see `entry`.
        unsafe { *self.counters_base().add(idx) }
    }

    /// Set the retained incarnation counter of slot `idx`.
    fn set_incarnation(&mut self, idx: usize, value: u32) {
        debug_assert!(idx < self.capacity());
        // SAFETY: see `entry`.
        unsafe { *self.counters_base().add(idx) = value }
    }

    // ── Key resolution ──

    /// Lookup a capability by its table-relative key.
    ///
    /// The selector includes the expected incarnation and the table's guard;
    /// this checks slot identity, not the referenced object's lifetime; payload
    /// access requires the separate guarded object-identity foundation.
    #[inline]
    pub fn lookup(&self, key: RawKey, guard: u32) -> Result<&KeyEntry, CapError> {
        let idx = self.validate_key(key, guard)?;
        Ok(self.entry(idx))
    }

    // Lookup a capability mutably
    // Implementation status: unrestricted mutable entry access is intentionally
    // unavailable. It could replace authority without advancing incarnation or
    // updating occupancy; future object operations need guarded transitions.

    fn validate_key(&self, key: RawKey, guard: u32) -> Result<usize, CapError> {
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
        // Guard check (selected 2026-09-23): the table-relative address packs
        // the table's guard above the slot index, and a mismatch rejects before
        // indexing. A matching guard implies the index is exactly the low
        // `size_bits` bits, so it is in bounds by construction; the explicit
        // bounds check below is defense against future layout changes.
        let width = u32::from(self.size_bits);
        let address = key.slot().0;
        if address >> width != guard {
            return Err(invalid(InvalidKeyReason::GuardMismatch));
        }
        let idx = usize::try_from(address & ((1_u32 << width) - 1))
            .ok()
            .filter(|&idx| idx < self.capacity())
            .ok_or_else(|| invalid(InvalidKeyReason::SlotOutOfRange))?;
        if self.incarnation_of(idx) == 0 {
            return Err(invalid(InvalidKeyReason::NeverIssued));
        }
        if self.incarnation_of(idx) != key.incarnation() {
            return Err(inconsistent(InconsistencyReason::SlotIncarnationMismatch));
        }
        if !self.entry(idx).is_valid() {
            return Err(inconsistent(InconsistencyReason::CapabilityInvalidated));
        }
        Ok(idx)
    }

    pub fn len(&self) -> usize {
        usize::try_from(self.count).unwrap()
    }

    /// The domain that owns this table.
    pub fn owner(&self) -> DomainId {
        self.owner
    }

    /// The self-table capability at the well-known `SELF_KEYTABLE` slot: the
    /// kernel's source for the caller's own-table guard (guarded key-space
    /// package, selected 2026-09-23). Returns the carved address, guard, and
    /// capacity exponent recorded in the capability payload, or `None` when
    /// the slot is beyond the table's capacity, vacant, or holds a non-
    /// `KeyTable` entry.
    pub fn self_table_capability(&self) -> Option<(u64, u32, u8)> {
        let idx = usize::try_from(KeySlot::SELF_KEYTABLE.0).ok()?;
        if idx >= self.capacity() {
            return None;
        }
        let entry = self.entry(idx);
        if entry.object_type() != ObjectType::KEY_TABLE {
            return None;
        }
        let address = entry.keytable_address().ok()?;
        let (guard, size_bits) = entry.keytable_guard_and_size().ok()?;
        Some((address, guard, size_bits))
    }

    /// Pre-validate that a new valid entry may be installed at `slot` (a bare
    /// index), returning the same error `insert` would return for it: range,
    /// vacancy, and remaining incarnation capacity.
    ///
    /// Used by Retype to validate a run of destination slots before
    /// committing to any of them, so the subsequent installs cannot fail.
    pub fn check_insert(&self, slot: KeySlot) -> Result<(), CapError> {
        let idx = usize::try_from(slot.0)
            .ok()
            .filter(|&idx| idx < self.capacity())
            .ok_or(CapError::InvalidSlot(slot))?;
        if self.entry(idx).is_valid() {
            return Err(CapError::SlotOccupied(slot));
        }
        self.incarnation_of(idx)
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
        guard: u32,
        new_watermark: usize,
    ) -> Result<(), CapError> {
        let idx = self.validate_key(key, guard)?;
        let entry = self.entry_mut(idx);
        if entry.object_type() != ObjectType::UNTYPED {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::UNTYPED,
                found: entry.object_type(),
            });
        }
        entry.as_untyped_mut()?.set_watermark_bytes(new_watermark);
        Ok(())
    }

    /// Record a mapping on the validated Frame entry at `key`.
    ///
    /// This is the commit step of `Frame.Map`: the entry's identity, rights,
    /// badge and incarnation are unchanged; only the frame's mapping record
    /// moves. Targeted mutation only — unrestricted mutable entry access
    /// remains unavailable.
    pub fn record_frame_mapping(
        &mut self,
        key: RawKey,
        guard: u32,
        domain: ObjectId,
        vaddr: u64,
    ) -> Result<(), CapError> {
        let idx = self.validate_key(key, guard)?;
        let entry = self.entry_mut(idx);
        if entry.object_type() != ObjectType::FRAME {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::FRAME,
                found: entry.object_type(),
            });
        }
        entry.as_frame_mut()?.set_mapped(domain, vaddr);
        Ok(())
    }

    /// Clear the mapping record of the validated Frame entry at `key`.
    ///
    /// This is the commit step of `Frame.Unmap`. Targeted mutation only —
    /// unrestricted mutable entry access remains unavailable.
    pub fn clear_frame_mapping(&mut self, key: RawKey, guard: u32) -> Result<(), CapError> {
        let idx = self.validate_key(key, guard)?;
        let entry = self.entry_mut(idx);
        if entry.object_type() != ObjectType::FRAME {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::FRAME,
                found: entry.object_type(),
            });
        }
        entry.as_frame_mut()?.clear_mapped();
        Ok(())
    }

    /// Insert a capability at a specific slot (a bare index).
    ///
    /// The returned key packs `guard` — the installing table's guard — above
    /// the slot index, so the key is only resolvable through capabilities
    /// naming this table.
    pub fn insert(
        &mut self,
        slot: KeySlot,
        entry: KeyEntry,
        guard: u32,
    ) -> Result<RawKey, InsertError> {
        let reservation = (|| {
            let idx = usize::try_from(slot.0)
                .ok()
                .filter(|&idx| idx < self.capacity())
                .ok_or(CapError::InvalidSlot(slot))?;
            if !entry.is_valid() {
                return Err(CapError::NullCapability);
            }
            if self.entry(idx).is_valid() {
                return Err(CapError::SlotOccupied(slot));
            }
            let next = self
                .incarnation_of(idx)
                .checked_add(1)
                .ok_or(CapError::KeySlotExhausted(slot))?;
            Ok((idx, next))
        })();
        let (idx, next) = match reservation {
            Ok(reservation) => reservation,
            Err(error) => return Err(InsertError { error, entry }),
        };

        *self.entry_mut(idx) = entry;
        self.set_incarnation(idx, next);
        self.count += 1;
        Ok(RawKey::from_parts(guard, self.size_bits, slot.0, next))
    }

    /// Remove a capability from a slot.
    pub fn remove(&mut self, key: RawKey, guard: u32) -> Result<KeyEntry, CapError> {
        let idx = self.validate_key(key, guard)?;
        let entry = core::mem::replace(self.entry_mut(idx), KeyEntry::null());
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
        core::mem::{align_of, size_of},
        libobject::{
            CapError, InconsistencyReason, InvalidKeyReason, KeySlot, ObjectType, RawKey, Rights,
            domain::DomainId,
        },
    };

    /// Test-table capacity exponent: the historical 256-slot table.
    const SIZE_BITS: u8 = 8;
    const CAPACITY: usize = KeyTable::capacity_for(SIZE_BITS);
    /// A nonzero guard exercising the guarded key-space machinery: keys
    /// minted into the fixture table carry it above the slot index.
    const GUARD: u32 = 0xC0F_FEE;

    /// 32-byte-aligned backing for the fixture table (the carve needs
    /// `KeyEntry` alignment, which a plain byte array does not provide).
    #[repr(align(32))]
    struct Backing<const N: usize>([u8; N]);

    const BACKING_SIZE: usize = KeyTable::carve_size(SIZE_BITS);

    static mut BACKING: Backing<BACKING_SIZE> = Backing([0; BACKING_SIZE]);

    /// A fresh 256-entry table over the shared backing; tests run
    /// sequentially and each call re-initializes it.
    fn table(owner: DomainId) -> &'static mut KeyTable {
        // SAFETY: the backing is exclusively owned by the currently running
        // test, is 32-byte aligned, and covers the full carve size.
        unsafe {
            let ptr = (&raw mut BACKING.0).cast::<u8>();
            KeyTable::initialize(ptr, owner, SIZE_BITS);
            &mut *(ptr as *mut KeyTable)
        }
    }

    /// Compose a fixture key: the fixture guard above the bare index.
    fn key(slot: KeySlot, incarnation: u32) -> RawKey {
        RawKey::from_parts(GUARD, SIZE_BITS, slot.0, incarnation)
    }

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
        // The unmapped frame's `vaddr` field is repurposed as a test marker.
        entry
            .as_frame_mut()
            .unwrap_or_else(|_| panic!("not a frame"))
            .vaddr = u64::from(marker);
        entry
    }

    #[derive(Debug, PartialEq, Eq)]
    struct EntryState {
        kind: ObjectType,
        rights: Rights,
        badge: u16,
        region: Option<(u64, u64, u8, bool, u8)>,
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
                    region.vaddr,
                    region.size_bits,
                    region.is_device(),
                    region.flags,
                ))
            } else {
                None
            },
        }
    }

    struct TableState {
        entries: [EntryState; CAPACITY],
        incarnations: [u32; CAPACITY],
        owner: DomainId,
        count: usize,
    }

    fn table_state(table: &KeyTable) -> TableState {
        TableState {
            entries: core::array::from_fn(|index| entry_state(table.entry(index))),
            incarnations: core::array::from_fn(|index| table.incarnation_of(index)),
            owner: table.owner,
            count: table.len(),
        }
    }

    fn assert_unchanged(table: &KeyTable, before: &TableState) {
        assert_eq!(table.owner, before.owner);
        assert_eq!(table.len(), before.count);
        for index in 0..CAPACITY {
            assert_eq!(table.incarnation_of(index), before.incarnations[index]);
            assert_eq!(entry_state(table.entry(index)), before.entries[index]);
        }
    }

    fn install(table: &mut KeyTable, slot: KeySlot, entry: KeyEntry) -> RawKey {
        table
            .insert(slot, entry, GUARD)
            .unwrap_or_else(|failure| panic!("installation failed: {:?}", failure.error.code()))
    }

    fn remove(table: &mut KeyTable, key: RawKey) -> KeyEntry {
        table
            .remove(key, GUARD)
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
        let InsertError { error, entry } = match table.insert(slot, entry, GUARD) {
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
        let error = match table.lookup(key, GUARD) {
            Err(error) => error,
            Ok(_) => panic!("invalid lookup succeeded"),
        };
        assert_eq!(error.code(), words);
        assert_unchanged(table, &before);
        let error = match table.remove(key, GUARD) {
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
    fn table_layout_and_carve_size_follow_the_capacity_exponent() {
        let table = table(DomainId(42));
        assert_eq!(table.owner, DomainId(42));
        assert_eq!(table.len(), 0);
        assert_eq!(table.capacity(), CAPACITY);
        assert_eq!(table.size_bits(), SIZE_BITS);
        for index in 0..CAPACITY {
            assert!(!table.entry(index).is_valid());
            assert_eq!(table.incarnation_of(index), 0);
        }
        // The header is exactly one 32-byte KeyEntry-sized unit, and the
        // carve covers header + entries + counters at that alignment.
        assert_eq!(size_of::<KeyTable>(), 32);
        assert_eq!(align_of::<KeyTable>(), align_of::<KeyEntry>());
        assert_eq!(
            KeyTable::carve_size(SIZE_BITS),
            size_of::<KeyTable>() + CAPACITY * (size_of::<KeyEntry>() + size_of::<u32>())
        );
        assert_eq!(KeyTable::carve_size(SIZE_BITS) % align_of::<KeyEntry>(), 0);
        // Capacity-exponent bounds: 2^1 .. 2^20 entries, power-of-two sizes.
        assert!(KeyTable::capacity_for(KeyTable::MIN_SIZE_BITS) >= 2);
        assert!(KeyTable::capacity_for(KeyTable::MAX_SIZE_BITS).is_power_of_two());
        assert!(usize::try_from(KeySlot::DEBUG_CONSOLE.0).unwrap() < CAPACITY);
    }

    #[test_case]
    fn null_installation_is_atomic_in_every_slot_state() {
        let mut table = table(DomainId(42));
        for index in 0..CAPACITY {
            let slot = slot(index);
            reject_install(&mut table, slot, KeyEntry::null(), CapError::NullCapability);
            let installed = install(&mut table, slot, frame(slot.0 + 1));
            assert_eq!(installed, key(slot, 1));
            reject_install(&mut table, slot, KeyEntry::null(), CapError::NullCapability);
            remove(&mut table, installed);
            reject_install(&mut table, slot, KeyEntry::null(), CapError::NullCapability);
            // Seed only the retained counter to reach otherwise impractical limits.
            table.set_incarnation(index, u32::MAX);
            reject_install(&mut table, slot, KeyEntry::null(), CapError::NullCapability);
            assert_eq!(table.len(), 0);
        }
    }

    #[test_case]
    fn out_of_bounds_installation_returns_the_submitted_entry_unchanged() {
        let mut table = table(DomainId(42));
        let first = install(&mut table, slot(0), frame(7));
        let capacity = u32::try_from(CAPACITY).unwrap();
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
                    let installed = install(&mut table, slot(1), returned);
                    assert_eq!(
                        entry_state(&remove(&mut table, installed)),
                        entry_state(&frame(99))
                    );
                }
            }
        }
        assert_eq!(table.len(), 1);
        assert_eq!(
            entry_state(
                table
                    .lookup(first, GUARD)
                    .unwrap_or_else(|_| panic!("first entry lost"))
            ),
            entry_state(&frame(7))
        );
    }

    #[test_case]
    fn occupied_installation_preserves_both_entries_and_all_counters() {
        let mut table = table(DomainId(42));
        for index in 0..CAPACITY {
            let slot = slot(index);
            assert_eq!(install(&mut table, slot, frame(slot.0 + 1)), key(slot, 1));
        }
        assert_eq!(table.len(), CAPACITY);
        for index in 0..CAPACITY {
            let slot = slot(index);
            let returned = reject_install(
                &mut table,
                slot,
                frame(u32::MAX),
                CapError::SlotOccupied(slot),
            );
            assert_eq!(
                entry_state(&remove(&mut table, key(slot, 1))),
                entry_state(&frame(slot.0 + 1))
            );
            let replacement = install(&mut table, slot, returned);
            assert_eq!(replacement, key(slot, 2));
            assert_eq!(
                entry_state(
                    table
                        .lookup(replacement, GUARD)
                        .unwrap_or_else(|_| panic!("lost entry"))
                ),
                entry_state(&frame(u32::MAX))
            );
            assert_eq!(table.len(), CAPACITY);
        }
    }

    #[test_case]
    fn lookup_and_remove_use_canonical_validation_precedence() {
        let mut table = table(DomainId(42));
        let capacity = u32::try_from(CAPACITY).unwrap();
        for value in [0, capacity - 1, capacity, u32::MAX] {
            let bad = RawKey::new(KeySlot(value), 0);
            reject_lookup_and_remove(
                &mut table,
                bad,
                invalid(bad, InvalidKeyReason::ZeroIncarnation),
            );
        }
        // Out-of-capacity addresses carry nonzero guard bits (or collide with
        // the guard), so the guard check rejects them before indexing.
        for value in [capacity, capacity + 1, u32::MAX] {
            for incarnation in [1, u32::MAX] {
                let bad = RawKey::new(KeySlot(value), incarnation);
                reject_lookup_and_remove(
                    &mut table,
                    bad,
                    invalid(bad, InvalidKeyReason::GuardMismatch),
                );
            }
        }
        // A wrong guard on an in-bounds index rejects the same way.
        for index in [0, 1, CAPACITY / 2, CAPACITY - 1] {
            let wrong = RawKey::from_parts(GUARD ^ 0xFFFF, SIZE_BITS, index as u32, 1);
            reject_lookup_and_remove(
                &mut table,
                wrong,
                invalid(wrong, InvalidKeyReason::GuardMismatch),
            );
        }
        for index in 0..CAPACITY {
            let slot = slot(index);
            for incarnation in [1, 2, u32::MAX] {
                let never = key(slot, incarnation);
                reject_lookup_and_remove(
                    &mut table,
                    never,
                    invalid(never, InvalidKeyReason::NeverIssued),
                );
            }
            let installed = install(&mut table, slot, frame(slot.0 + 1));
            let zero = RawKey::from_parts(GUARD, SIZE_BITS, slot.0, 0);
            reject_lookup_and_remove(
                &mut table,
                zero,
                invalid(zero, InvalidKeyReason::ZeroIncarnation),
            );
            for incarnation in [2, u32::MAX] {
                let bad = key(slot, incarnation);
                reject_lookup_and_remove(
                    &mut table,
                    bad,
                    inconsistent(bad, InconsistencyReason::SlotIncarnationMismatch),
                );
            }
            remove(&mut table, installed);
            reject_lookup_and_remove(
                &mut table,
                zero,
                invalid(zero, InvalidKeyReason::ZeroIncarnation),
            );
            for incarnation in [2, u32::MAX] {
                let bad = key(slot, incarnation);
                reject_lookup_and_remove(
                    &mut table,
                    bad,
                    inconsistent(bad, InconsistencyReason::SlotIncarnationMismatch),
                );
            }
            reject_lookup_and_remove(
                &mut table,
                installed,
                inconsistent(installed, InconsistencyReason::CapabilityInvalidated),
            );
        }
    }

    #[test_case]
    fn same_type_replacement_rejects_stale_removal_without_consuming_incarnations() {
        let mut table = table(DomainId(42));
        let slot = slot(CAPACITY - 1);
        let first = install(&mut table, slot, frame(7));
        let returned = reject_install(&mut table, slot, frame(99), CapError::SlotOccupied(slot));
        assert_eq!(
            entry_state(&remove(&mut table, first)),
            entry_state(&frame(7))
        );
        reject_install(&mut table, slot, KeyEntry::null(), CapError::NullCapability);
        let second = install(&mut table, slot, returned);
        assert_eq!(first, key(slot, 1));
        assert_eq!(second, key(slot, 2));
        assert_eq!(table.len(), 1);
        reject_lookup_and_remove(
            &mut table,
            first,
            inconsistent(first, InconsistencyReason::SlotIncarnationMismatch),
        );
        assert_eq!(
            entry_state(
                table
                    .lookup(second, GUARD)
                    .unwrap_or_else(|_| panic!("replacement lost"))
            ),
            entry_state(&frame(99))
        );
        let entry = remove(&mut table, second);
        assert_eq!(table.len(), 0);
        assert_eq!(table.incarnation_of(usize::try_from(slot.0).unwrap()), 2);
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
        assert_eq!(third, key(slot, 3));
    }

    #[test_case]
    fn exhaustion_is_slot_local_and_the_final_live_key_remains_usable_and_removable() {
        let mut table = table(DomainId(42));
        let exhausted = slot(0);
        table.set_incarnation(0, u32::MAX - 1);
        let previous = key(exhausted, u32::MAX - 1);
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
        assert_eq!(last, key(exhausted, u32::MAX));
        assert_eq!(table.len(), 1);
        assert_eq!(
            entry_state(
                table
                    .lookup(last, GUARD)
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
        assert_eq!(table.incarnation_of(0), u32::MAX);
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
        assert_eq!(other, key(slot(1), 1));
        assert_eq!(table.len(), 1);
        assert_eq!(
            entry_state(
                table
                    .lookup(other, GUARD)
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
        assert_eq!(table.incarnation_of(0), u32::MAX);
        assert_eq!(table.incarnation_of(1), 1);
    }

    #[test_case]
    fn full_capacity_count_and_cleanup_follow_the_arrays_without_wrapping() {
        let mut table = table(DomainId(42));
        for incarnation in [1, 2] {
            for index in 0..CAPACITY {
                let slot = slot(index);
                assert_eq!(
                    install(&mut table, slot, frame(slot.0 + 1)),
                    key(slot, incarnation)
                );
                assert_eq!(table.len(), index + 1);
            }
            assert_eq!(table.len(), table.capacity());
            for index in 0..CAPACITY {
                assert!(table.entry(index).is_valid());
                assert_eq!(table.incarnation_of(index), incarnation);
            }
            for index in (0..CAPACITY).rev() {
                let slot = slot(index);
                assert_eq!(
                    entry_state(&remove(&mut table, key(slot, incarnation))),
                    entry_state(&frame(slot.0 + 1))
                );
                assert_eq!(table.len(), index);
            }
            for index in 0..CAPACITY {
                assert!(!table.entry(index).is_valid());
                assert_eq!(table.incarnation_of(index), incarnation);
            }
        }
    }

    #[test_case]
    fn guard_mismatch_rejects_keys_minted_for_another_table() {
        let mut table = table(DomainId(42));
        let installed = install(&mut table, slot(5), frame(7));
        // The minted key carries the table's guard above the index.
        assert_eq!(installed.slot().0, (GUARD << SIZE_BITS) | 5);
        assert_eq!(
            entry_state(
                table
                    .lookup(installed, GUARD)
                    .unwrap_or_else(|_| panic!("guarded key must resolve"))
            ),
            entry_state(&frame(7))
        );
        // Any other guard — including the zero guard of a bare index —
        // rejects before indexing, without revealing slot state.
        for wrong in [0_u32, GUARD ^ 1, GUARD + 1, u32::MAX] {
            let foreign = RawKey::from_parts(wrong, SIZE_BITS, 5, 1);
            reject_lookup_and_remove(
                &mut table,
                foreign,
                invalid(foreign, InvalidKeyReason::GuardMismatch),
            );
        }
        // Zero-incarnation still wins over the guard check (precedence).
        let zero = RawKey::from_parts(GUARD ^ 1, SIZE_BITS, 5, 0);
        reject_lookup_and_remove(
            &mut table,
            zero,
            invalid(zero, InvalidKeyReason::ZeroIncarnation),
        );
        // The entry is untouched by every rejection.
        assert_eq!(table.len(), 1);
    }

    #[test_case]
    fn variable_capacity_tables_bound_their_own_indices() {
        const SMALL_BITS: u8 = 2;
        const SMALL_GUARD: u32 = 0x1234_5678;
        const SMALL_BACKING_SIZE: usize = KeyTable::carve_size(SMALL_BITS);
        static mut SMALL_BACKING: Backing<SMALL_BACKING_SIZE> = Backing([0; SMALL_BACKING_SIZE]);
        // SAFETY: the backing is exclusively owned by the currently running
        // test, is 32-byte aligned, and covers the full carve size.
        let table = unsafe {
            let ptr = (&raw mut SMALL_BACKING.0).cast::<u8>();
            KeyTable::initialize(ptr, DomainId(7), SMALL_BITS);
            &mut *(ptr as *mut KeyTable)
        };
        assert_eq!(table.capacity(), 4);
        assert_eq!(table.size_bits(), SMALL_BITS);
        // In-bounds installs mint keys with the small table's wider guard.
        let installed = table
            .insert(slot(3), frame(7), SMALL_GUARD)
            .unwrap_or_else(|failure| panic!("small-table install failed"));
        assert_eq!(installed, RawKey::from_parts(SMALL_GUARD, SMALL_BITS, 3, 1));
        // Out-of-capacity bare indices reject at pre-validation.
        assert!(matches!(
            table.check_insert(slot(4)),
            Err(CapError::InvalidSlot(slot)) if slot == KeySlot(4)
        ));
        assert!(matches!(
            table.insert(slot(4), frame(9), SMALL_GUARD),
            Err(InsertError {
                error: CapError::InvalidSlot(_),
                ..
            })
        ));
        // The 256-slot fixture's keys do not address the small table: their
        // guard bits are checked against the small table's guard first.
        let foreign = key(slot(3), 1);
        assert!(matches!(
            table.lookup(foreign, SMALL_GUARD),
            Err(CapError::InvalidKey {
                reason: InvalidKeyReason::GuardMismatch,
                ..
            })
        ));
        assert_eq!(table.len(), 1);
    }

    #[test_case]
    fn self_table_capability_reads_the_well_known_slot() {
        let mut table = table(DomainId(42));
        // Absent: the well-known slot is vacant.
        assert_eq!(table.self_table_capability(), None);
        // Wrong kind: a non-KeyTable entry does not anchor invocation.
        install(&mut table, KeySlot::SELF_KEYTABLE, frame(9));
        assert_eq!(table.self_table_capability(), None);
        remove(&mut table, key(KeySlot::SELF_KEYTABLE, 1));
        // Present: the payload's address, guard, and size_bits read back.
        install(
            &mut table,
            KeySlot::SELF_KEYTABLE,
            KeyEntry::new_keytable(0x1000, GUARD, SIZE_BITS, Rights::all(), 0),
        );
        assert_eq!(
            table.self_table_capability(),
            Some((0x1000, GUARD, SIZE_BITS))
        );
    }
}

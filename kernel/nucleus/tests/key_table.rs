#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![feature(format_args_nl)]
#![feature(likely_unlikely)]
#![test_runner(libtest::test_runner)]
#![reexport_test_harness_main = "test_main"]

#[path = "../../../tests/common/mod.rs"]
mod common;

// Compile the production module trees, not replacement capability/handler models.
#[allow(unused)]
#[path = "../src/api/mod.rs"]
mod api;
#[allow(unused)]
#[path = "../src/objects/mod.rs"]
mod objects;

// The interrupt-controller component as the IRQ operations reach it.
#[allow(dead_code)]
#[path = "../src/platform.rs"]
mod platform;

// Only the clock is used here; the tick is armed by the nucleus binary.
#[allow(dead_code)]
#[path = "../src/timer.rs"]
mod timer;

use {
    api::{
        KeyEntry,
        key_entry::{FramePayload, InvocationPayload, ThreadSelector},
    },
    core::{
        mem::{align_of, offset_of, size_of},
        num::NonZero,
    },
    libobject::{
        CapError, InconsistencyReason, KeySlot, ObjectType, PlacementWindow, RawKey, Rights,
        StackPoolHandle,
    },
    objects::{
        KeyTable,
        access::{Access, ObjectId, PoolTag},
        arch_objects::AddressSpaceId,
        carved::{Carved, CarvedId, test_id},
        key_table::CallerTable,
    },
};

// ═══════════════════════════════════════════════════════════════════
// FIXTURES
// ═══════════════════════════════════════════════════════════════════

const FULL: u8 = Rights::DERIVE | Rights::REMOVE | Rights::INSTALL;

/// Fixture-table capacity exponent: the historical 256-slot table.
const SIZE_BITS: u8 = 8;

/// The caller fixture table's guard (nonzero, exercising the guarded
/// key-space machinery).
const CALLER_GUARD: u32 = 0x51A_B7D;

/// The destination fixture table's guard — distinct, so cross-table key
/// confusion is exercised: a key minted into one table never resolves in
/// the other.
const DST_GUARD: u32 = 0x7DA_B51;

/// The slot half of a key in the caller fixture table.
fn caller_slot(index: u32) -> KeySlot {
    KeySlot((CALLER_GUARD << u32::from(SIZE_BITS)) | index)
}

/// The slot half of a key in the destination fixture table.
fn dst_slot(index: u32) -> KeySlot {
    KeySlot((DST_GUARD << u32::from(SIZE_BITS)) | index)
}

/// Fixed RAM address for test-carved `KeyTable`s (QEMU rpi3: 1 GiB RAM at 0).
///
/// The test binary loads at `0x80000` and the DTB sits at `0x8000000`; 512 MiB
/// is clear of both. Carving from a fixed address keeps the large `KeyTable`
/// storage out of the test's stack frame.
const TEST_BACKING: u64 = 0x2000_0000;

/// Carve a `KeyTable` into the fixed test backing at `index`, returning its
/// kernel address.
///
/// Mirrors the boot carve / runtime Retype: the table's storage is the carved
/// region (header plus entries and counters, sized by the capacity exponent)
/// and capabilities reference it by address.
fn carve(index: usize) -> u64 {
    let stride = KeyTable::carve_size(SIZE_BITS);
    let obj = (TEST_BACKING + (index as u64) * (stride as u64)) as *mut u8;
    // SAFETY: TEST_BACKING is RAM, 32-byte aligned (a 2 MiB boundary), and
    // exclusively owned by the test fixture for its lifetime; the stride
    // covers the full variable-size carve.
    unsafe {
        KeyTable::initialize(obj, SIZE_BITS);
    }
    obj as u64
}

/// The identity of the fixture table carved at `addr`.
fn table_id(addr: u64) -> CarvedId {
    // SAFETY: fixture tables are carved by `carve` and stay initialized.
    unsafe { KeyTable::carve_id(addr) }
}

/// A carved-table test fixture: a caller table (with a self-table capability at
/// `SELF_KEYTABLE`) and a second, initially vacant table for cross-table
/// operations. The two tables carry distinct guards.
struct Fixture {
    caller_table_addr: u64,
    dst_table_addr: u64,
    self_table_key: RawKey,
}

impl Fixture {
    fn new(table_rights: u8) -> Self {
        let caller_table_addr = carve(0);
        let dst_table_addr = carve(1);
        let self_table_key = unsafe { &mut *KeyTable::at_carve(caller_table_addr) }
            .insert(
                KeySlot::SELF_KEYTABLE,
                KeyEntry::new_keytable(
                    table_id(caller_table_addr),
                    CALLER_GUARD,
                    SIZE_BITS,
                    Rights(table_rights),
                    0,
                ),
                CALLER_GUARD,
            )
            .unwrap_or_else(|_| panic!("self-table installation failed"));
        Self {
            caller_table_addr,
            dst_table_addr,
            self_table_key,
        }
    }

    fn table_addr(&self, index: usize) -> u64 {
        match index {
            0 => self.caller_table_addr,
            1 => self.dst_table_addr,
            _ => panic!("no such table"),
        }
    }

    /// The guard of a table by fixture index.
    fn table_guard(&self, index: usize) -> u32 {
        match index {
            0 => CALLER_GUARD,
            1 => DST_GUARD,
            _ => panic!("no such table"),
        }
    }

    /// Install an entry into a table by fixture index.
    fn install(&mut self, index: usize, slot: KeySlot, entry: KeyEntry) -> RawKey {
        let addr = self.table_addr(index);
        let guard = self.table_guard(index);
        // SAFETY: the address names a live carved table owned by the fixture.
        unsafe { &mut *KeyTable::at_carve(addr) }
            .insert(slot, entry, guard)
            .unwrap_or_else(|_| panic!("fixture installation failed"))
    }

    /// Look up an entry in a table by fixture index.
    fn lookup(&self, index: usize, key: RawKey) -> Result<&KeyEntry, CapError> {
        let addr = self.table_addr(index);
        let guard = self.table_guard(index);
        // SAFETY: the address names a live carved table owned by the fixture.
        Ok(unsafe { &*KeyTable::at_carve(addr) }.lookup(key, guard)?)
    }

    /// Remove an entry from a table by fixture index.
    fn remove(&mut self, index: usize, key: RawKey) -> Result<KeyEntry, CapError> {
        let addr = self.table_addr(index);
        let guard = self.table_guard(index);
        // SAFETY: the address names a live carved table owned by the fixture.
        unsafe { &mut *KeyTable::at_carve(addr) }.remove(key, guard)
    }

    /// The caller table's live-entry count.
    fn caller_len(&self) -> usize {
        // SAFETY: see lookup.
        unsafe { &*KeyTable::at_carve(self.caller_table_addr) }.len()
    }

    /// Invoke the KeyTable management handler against the fixture.
    fn invoke(
        &mut self,
        table_key: RawKey,
        op: u64,
        args: &[u64; 6],
    ) -> Result<(u64, u64), CapError> {
        // SAFETY: test-only; no overlapping access context.
        let access = unsafe { Access::new() };
        let caller = CallerTable {
            table: table_id(self.caller_table_addr),
            guard: CALLER_GUARD,
        };
        api::key_table::invoke(&access, caller, table_key, op, args)
    }
}

/// A `KeyTable` capability naming the distinct destination table object (used
/// as a source entry and as a destination-table capability): it carries the
/// destination table's guard and capacity exponent, copied verbatim by
/// derivation.
fn table_cap(addr: u64, rights: Rights, badge: u64) -> KeyEntry {
    KeyEntry::new_keytable(table_id(addr), DST_GUARD, SIZE_BITS, rights, badge)
}

fn args(a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> [u64; 6] {
    [a0, a1, a2, a3, a4, a5]
}

fn stack_pool(slot: u8, generation: u32) -> StackPoolHandle {
    StackPoolHandle::new(slot, generation)
}

// ═══════════════════════════════════════════════════════════════════
// THREAD / INVOCATION ENTRY REPRESENTATION
// ═══════════════════════════════════════════════════════════════════

#[test_case]
fn named_thread_entries_round_trip_carved_identity() {
    for id in [
        test_id(ObjectType::THREAD, 0x1000),
        CarvedId {
            kind: ObjectType::THREAD,
            address: u64::MAX & !0x3F,
            generation: u32::MAX,
        },
    ] {
        let entry = KeyEntry::new_thread(id, Rights::all(), 0xBEEF);
        assert!(entry.is_valid());
        assert_eq!(entry.object_type(), ObjectType::THREAD);
        assert!(matches!(
            entry.thread_selector(),
            Ok(ThreadSelector::Named(found)) if found == id
        ));
        assert!(matches!(entry.thread_id(), Ok(found) if found == id));
        // A Thread is not a pool object.
        assert!(matches!(
            entry.object_id(),
            Err(CapError::TypeMismatch { .. })
        ));
        assert!(!entry.is_thread_return_key());
        assert_eq!(entry.rights(), Rights::all());
        assert_eq!(entry.badge(), 0xBEEF);
        let derived = entry.derive(Rights(Rights::RETIRE));
        assert!(matches!(
            derived.thread_selector(),
            Ok(ThreadSelector::Named(found)) if found == id
        ));
        assert!(matches!(derived.thread_id(), Ok(found) if found == id));
        assert_eq!(derived.rights(), Rights(Rights::RETIRE));
        assert_eq!(derived.badge(), 0xBEEF);
    }
}

#[test_case]
fn thread_return_entry_has_only_current_relative_selector() {
    let entry = KeyEntry::new_thread_return();
    assert!(entry.is_valid());
    assert_eq!(entry.object_type(), ObjectType::THREAD);
    assert!(matches!(
        entry.thread_selector(),
        Ok(ThreadSelector::CurrentReturnOnly)
    ));
    assert!(entry.is_thread_return_key());
    assert_eq!(entry.rights(), Rights::empty());
    assert_eq!(entry.badge(), 0);
    assert!(matches!(entry.thread_id(), Err(CapError::InvalidOperation)));
    assert!(matches!(
        entry.object_id(),
        Err(CapError::TypeMismatch { .. })
    ));
    assert!(matches!(
        entry.invocation_target(),
        Err(CapError::TypeMismatch { expected, found })
            if expected == ObjectType::INVOCATION && found == ObjectType::THREAD
    ));
    assert!(!entry.is_region());
    assert!(!entry.is_carved());
}

#[test_case]
fn thread_return_derivation_preserves_selector_without_authorizing_rights() {
    let entry = KeyEntry::new_thread_return();
    // derive is only a representation transform. The all-rights case is a
    // test-only alteration, not permission to amplify or distribute authority.
    for rights in [Rights::empty(), Rights::all()] {
        let derived = entry.derive(rights);
        assert_eq!(derived.object_type(), ObjectType::THREAD);
        assert!(matches!(
            derived.thread_selector(),
            Ok(ThreadSelector::CurrentReturnOnly)
        ));
        assert!(derived.is_thread_return_key());
        assert_eq!(derived.rights(), rights);
        assert_eq!(derived.badge(), 0);
        assert!(matches!(
            derived.thread_id(),
            Err(CapError::InvalidOperation)
        ));
        assert!(matches!(
            derived.object_id(),
            Err(CapError::TypeMismatch { .. })
        ));
        assert!(matches!(
            derived.invocation_target(),
            Err(CapError::TypeMismatch { expected, found })
                if expected == ObjectType::INVOCATION && found == ObjectType::THREAD
        ));
    }
    assert_eq!(entry.rights(), Rights::empty());
    assert_eq!(entry.badge(), 0);
    assert!(entry.is_thread_return_key());
}

#[test_case]
fn invocation_entries_require_and_preserve_nonzero_target() {
    let address_space = CarvedId {
        kind: ObjectType::ADDRESS_SPACE,
        address: u64::MAX & !0xF,
        generation: u32::MAX,
    };
    // These types pin both public signatures to a mandatory nonzero address.
    let constructor: fn(
        AddressSpaceId,
        NonZero<u64>,
        StackPoolHandle,
        PlacementWindow,
    ) -> KeyEntry = KeyEntry::new_invocation;
    let target_getter: fn(&KeyEntry) -> Result<(AddressSpaceId, NonZero<u64>), CapError> =
        KeyEntry::invocation_target;
    let pool_getter: fn(&KeyEntry) -> Result<StackPoolHandle, CapError> =
        KeyEntry::invocation_stack_pool;
    let window_getter: fn(&KeyEntry) -> Result<PlacementWindow, CapError> =
        KeyEntry::invocation_placement_window;
    for pool in [
        stack_pool(0, 0),
        stack_pool(3, 1),
        stack_pool(15, 0xFFFF_FFFF),
    ] {
        for address in [1, 0x1234_5678_9ABC_DEF0, u64::MAX] {
            let function = NonZero::new(address).unwrap();
            let window = if address == 1 {
                PlacementWindow::NONE
            } else {
                PlacementWindow::new(40, 3)
            };
            let entry = constructor(address_space, function, pool, window);
            assert!(matches!(window_getter(&entry), Ok(found) if found == window));
            assert!(!entry.is_lent());
            let target = target_getter(&entry)
                .unwrap_or_else(|error| panic!("Invocation target: {:?}", error.code()));
            assert_eq!(target, (address_space, function));
            assert_eq!(entry.object_type(), ObjectType::INVOCATION);
            assert_eq!(entry.rights(), Rights(Rights::CALL | Rights::BADGE));
            assert_eq!(entry.badge(), 0);
            assert!(!entry.is_thread_return_key());
            assert!(entry.object_id().is_err());
            assert_eq!(
                pool_getter(&entry)
                    .unwrap_or_else(|error| panic!("stored stack pool: {:?}", error.code())),
                pool
            );
            // derive is a representation helper, not permission to distribute
            // Invocation authority through the management allowlist.
            for rights in [Rights::empty(), Rights(Rights::CALL)] {
                let derived = entry.derive(rights);
                assert!(matches!(derived.invocation_target(), Ok(found) if found == target));
                assert!(matches!(window_getter(&derived), Ok(found) if found == window));
                let stored = derived
                    .invocation_stack_pool()
                    .unwrap_or_else(|error| panic!("derived stack pool: {:?}", error.code()));
                assert_eq!(stored, pool);
                assert_eq!(derived.rights(), rights);
                assert_eq!(derived.badge(), 0);
            }
            assert_eq!(entry.rights(), Rights(Rights::CALL | Rights::BADGE));
        }
    }
}

#[test_case]
fn thread_selector_rejects_non_thread_payloads() {
    let address_space = test_id(ObjectType::ADDRESS_SPACE, 0x3000);
    for entry in [
        KeyEntry::null(),
        KeyEntry::new_untyped(0x1000, 12, false, Rights::all()),
        KeyEntry::new_frame(0x2000, 12, false, Rights::all()),
        KeyEntry::new_keytable(
            test_id(ObjectType::KEY_TABLE, TEST_BACKING),
            CALLER_GUARD,
            SIZE_BITS,
            Rights::all(),
            0,
        ),
        KeyEntry::new_carved(address_space, Rights::all(), 0),
        KeyEntry::new_invocation(
            address_space,
            NonZero::new(0x80000).unwrap(),
            stack_pool(1, 1),
            PlacementWindow::NONE,
        ),
    ] {
        assert!(matches!(
            entry.thread_selector(),
            Err(CapError::TypeMismatch { expected, found })
                if expected == ObjectType::THREAD && found == entry.object_type()
        ));
        assert!(!entry.is_thread_return_key());
    }
}

#[test_case]
fn identity_entry_constructor_rejects_dedicated_payload_kinds() {
    for kind in [
        ObjectType::NULL,
        ObjectType::UNTYPED,
        ObjectType::FRAME,
        ObjectType::KEY_TABLE,
        ObjectType::INVOCATION,
        ObjectType::THREAD,
        ObjectType::NOTIFICATION,
        ObjectType::EVENT_COUNT,
        ObjectType::TIME,
        ObjectType::SCHEDULER,
        ObjectType::PAGE_TABLE,
        ObjectType::ADDRESS_SPACE,
    ] {
        for id in [
            ObjectId {
                pool: PoolTag::Region,
                index: 0,
                generation: 0,
            },
            ObjectId {
                pool: PoolTag::ASIDPool,
                index: u16::MAX,
                generation: u32::MAX,
            },
        ] {
            for rights in [Rights::empty(), Rights::all()] {
                for badge in [0, 0xBEEF] {
                    // Inspect only the constructor result: never read a dedicated
                    // union payload that an identity constructor must not create.
                    assert!(matches!(
                        KeyEntry::from_id(kind, id, rights, badge),
                        Err(CapError::InvalidObjectType(found)) if found == kind
                    ));
                }
            }
        }
    }
}

#[test_case]
fn an_invocation_entry_stores_the_pool_handle_without_loss() {
    for (slot, generation) in [(0_u8, 0_u32), (15, 1), (255, u32::MAX)] {
        let pool = stack_pool(slot, generation);
        let entry = KeyEntry::new_invocation(
            test_id(ObjectType::ADDRESS_SPACE, 0x2000),
            NonZero::new(0x4_0000).unwrap_or_else(|| panic!("nonzero")),
            pool,
            PlacementWindow::NONE,
        );
        let stored = entry
            .invocation_stack_pool()
            .unwrap_or_else(|_| panic!("stored pool"));
        assert_eq!(stored, pool);
        assert_eq!(stored.slot(), slot);
        assert_eq!(stored.generation(), generation);
    }
}

#[test_case]
fn thread_return_representation_preserves_entry_and_payload_layout() {
    assert_eq!(size_of::<ThreadSelector>(), 24);
    assert_eq!(align_of::<ThreadSelector>(), 8);
    assert_eq!(size_of::<InvocationPayload>(), 48);
    assert_eq!(align_of::<InvocationPayload>(), 8);
    assert_eq!(offset_of!(InvocationPayload, function_address), 0);
    assert_eq!(offset_of!(InvocationPayload, address_space_address), 8);
    assert_eq!(offset_of!(InvocationPayload, pool_generation), 16);
    assert_eq!(offset_of!(InvocationPayload, address_space_generation), 20);
    assert_eq!(offset_of!(InvocationPayload, placement_first), 24);
    assert_eq!(offset_of!(InvocationPayload, placement_count), 28);
    assert_eq!(offset_of!(InvocationPayload, pool_slot), 32);
    assert_eq!(size_of::<FramePayload>(), 32);
    assert_eq!(size_of::<KeyEntry>(), 64);
    assert_eq!(align_of::<KeyEntry>(), 32);
    assert_eq!(size_of::<[KeyEntry; 2]>(), 128);
    let entries = [KeyEntry::null(); 2];
    assert_eq!(
        core::ptr::from_ref(&entries[1]) as usize - core::ptr::from_ref(&entries[0]) as usize,
        64
    );
    // The compiled production module also asserts its private KeyPayload union
    // is 48 bytes, with the badge at entry offset 8 and the payload at 16; do not
    // substitute a test-only mirror of that union.
}

#[test_case]
fn fixture_table_carves_use_literal_non_overlapping_grown_entry_stride() {
    let mut fx = Fixture::new(FULL);
    assert_eq!(KeyTable::carve_size(SIZE_BITS), 17_472);
    assert_eq!(fx.caller_table_addr, TEST_BACKING);
    assert_eq!(fx.dst_table_addr, TEST_BACKING + 17_472);
    assert_eq!(fx.caller_table_addr % 32, 0);
    assert_eq!(fx.dst_table_addr % 32, 0);
    let first = fx.install(
        0,
        KeySlot(255),
        table_cap(fx.dst_table_addr, Rights::all(), 0xBEEF),
    );
    let second = fx.install(
        1,
        KeySlot(0),
        KeyEntry::new_keytable(
            table_id(fx.caller_table_addr),
            CALLER_GUARD,
            SIZE_BITS,
            Rights::all(),
            0xCAFE,
        ),
    );
    assert_eq!(first.incarnation(), 1);
    assert_eq!(second.incarnation(), 1);
    assert_eq!(fx.caller_len(), 2);
    assert_eq!(
        fx.lookup(0, first)
            .unwrap_or_else(|error| panic!("first table boundary: {:?}", error.code()))
            .badge(),
        0xBEEF
    );
    assert_eq!(
        fx.lookup(1, second)
            .unwrap_or_else(|error| panic!("second table boundary: {:?}", error.code()))
            .badge(),
        0xCAFE
    );
    for table_index in [0, 1] {
        for index in 0..256 {
            if (table_index == 0 && (index == KeySlot::SELF_KEYTABLE.0 || index == 255))
                || (table_index == 1 && index == 0)
            {
                continue;
            }
            let probe = RawKey::from_parts(fx.table_guard(table_index), SIZE_BITS, index, 1);
            assert!(matches!(
                fx.lookup(table_index, probe),
                Err(CapError::InvalidKey {
                    key,
                    reason: libobject::InvalidKeyReason::NeverIssued,
                    operand: 0,
                }) if key == probe
            ));
        }
    }
    let self_entry = fx
        .lookup(0, fx.self_table_key)
        .unwrap_or_else(|error| panic!("self-table entry preserved: {:?}", error.code()));
    assert_eq!(
        self_entry
            .keytable_id()
            .unwrap_or_else(|error| panic!("self-table identity preserved: {:?}", error.code())),
        table_id(fx.caller_table_addr)
    );
    assert_eq!(
        self_entry
            .keytable_guard_and_size()
            .unwrap_or_else(|error| panic!("self-table guard preserved: {:?}", error.code())),
        (CALLER_GUARD, SIZE_BITS)
    );
}

#[test_case]
fn invocation_getters_reject_every_non_invocation_payload() {
    for entry in [
        KeyEntry::null(),
        KeyEntry::new_untyped(0x1000, 12, false, Rights::all()),
        KeyEntry::new_frame(0x2000, 12, false, Rights::all()),
        KeyEntry::new_keytable(
            test_id(ObjectType::KEY_TABLE, TEST_BACKING),
            CALLER_GUARD,
            SIZE_BITS,
            Rights::all(),
            0,
        ),
        KeyEntry::new_thread_return(),
        KeyEntry::new_thread(test_id(ObjectType::THREAD, 0x1000), Rights::all(), 0xBEEF),
        KeyEntry::new_carved(test_id(ObjectType::ADDRESS_SPACE, 0x3000), Rights::all(), 0),
    ] {
        assert!(matches!(
            entry.invocation_target(),
            Err(CapError::TypeMismatch { expected, found })
                if expected == ObjectType::INVOCATION && found == entry.object_type()
        ));
        assert!(matches!(
            entry.invocation_stack_pool(),
            Err(CapError::TypeMismatch { expected, found })
                if expected == ObjectType::INVOCATION && found == entry.object_type()
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════
// COPYDERIVE
// ═══════════════════════════════════════════════════════════════════

#[test_case]
fn copy_derive_attenuates_rights_and_preserves_badge() {
    let mut fx = Fixture::new(FULL);
    let src = fx.install(
        0,
        KeySlot(10),
        table_cap(fx.dst_table_addr, Rights::all(), 0xBEEF),
    );

    let result = fx.invoke(
        fx.self_table_key,
        0, // CopyDerive
        &args(
            src.to_wire(),
            fx.self_table_key.to_wire(),
            u64::from(KeySlot(20).0),
            u64::from(Rights::READ),
            0,
            0,
        ),
    );
    let (key_wire, zero) = result.unwrap_or_else(|_| panic!("copy_derive failed"));
    assert_eq!(zero, 0);
    let dst_key = RawKey::from_wire(key_wire);
    assert_eq!(dst_key.slot(), caller_slot(20));
    assert_eq!(dst_key.incarnation(), 1);

    let derived = fx
        .lookup(0, dst_key)
        .unwrap_or_else(|_| panic!("derived entry missing"));
    assert_eq!(derived.object_type(), ObjectType::KEY_TABLE);
    assert_eq!(derived.rights(), Rights(Rights::READ));
    assert_eq!(derived.badge(), 0xBEEF);

    // Source is unchanged.
    let source = fx
        .lookup(0, src)
        .unwrap_or_else(|_| panic!("source entry lost"));
    assert_eq!(source.rights(), Rights::all());
    assert_eq!(source.badge(), 0xBEEF);
}

// ═══════════════════════════════════════════════════════════════════
// BADGES ON DERIVE
// ═══════════════════════════════════════════════════════════════════

fn notification_cap(rights: u8, badge: u64) -> KeyEntry {
    KeyEntry::new_carved(
        test_id(ObjectType::NOTIFICATION, 0x1000),
        Rights(rights),
        badge,
    )
}

/// `CopyDerive` of `source` into slot 20 of the caller's table with `rights`
/// and the requested `badge` (`x6`).
fn derive_with_badge(
    fx: &mut Fixture,
    source: RawKey,
    rights: u8,
    badge: u64,
) -> Result<(u64, u64), CapError> {
    fx.invoke(
        fx.self_table_key,
        0,
        &args(
            source.to_wire(),
            fx.self_table_key.to_wire(),
            20,
            u64::from(rights),
            badge,
            0,
        ),
    )
}

#[test_case]
fn copy_derive_sets_a_badge_once_and_every_copy_keeps_it() {
    let mut fx = Fixture::new(FULL);
    let source = fx.install(0, KeySlot(10), notification_cap(Rights::all().bits(), 0));
    let rights = Rights::RECV | Rights::SEND | Rights::BADGE;

    let Ok((key, 0)) = derive_with_badge(&mut fx, source, rights, 0b101) else {
        panic!("a badge is set on an unbadged key with BADGE");
    };
    let badged = RawKey::from_wire(key);
    let entry = fx
        .lookup(0, badged)
        .unwrap_or_else(|_| panic!("badged entry"));
    assert_eq!(entry.badge(), 0b101);
    assert_eq!(entry.rights(), Rights(rights));
    assert_eq!(entry.object_type(), ObjectType::NOTIFICATION);
    // The source stays unbadged and can mint another badge.
    let source_entry = fx.lookup(0, source).unwrap_or_else(|_| panic!("source"));
    assert_eq!(source_entry.badge(), 0);

    // A copy without a badge request keeps the badge.
    let call = args(
        badged.to_wire(),
        fx.self_table_key.to_wire(),
        21,
        u64::from(Rights::RECV),
        0,
        0,
    );
    let Ok((copy, 0)) = fx.invoke(fx.self_table_key, 0, &call) else {
        panic!("a plain copy of a badged key");
    };
    let copy = fx
        .lookup(0, RawKey::from_wire(copy))
        .unwrap_or_else(|_| panic!("copy"));
    assert_eq!(copy.badge(), 0b101);

    // Setting a badge on a badged key fails, even with BADGE and the same value.
    assert!(matches!(
        derive_with_badge(&mut fx, badged, rights, 0b101),
        Err(CapError::AlreadyBadged)
    ));
    assert!(matches!(
        derive_with_badge(&mut fx, badged, rights, 7),
        Err(CapError::AlreadyBadged)
    ));
    assert_eq!(fx.caller_len(), 4, "a refused derive installed nothing");
}

#[test_case]
fn setting_a_badge_needs_the_badge_right_and_a_badgeable_kind() {
    let mut fx = Fixture::new(FULL);
    // No BADGE on the source.
    let plain = fx.install(
        0,
        KeySlot(10),
        notification_cap(Rights::all().bits() & !Rights::BADGE, 0),
    );
    assert!(matches!(
        derive_with_badge(&mut fx, plain, Rights::RECV, 9),
        Err(CapError::InsufficientRights)
    ));
    // A key without BADGE still derives when it asks for no badge.
    assert!(derive_with_badge(&mut fx, plain, Rights::RECV, 0).is_ok());

    // A KeyTable carries BADGE here but is not a kind that can be badged.
    let table = fx.install(
        0,
        KeySlot(11),
        table_cap(fx.dst_table_addr, Rights(Rights::all().bits()), 0),
    );
    assert!(matches!(
        derive_with_badge(&mut fx, table, Rights::DERIVE, 9),
        Err(CapError::InvalidObjectType(ObjectType::KEY_TABLE))
    ));
    // The reserved argument word stays zero.
    let source = fx.install(0, KeySlot(12), notification_cap(Rights::all().bits(), 0));
    let call = args(
        source.to_wire(),
        fx.self_table_key.to_wire(),
        22,
        u64::from(Rights::RECV),
        1,
        1,
    );
    assert!(matches!(
        fx.invoke(fx.self_table_key, 0, &call),
        Err(CapError::InvalidOperation)
    ));
}

#[test_case]
fn an_invocation_derives_with_narrower_rights_and_a_badge_keeping_its_target() {
    let mut fx = Fixture::new(FULL);
    let address_space = test_id(ObjectType::ADDRESS_SPACE, 0x2000);
    let pool = stack_pool(2, 7);
    let mut original = KeyEntry::new_invocation(
        address_space,
        NonZero::new(0x4_0000).unwrap_or_else(|| panic!("nonzero")),
        pool,
        PlacementWindow::NONE,
    );
    original = original.derive(Rights(Rights::CALL | Rights::BADGE));
    let source = fx.install(0, KeySlot(10), original);

    let Ok((key, 0)) = derive_with_badge(&mut fx, source, Rights::CALL, u64::MAX) else {
        panic!("an Invocation is badged on derive");
    };
    let derived = fx
        .lookup(0, RawKey::from_wire(key))
        .unwrap_or_else(|_| panic!("derived Invocation"));
    assert_eq!(derived.badge(), u64::MAX);
    assert_eq!(derived.rights(), Rights(Rights::CALL));
    assert_eq!(derived.object_type(), ObjectType::INVOCATION);
    let (derived_space, derived_entry) = derived
        .invocation_target()
        .unwrap_or_else(|_| panic!("derived target"));
    let (space, entry) = original
        .invocation_target()
        .unwrap_or_else(|_| panic!("target"));
    assert_eq!((derived_space, derived_entry), (space, entry));
    assert_eq!(
        derived
            .invocation_stack_pool()
            .unwrap_or_else(|_| panic!("derived pool")),
        pool
    );
    // Rights only narrow: asking for more than the source holds fails.
    assert!(matches!(
        derive_with_badge(&mut fx, source, Rights::CALL | Rights::GRANT, 1),
        Err(CapError::InsufficientRights)
    ));
}

#[test_case]
fn copy_derive_still_rejects_kinds_outside_the_allow_list() {
    let mut fx = Fixture::new(FULL);
    let time = fx.install(
        0,
        KeySlot(10),
        KeyEntry::new_carved(test_id(ObjectType::TIME, 0x3000), Rights::all(), 0),
    );
    assert!(matches!(
        derive_with_badge(&mut fx, time, Rights::READ, 0),
        Err(CapError::InvalidObjectType(ObjectType::TIME))
    ));
    assert!(matches!(
        derive_with_badge(&mut fx, time, Rights::READ, 5),
        Err(CapError::InvalidObjectType(ObjectType::TIME))
    ));
}

/// Retiring a table bumps its generation, so every capability naming it fails
/// with `InconsistentKey`/`ObjectRetired` on the key that named it, whichever
/// table holds the capability, without walking any derivation tree. Other
/// tables keep working, and a rejected operation changes nothing.
#[test_case]
fn retired_table_rejects_capabilities_in_every_table() {
    let mut fx = Fixture::new(FULL);
    let to_dst = fx.install(
        0,
        KeySlot(10),
        table_cap(fx.dst_table_addr, Rights::all(), 0),
    );
    // A second capability to the destination table, held in that table.
    let in_dst = fx.install(
        1,
        KeySlot(3),
        table_cap(fx.dst_table_addr, Rights::all(), 0),
    );
    let movable = fx.install(
        0,
        KeySlot(11),
        KeyEntry::new_frame(0x4000, 12, false, Rights::all()),
    );

    // SAFETY: the fixture table is a live carve; no guard to it is held.
    unsafe { Carved::<KeyTable>::retire(table_id(fx.dst_table_addr)) }
        .unwrap_or_else(|_| panic!("destination table retirement failed"));

    // Both capabilities carry the old generation and fail on their own.
    for (index, key) in [(0, to_dst), (1, in_dst)] {
        let entry = *fx
            .lookup(index, key)
            .unwrap_or_else(|_| panic!("capability entry missing"));
        let named = entry
            .keytable_id()
            .unwrap_or_else(|_| panic!("not a table capability"));
        // SAFETY: the address names the fixture carve.
        assert_eq!(
            unsafe { Carved::<KeyTable>::validate(named) }.err(),
            Some(objects::object_pool::IdentityError::Retired)
        );
    }

    let retired = |key: RawKey, operand: u8| CapError::InconsistentKey {
        key,
        reason: InconsistencyReason::ObjectRetired,
        operand,
    };
    // The retired table as the invoked table (x0).
    let result = fx.invoke(to_dst, 2, &args(in_dst.to_wire(), 0, 0, 0, 0, 0));
    assert_eq!(
        result.err().map(|error| error.code()),
        Some(retired(to_dst, 0).code())
    );
    // The retired table as the destination table (x3): the source stays put.
    let result = fx.invoke(
        fx.self_table_key,
        1,
        &args(movable.to_wire(), to_dst.to_wire(), 20, 0, 0, 0),
    );
    assert_eq!(
        result.err().map(|error| error.code()),
        Some(retired(to_dst, 3).code())
    );
    assert_eq!(fx.caller_len(), 3);
    assert!(fx.lookup(0, movable).is_ok());

    // The live caller table still serves its own capabilities.
    let result = fx.invoke(
        fx.self_table_key,
        2,
        &args(movable.to_wire(), 0, 0, 0, 0, 0),
    );
    assert!(result.is_ok());
    assert_eq!(fx.caller_len(), 2);
}

#[test_case]
fn copy_derive_rejects_rights_amplification() {
    let mut fx = Fixture::new(FULL);
    let src = fx.install(
        0,
        KeySlot(10),
        table_cap(fx.dst_table_addr, Rights(Rights::READ), 0),
    );
    let result = fx.invoke(
        fx.self_table_key,
        0,
        &args(
            src.to_wire(),
            fx.self_table_key.to_wire(),
            20,
            u64::from(Rights::READ | Rights::WRITE),
            0,
            0,
        ),
    );
    assert!(matches!(result, Err(CapError::InsufficientRights)));
    // Failed operation leaves the destination vacant and source unchanged.
    assert_eq!(fx.caller_len(), 2); // self-table + source
}

/// Requested-rights words wider than the eight defined bits are malformed input, rejected before
/// authority checks rather than reported as an amplification attempt.
#[test_case]
fn copy_derive_rejects_rights_words_wider_than_the_defined_bits() {
    let mut fx = Fixture::new(FULL);
    let src = fx.install(
        0,
        KeySlot(10),
        table_cap(fx.dst_table_addr, Rights::all(), 0),
    );
    for rights_word in [1_u64 << 8, 0x1FF, 1 << 63, u64::MAX] {
        let result = fx.invoke(
            fx.self_table_key,
            0,
            &args(
                src.to_wire(),
                fx.self_table_key.to_wire(),
                20,
                rights_word,
                0,
                0,
            ),
        );
        assert!(
            matches!(result, Err(CapError::InvalidOperation)),
            "rights word {rights_word:#x} must be rejected"
        );
    }
    assert_eq!(fx.caller_len(), 2); // self-table + source
}

#[test_case]
fn copy_derive_requires_derive_on_source_and_install_on_destination() {
    // Source table without DERIVE.
    let mut fx = Fixture::new(Rights::INSTALL);
    let src = fx.install(
        0,
        KeySlot(10),
        table_cap(fx.dst_table_addr, Rights::all(), 0),
    );
    let call = args(src.to_wire(), fx.self_table_key.to_wire(), 20, 1, 0, 0);
    assert!(matches!(
        fx.invoke(fx.self_table_key, 0, &call),
        Err(CapError::InsufficientRights)
    ));

    // Destination table without INSTALL: source has DERIVE but not INSTALL.
    let mut fx = Fixture::new(Rights::DERIVE);
    let src = fx.install(
        0,
        KeySlot(10),
        table_cap(fx.dst_table_addr, Rights::all(), 0),
    );
    let call = args(src.to_wire(), fx.self_table_key.to_wire(), 20, 1, 0, 0);
    assert!(matches!(
        fx.invoke(fx.self_table_key, 0, &call),
        Err(CapError::InsufficientRights)
    ));
}

#[test_case]
fn copy_derive_rejects_occupied_destination_and_non_allowlisted_kinds() {
    let mut fx = Fixture::new(FULL);
    let src = fx.install(
        0,
        KeySlot(10),
        table_cap(fx.dst_table_addr, Rights::all(), 0),
    );
    let _occupant = fx.install(
        0,
        KeySlot(20),
        table_cap(fx.dst_table_addr, Rights::all(), 0),
    );
    let call = args(src.to_wire(), fx.self_table_key.to_wire(), 20, 1, 0, 0);
    assert!(matches!(
        fx.invoke(fx.self_table_key, 0, &call),
        Err(CapError::SlotOccupied(KeySlot(20)))
    ));

    // A non-allowlisted kind (Untyped) cannot be derived: one region must not
    // gain independent allocation watermarks.
    let mut fx = Fixture::new(FULL);
    let untyped = fx.install(
        0,
        KeySlot(11),
        KeyEntry::new_untyped(0x1000, 12, false, Rights::all()),
    );
    let call = args(untyped.to_wire(), fx.self_table_key.to_wire(), 21, 1, 0, 0);
    assert!(matches!(
        fx.invoke(fx.self_table_key, 0, &call),
        Err(CapError::InvalidObjectType(ObjectType::UNTYPED))
    ));

    // Frame is allowlisted for capability-only derivation: a
    // mapped frame derives into an unmapped capability with no mapping
    // association, while the original keeps its mapping.
    let mut fx = Fixture::new(FULL);
    let mut mapped_frame = KeyEntry::new_frame(0x1000, 12, false, Rights::all());
    {
        let frame = mapped_frame
            .as_frame_mut()
            .unwrap_or_else(|_| panic!("not a frame"));
        frame.set_mapped(test_id(ObjectType::ADDRESS_SPACE, 0x1000), 0x1000_0000);
    }
    let frame = fx.install(0, KeySlot(12), mapped_frame);
    let call = args(frame.to_wire(), fx.self_table_key.to_wire(), 22, 1, 0, 0);
    let (derived_wire, _) = fx
        .invoke(fx.self_table_key, 0, &call)
        .unwrap_or_else(|_| panic!("frame CopyDerive failed"));
    let derived = RawKey::from_wire(derived_wire);
    let derived_entry = fx
        .lookup(0, derived)
        .unwrap_or_else(|_| panic!("derived frame entry missing"));
    let derived_frame = derived_entry
        .as_frame()
        .unwrap_or_else(|_| panic!("derived entry is not a Frame cap"));
    assert!(
        !derived_frame.is_mapped(),
        "a derived frame starts unmapped"
    );
    let original_entry = fx
        .lookup(0, frame)
        .unwrap_or_else(|_| panic!("original frame entry missing"));
    assert!(
        original_entry
            .as_frame()
            .unwrap_or_else(|_| panic!("original entry is not a Frame cap"))
            .is_mapped(),
        "the original frame keeps its mapping"
    );
}

// ═══════════════════════════════════════════════════════════════════
// MOVE
// ═══════════════════════════════════════════════════════════════════

#[test_case]
fn thread_entries_remain_outside_copy_derive_and_move_allowlist() {
    let thread_id = test_id(ObjectType::THREAD, 0x1000);
    for entry in [
        KeyEntry::new_thread(thread_id, Rights::all(), 0xBEEF),
        KeyEntry::new_thread_return(),
        KeyEntry::new_thread_return().derive(Rights::all()),
    ] {
        let mut fx = Fixture::new(FULL);
        let src = fx.install(0, KeySlot(10), entry);
        let dst_cap = fx.install(
            0,
            KeySlot(5),
            table_cap(fx.dst_table_addr, Rights::all(), 0),
        );
        for (table_index, table_key) in [(0, fx.self_table_key), (1, dst_cap)] {
            for op in [0, 1] {
                // Empty requested rights isolate the kind allowlist from the
                // independent attenuation check, including the rights-empty sentinel.
                let call = args(src.to_wire(), table_key.to_wire(), 20, 0, 0, 0);
                assert!(matches!(
                    fx.invoke(fx.self_table_key, op, &call),
                    Err(CapError::InvalidObjectType(found)) if found == entry.object_type()
                ));
                assert_eq!(fx.caller_len(), 3);
                let source = fx
                    .lookup(0, src)
                    .unwrap_or_else(|_| panic!("source changed"));
                assert_eq!(source.object_type(), entry.object_type());
                assert_eq!(source.rights(), entry.rights());
                assert_eq!(source.badge(), entry.badge());
                if entry.object_type() == ObjectType::THREAD {
                    assert_eq!(
                        source.thread_selector().unwrap_or_else(|error| {
                            panic!("source Thread selector: {:?}", error.code())
                        }),
                        entry.thread_selector().unwrap_or_else(|error| {
                            panic!("original Thread selector: {:?}", error.code())
                        })
                    );
                } else {
                    assert_eq!(
                        source.invocation_target().unwrap_or_else(|error| {
                            panic!("source Invocation target: {:?}", error.code())
                        }),
                        entry.invocation_target().unwrap_or_else(|error| {
                            panic!("original Invocation target: {:?}", error.code())
                        })
                    );
                    assert_eq!(
                        source.invocation_stack_pool().unwrap_or_else(|error| {
                            panic!("source Invocation pool: {:?}", error.code())
                        }),
                        entry.invocation_stack_pool().unwrap_or_else(|error| {
                            panic!("original Invocation pool: {:?}", error.code())
                        })
                    );
                }
                let probe = RawKey::from_parts(fx.table_guard(table_index), SIZE_BITS, 20, 1);
                assert!(matches!(
                    fx.lookup(table_index, probe),
                    Err(CapError::InvalidKey {
                        key,
                        reason: libobject::InvalidKeyReason::NeverIssued,
                        operand: 0,
                    }) if key == probe
                ));
                assert_eq!(
                    fx.lookup(0, fx.self_table_key)
                        .unwrap_or_else(|error| panic!(
                            "source table capability: {:?}",
                            error.code()
                        ))
                        .rights(),
                    Rights(FULL)
                );
                assert_eq!(
                    fx.lookup(0, dst_cap)
                        .unwrap_or_else(|error| {
                            panic!("destination table capability: {:?}", error.code())
                        })
                        .rights(),
                    Rights::all()
                );
            }
        }
    }
}

/// `Invocation` and `Notification` entries can be derived (and badged) but not
/// moved: the `Move` allow-list is unchanged.
#[test_case]
fn invocation_and_notification_entries_derive_but_do_not_move() {
    let address_space = test_id(ObjectType::ADDRESS_SPACE, 0x2000);
    for entry in [
        KeyEntry::new_invocation(
            address_space,
            NonZero::new(0x80000).unwrap(),
            stack_pool(1, 1),
            PlacementWindow::NONE,
        ),
        notification_cap(Rights::all().bits(), 0),
    ] {
        let mut fx = Fixture::new(FULL);
        let src = fx.install(0, KeySlot(10), entry);
        let call = args(src.to_wire(), fx.self_table_key.to_wire(), 20, 0, 0, 0);
        assert!(matches!(
            fx.invoke(fx.self_table_key, 1, &call),
            Err(CapError::InvalidObjectType(found)) if found == entry.object_type()
        ));
        assert!(
            fx.invoke(fx.self_table_key, 0, &call).is_ok(),
            "CopyDerive accepts the kind, here with empty rights"
        );
    }
}

#[test_case]
fn move_preserves_state_and_invalidates_source() {
    let mut fx = Fixture::new(FULL);
    let src = fx.install(
        0,
        KeySlot(10),
        table_cap(fx.dst_table_addr, Rights(Rights::READ), 0x1234),
    );

    let result = fx.invoke(
        fx.self_table_key,
        1, // Move
        &args(
            src.to_wire(),
            fx.self_table_key.to_wire(),
            u64::from(KeySlot(20).0),
            0,
            0,
            0,
        ),
    );
    let (key_wire, zero) = result.unwrap_or_else(|_| panic!("move failed"));
    assert_eq!(zero, 0);
    let dst_key = RawKey::from_wire(key_wire);
    assert_eq!(dst_key.slot(), caller_slot(20));

    let moved = fx
        .lookup(0, dst_key)
        .unwrap_or_else(|_| panic!("moved entry missing"));
    assert_eq!(moved.rights(), Rights(Rights::READ));
    assert_eq!(moved.badge(), 0x1234);

    // Source key is invalidated (capability invalidated, not just vacant).
    assert!(matches!(
        fx.lookup(0, src),
        Err(CapError::InconsistentKey { .. })
    ));
}

#[test_case]
fn move_rejects_same_slot_and_requires_derive_plus_remove() {
    let mut fx = Fixture::new(FULL);
    let src = fx.install(
        0,
        KeySlot(10),
        table_cap(fx.dst_table_addr, Rights::all(), 0),
    );
    // Same table, same slot: rejected, not a no-op.
    let call = args(
        src.to_wire(),
        fx.self_table_key.to_wire(),
        u64::from(KeySlot(10).0),
        0,
        0,
        0,
    );
    assert!(matches!(
        fx.invoke(fx.self_table_key, 1, &call),
        Err(CapError::SlotOccupied(KeySlot(10)))
    ));

    // Missing REMOVE on the source table.
    let mut fx = Fixture::new(Rights::DERIVE | Rights::INSTALL);
    let src = fx.install(
        0,
        KeySlot(10),
        table_cap(fx.dst_table_addr, Rights::all(), 0),
    );
    let call = args(src.to_wire(), fx.self_table_key.to_wire(), 20, 0, 0, 0);
    assert!(matches!(
        fx.invoke(fx.self_table_key, 1, &call),
        Err(CapError::InsufficientRights)
    ));
}

// ═══════════════════════════════════════════════════════════════════
// DELETE
// ═══════════════════════════════════════════════════════════════════

#[test_case]
fn delete_removes_entry_without_object_retirement() {
    let mut fx = Fixture::new(FULL);
    let src = fx.install(
        0,
        KeySlot(10),
        table_cap(fx.dst_table_addr, Rights::all(), 0),
    );
    let call = args(src.to_wire(), 0, 0, 0, 0, 0);
    let result = fx.invoke(fx.self_table_key, 2, &call);
    assert_eq!(result.unwrap_or_else(|_| panic!("delete failed")), (0, 0));
    assert!(matches!(
        fx.lookup(0, src),
        Err(CapError::InconsistentKey { .. })
    ));
    assert_eq!(fx.caller_len(), 1); // only the self-table cap remains
}

#[test_case]
fn delete_requires_remove_and_rejects_stale_selector() {
    // Missing REMOVE.
    let mut fx = Fixture::new(Rights::DERIVE);
    let src = fx.install(
        0,
        KeySlot(10),
        table_cap(fx.dst_table_addr, Rights::all(), 0),
    );
    let call = args(src.to_wire(), 0, 0, 0, 0, 0);
    assert!(matches!(
        fx.invoke(fx.self_table_key, 2, &call),
        Err(CapError::InsufficientRights)
    ));

    // Stale incarnation must not delete a replacement occupant.
    let mut fx = Fixture::new(FULL);
    let old = fx.install(
        0,
        KeySlot(10),
        table_cap(fx.dst_table_addr, Rights::all(), 0),
    );
    fx.remove(0, old)
        .unwrap_or_else(|_| panic!("remove failed"));
    let replacement = fx.install(
        0,
        KeySlot(10),
        table_cap(fx.dst_table_addr, Rights::all(), 0),
    );
    assert_ne!(old.incarnation(), replacement.incarnation());
    let call = args(old.to_wire(), 0, 0, 0, 0, 0);
    assert!(matches!(
        fx.invoke(fx.self_table_key, 2, &call),
        Err(CapError::InconsistentKey { .. })
    ));
    // The replacement survives the stale delete attempt.
    assert!(fx.lookup(0, replacement).is_ok());
}

// ═══════════════════════════════════════════════════════════════════
// REVOKE
// ═══════════════════════════════════════════════════════════════════

#[test_case]
fn revoke_is_rejected() {
    let mut fx = Fixture::new(FULL);
    let src = fx.install(
        0,
        KeySlot(10),
        table_cap(fx.dst_table_addr, Rights::all(), 0),
    );
    // Revoke (op 4) is not part of the approved minimal lifecycle.
    let call = args(src.to_wire(), 0, 0, 0, 0, 0);
    assert!(matches!(
        fx.invoke(fx.self_table_key, 4, &call),
        Err(CapError::InvalidOperation)
    ));
}

// ═══════════════════════════════════════════════════════════════════
// CROSS-TABLE RESOLUTION
// ═══════════════════════════════════════════════════════════════════

#[test_case]
fn copy_derive_across_distinct_tables() {
    let mut fx = Fixture::new(FULL);
    let dst_cap_key = fx.install(
        0,
        KeySlot(5),
        table_cap(fx.dst_table_addr, Rights::all(), 0),
    );
    let src = fx.install(
        0,
        KeySlot(10),
        table_cap(fx.dst_table_addr, Rights::all(), 0xBEEF),
    );

    let result = fx.invoke(
        fx.self_table_key,
        0,
        &args(
            src.to_wire(),
            dst_cap_key.to_wire(),
            20,
            u64::from(Rights::READ),
            0,
            0,
        ),
    );
    let (key_wire, zero) = result.unwrap_or_else(|_| panic!("cross-table copy_derive failed"));
    assert_eq!(zero, 0);
    let dst_key = RawKey::from_wire(key_wire);
    assert_eq!(dst_key.slot(), dst_slot(20));

    // The derived entry lands in the distinct destination table.
    let derived = fx
        .lookup(1, dst_key)
        .unwrap_or_else(|_| panic!("derived entry missing in destination table"));
    assert_eq!(derived.rights(), Rights(Rights::READ));
    assert_eq!(derived.badge(), 0xBEEF);

    // Source is unchanged in the caller table.
    let source = fx
        .lookup(0, src)
        .unwrap_or_else(|_| panic!("source entry lost"));
    assert_eq!(source.rights(), Rights::all());
}

#[test_case]
fn move_across_distinct_tables() {
    let mut fx = Fixture::new(FULL);
    let dst_cap_key = fx.install(
        0,
        KeySlot(5),
        table_cap(fx.dst_table_addr, Rights::all(), 0),
    );
    let src = fx.install(
        0,
        KeySlot(10),
        table_cap(fx.dst_table_addr, Rights(Rights::READ), 0x1234),
    );

    let result = fx.invoke(
        fx.self_table_key,
        1,
        &args(src.to_wire(), dst_cap_key.to_wire(), 20, 0, 0, 0),
    );
    let (key_wire, zero) = result.unwrap_or_else(|_| panic!("cross-table move failed"));
    assert_eq!(zero, 0);
    let dst_key = RawKey::from_wire(key_wire);
    assert_eq!(dst_key.slot(), dst_slot(20));

    let moved = fx
        .lookup(1, dst_key)
        .unwrap_or_else(|_| panic!("moved entry missing in destination table"));
    assert_eq!(moved.rights(), Rights(Rights::READ));
    assert_eq!(moved.badge(), 0x1234);

    // Source key is invalidated in the caller table.
    assert!(matches!(
        fx.lookup(0, src),
        Err(CapError::InconsistentKey { .. })
    ));
}

#[test_case]
fn cross_table_move_rolls_back_on_occupied_destination() {
    let mut fx = Fixture::new(FULL);
    let dst_cap_key = fx.install(
        0,
        KeySlot(5),
        table_cap(fx.dst_table_addr, Rights::all(), 0),
    );
    let src = fx.install(
        0,
        KeySlot(10),
        table_cap(fx.dst_table_addr, Rights::all(), 0),
    );
    let _occupant = fx.install(
        1,
        KeySlot(20),
        table_cap(fx.dst_table_addr, Rights::all(), 0),
    );

    let call = args(src.to_wire(), dst_cap_key.to_wire(), 20, 0, 0, 0);
    assert!(matches!(
        fx.invoke(fx.self_table_key, 1, &call),
        Err(CapError::SlotOccupied(KeySlot(20)))
    ));

    // The source entry is restored into the caller table with a fresh
    // incarnation; the original source key is invalidated by the failed move.
    assert!(matches!(
        fx.lookup(0, src),
        Err(CapError::InconsistentKey { .. })
    ));
    assert_eq!(fx.caller_len(), 3); // self + destination cap + restored source
    let restored = RawKey::from_parts(CALLER_GUARD, SIZE_BITS, 10, 2);
    assert!(fx.lookup(0, restored).is_ok());
}

// ═══════════════════════════════════════════════════════════════════
// GUARDED KEY SPACE
// ═══════════════════════════════════════════════════════════════════

/// A key minted into one table never resolves in another, even on an equal
/// slot and incarnation: the guard check rejects it before indexing.
#[test_case]
fn keys_do_not_cross_tables_with_distinct_guards() {
    let mut fx = Fixture::new(FULL);
    let src = fx.install(
        0,
        KeySlot(10),
        table_cap(fx.dst_table_addr, Rights::all(), 0xBEEF),
    );
    let dst_cap_key = fx.install(
        0,
        KeySlot(5),
        table_cap(fx.dst_table_addr, Rights::all(), 0),
    );

    // CopyDerive into the destination table: the returned key carries the
    // destination's guard.
    let result = fx.invoke(
        fx.self_table_key,
        0,
        &args(
            src.to_wire(),
            dst_cap_key.to_wire(),
            10,
            u64::from(Rights::READ),
            0,
            0,
        ),
    );
    let (dst_wire, _) = result.unwrap_or_else(|_| panic!("cross-table copy_derive failed"));
    let dst_key = RawKey::from_wire(dst_wire);
    assert_eq!(dst_key.slot(), dst_slot(10));

    // The same slot and incarnation in the caller table hold a different
    // entry (`src` itself): the destination-minted key does not resolve
    // there, and vice versa, despite identical index and incarnation.
    assert_eq!(src.slot(), caller_slot(10));
    assert_eq!(dst_key.incarnation(), src.incarnation());
    assert!(matches!(
        fx.lookup(0, dst_key),
        Err(CapError::InvalidKey {
            reason: libobject::InvalidKeyReason::GuardMismatch,
            ..
        })
    ));
    // And the caller-minted key does not resolve in the destination table.
    assert!(matches!(
        fx.lookup(1, src),
        Err(CapError::InvalidKey {
            reason: libobject::InvalidKeyReason::GuardMismatch,
            ..
        })
    ));
}

/// A selector with a wrong guard is rejected before any table state is
/// touched, and a management invocation presenting it fails the same way.
#[test_case]
fn management_selectors_validate_the_guard() {
    let mut fx = Fixture::new(FULL);
    let src = fx.install(
        0,
        KeySlot(10),
        table_cap(fx.dst_table_addr, Rights::all(), 0),
    );

    // A foreign-guard selector for an existing slot: GuardMismatch, not a
    // hit on the occupant.
    let foreign = RawKey::from_parts(DST_GUARD, SIZE_BITS, 10, 1);
    let call = args(foreign.to_wire(), fx.self_table_key.to_wire(), 20, 1, 0, 0);
    assert!(matches!(
        fx.invoke(fx.self_table_key, 0, &call),
        Err(CapError::InvalidKey {
            key,
            reason: libobject::InvalidKeyReason::GuardMismatch,
            ..
        }) if key == foreign
    ));
    // The occupant is untouched.
    assert!(fx.lookup(0, src).is_ok());
}

/// Snapshot of Slot(1): its entry kind/selector and the table's live count.
fn return_slot_state(table: &KeyTable) -> (Option<bool>, usize) {
    let key = RawKey::from_parts(
        CALLER_GUARD,
        SIZE_BITS,
        KeySlot::THREAD_RETURN.0,
        KeySlot::THREAD_RETURN_INCARNATION,
    );
    let sentinel = table
        .lookup(key, CALLER_GUARD)
        .ok()
        .map(KeyEntry::is_thread_return_key);
    (sentinel, table.len())
}

#[test_case]
fn address_space_provisioning_installs_the_return_sentinel_once() {
    let address = carve(0);
    // SAFETY: freshly initialized fixture carve, exclusively owned here.
    let table = unsafe { &mut *KeyTable::at_carve(address) };
    assert_eq!(return_slot_state(table), (None, 0));

    // A fresh table gets the sentinel at the deterministic first incarnation.
    // SAFETY: the fixture carve stays initialized at this address.
    let binding = unsafe { table.bind_address_space() }
        .unwrap_or_else(|error| panic!("fresh provisioning: {:?}", error.code()));
    assert_eq!(binding.address(), address);
    assert_eq!(binding.size_bits(), SIZE_BITS);
    assert_eq!(return_slot_state(table), (Some(true), 1));

    // Rebinding a table that already holds it changes nothing.
    // SAFETY: as above.
    let rebound = unsafe { table.bind_address_space() }
        .unwrap_or_else(|error| panic!("rebinding: {:?}", error.code()));
    assert_eq!(rebound, binding);
    assert_eq!(return_slot_state(table), (Some(true), 1));
}

#[test_case]
fn address_space_provisioning_rejects_a_foreign_or_reissued_slot_one() {
    let address = carve(0);
    // SAFETY: freshly initialized fixture carve, exclusively owned here.
    let table = unsafe { &mut *KeyTable::at_carve(address) };
    let foreign = table
        .insert(
            KeySlot::THREAD_RETURN,
            KeyEntry::new_keytable(table_id(address), CALLER_GUARD, SIZE_BITS, Rights::all(), 0),
            CALLER_GUARD,
        )
        .unwrap_or_else(|_| panic!("foreign Slot(1) entry"));
    // SAFETY: the fixture carve stays initialized at this address.
    let error = unsafe { table.bind_address_space() }.expect_err("foreign Slot(1) accepted");
    assert_eq!(
        error.code(),
        CapError::SlotOccupied(KeySlot::THREAD_RETURN).code()
    );
    assert_eq!(return_slot_state(table), (Some(false), 1));

    // Vacant but previously issued: the provisioned key would be stale.
    table
        .remove(foreign, CALLER_GUARD)
        .unwrap_or_else(|_| panic!("foreign Slot(1) removal"));
    // SAFETY: as above.
    let error = unsafe { table.bind_address_space() }.expect_err("reissued Slot(1) accepted");
    assert_eq!(error.code(), CapError::InvalidOperation.code());
    assert_eq!(return_slot_state(table), (None, 0));

    // A sentinel at a later incarnation is not the provisioned key either.
    table
        .insert(
            KeySlot::THREAD_RETURN,
            KeyEntry::new_thread_return(),
            CALLER_GUARD,
        )
        .unwrap_or_else(|_| panic!("late sentinel"));
    // SAFETY: as above.
    let error = unsafe { table.bind_address_space() }.expect_err("late sentinel accepted");
    assert_eq!(
        error.code(),
        CapError::SlotOccupied(KeySlot::THREAD_RETURN).code()
    );
}

/// A borrowed entry (placed by `Invocation.CallWithKey`) is never the source
/// of CopyDerive or Move, whatever its kind or rights: the callee cannot keep
/// it past the call. Delete and the borrowed entry's own use stay available.
#[test_case]
fn borrowed_entries_are_not_copy_derive_or_move_sources() {
    for entry in [
        KeyEntry::new_keytable(
            test_id(ObjectType::KEY_TABLE, 0x1000),
            DST_GUARD,
            SIZE_BITS,
            Rights::all(),
            0,
        ),
        KeyEntry::new_frame(0x1000, 12, false, Rights::all()),
    ] {
        let mut fx = Fixture::new(FULL);
        let borrowed = entry.borrowed_copy();
        assert!(borrowed.is_lent());
        let src = fx.install(0, KeySlot(10), borrowed);

        let copy = args(src.to_wire(), fx.self_table_key.to_wire(), 20, 1, 0, 0);
        assert!(matches!(
            fx.invoke(fx.self_table_key, 0, &copy),
            Err(CapError::InvalidOperation)
        ));
        let mv = args(src.to_wire(), fx.self_table_key.to_wire(), 21, 0, 0, 0);
        assert!(matches!(
            fx.invoke(fx.self_table_key, 1, &mv),
            Err(CapError::InvalidOperation)
        ));
        // Both refusals left the table as it was: the borrowed entry is intact.
        assert!(
            fx.lookup(0, src)
                .unwrap_or_else(|_| panic!("borrowed entry lost"))
                .is_lent()
        );
        assert_eq!(fx.caller_len(), 2); // self-table + borrowed entry

        // Deleting it is allowed.
        let delete = args(src.to_wire(), 0, 0, 0, 0, 0);
        assert_eq!(
            fx.invoke(fx.self_table_key, 2, &delete)
                .unwrap_or_else(|_| panic!("delete of a borrowed entry failed")),
            (0, 0)
        );
        assert_eq!(fx.caller_len(), 1);
    }
}

/// A borrowed copy keeps the object and the exact rights (including `LEND`),
/// is marked, and derive() of any entry yields an unmarked, ordinary one.
#[test_case]
fn borrowed_copy_keeps_rights_and_marks_the_entry() {
    let original = KeyEntry::new_frame(
        0x1000,
        12,
        false,
        Rights(Rights::READ | Rights::WRITE | Rights::LEND),
    );
    assert!(!original.is_lent());
    let borrowed = original.borrowed_copy();
    assert!(borrowed.is_lent());
    assert_eq!(borrowed.rights(), original.rights());
    assert_eq!(borrowed.object_type(), ObjectType::FRAME);
    // An ordinary derivation of a borrowed entry never carries the mark.
    assert!(!borrowed.derive(Rights(Rights::READ)).is_lent());
    assert!(KeyEntry::is_derivable_kind(ObjectType::KEY_TABLE));
    assert!(KeyEntry::is_derivable_kind(ObjectType::FRAME));
    assert!(!KeyEntry::is_derivable_kind(ObjectType::UNTYPED));
    assert!(!KeyEntry::is_derivable_kind(ObjectType::INVOCATION));
}

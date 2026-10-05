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
        CapError, InvalidStackReason, KeySlot, ObjectType, RawKey, Rights, domain::DomainId,
    },
    objects::{
        ArchObjects, ArchObjectsImpl, KeyTable, Thread,
        access::{Access, ObjectId, PoolTag},
        invocation::InvocationStackExtent,
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
        KeyTable::initialize(obj, DomainId(0), SIZE_BITS);
    }
    obj as u64
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
        let self_table_key = unsafe { &mut *(caller_table_addr as *mut KeyTable) }
            .insert(
                KeySlot::SELF_KEYTABLE,
                KeyEntry::new_keytable(
                    caller_table_addr,
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
        unsafe { &mut *(addr as *mut KeyTable) }
            .insert(slot, entry, guard)
            .unwrap_or_else(|_| panic!("fixture installation failed"))
    }

    /// Look up an entry in a table by fixture index.
    fn lookup(&self, index: usize, key: RawKey) -> Result<&KeyEntry, CapError> {
        let addr = self.table_addr(index);
        let guard = self.table_guard(index);
        // SAFETY: the address names a live carved table owned by the fixture.
        Ok(unsafe { &*(addr as *const KeyTable) }.lookup(key, guard)?)
    }

    /// Remove an entry from a table by fixture index.
    fn remove(&mut self, index: usize, key: RawKey) -> Result<KeyEntry, CapError> {
        let addr = self.table_addr(index);
        let guard = self.table_guard(index);
        // SAFETY: the address names a live carved table owned by the fixture.
        unsafe { &mut *(addr as *mut KeyTable) }.remove(key, guard)
    }

    /// The caller table's live-entry count.
    fn caller_len(&self) -> usize {
        // SAFETY: see lookup.
        unsafe { &*(self.caller_table_addr as *const KeyTable) }.len()
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
            addr: self.caller_table_addr,
            guard: CALLER_GUARD,
        };
        api::key_table::invoke(&access, caller, table_key, op, args)
    }
}

/// A `KeyTable` capability naming the distinct destination table object (used
/// as a source entry and as a destination-table capability): it carries the
/// destination table's guard and capacity exponent, copied verbatim by
/// derivation.
fn table_cap(addr: u64, rights: Rights, badge: u16) -> KeyEntry {
    KeyEntry::new_keytable(addr, DST_GUARD, SIZE_BITS, rights, badge)
}

fn args(a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> [u64; 6] {
    [a0, a1, a2, a3, a4, a5]
}

fn stack_extent(base: u64, end: u64, minimum_headroom: u64) -> InvocationStackExtent {
    InvocationStackExtent::new(base, end, minimum_headroom, ArchObjectsImpl::USER_VA_END)
        .unwrap_or_else(|error| panic!("fixture stack extent: {:?}", error.code()))
}

fn assert_invalid_stack<T>(
    result: Result<T, CapError>,
    value: u64,
    reason: InvalidStackReason,
    reason_id: u64,
) {
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("invalid stack input accepted"),
    };
    assert!(matches!(
        error,
        CapError::InvalidStack { value: found_value, reason: found_reason }
            if found_value == value && found_reason == reason
    ));
    // Pin the complete diagnostic words independently of enum discriminants.
    assert_eq!(error.code(), (32, value, reason_id));
}

// ═══════════════════════════════════════════════════════════════════
// THREAD / INVOCATION ENTRY REPRESENTATION
// ═══════════════════════════════════════════════════════════════════

#[test_case]
fn named_thread_entries_round_trip_checked_identity() {
    for id in [
        ObjectId {
            pool: PoolTag::Thread,
            index: 0,
            generation: 1,
        },
        ObjectId {
            pool: PoolTag::Thread,
            index: u16::MAX,
            generation: u32::MAX,
        },
    ] {
        for entry in [
            KeyEntry::from_id(ObjectType::THREAD, id, Rights::all(), 0xBEEF)
                .unwrap_or_else(|error| panic!("named Thread construction: {:?}", error.code())),
            KeyEntry::new::<Thread>(id, Rights::all(), 0xBEEF),
        ] {
            assert!(entry.is_valid());
            assert_eq!(entry.object_type(), ObjectType::THREAD);
            assert!(matches!(
                entry.thread_selector(),
                Ok(ThreadSelector::Named(found)) if found == id
            ));
            assert!(matches!(entry.object_id(), Ok(found) if found == id));
            assert!(!entry.is_thread_return_key());
            assert_eq!(entry.rights(), Rights::all());
            assert_eq!(entry.badge(), 0xBEEF);
            let derived = entry.derive(Rights(Rights::RETIRE));
            assert!(matches!(
                derived.thread_selector(),
                Ok(ThreadSelector::Named(found)) if found == id
            ));
            assert!(matches!(derived.object_id(), Ok(found) if found == id));
            assert_eq!(derived.rights(), Rights(Rights::RETIRE));
            assert_eq!(derived.badge(), 0xBEEF);
        }
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
    assert!(matches!(entry.object_id(), Err(CapError::InvalidOperation)));
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
            derived.object_id(),
            Err(CapError::InvalidOperation)
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
    let address_space = ObjectId {
        pool: PoolTag::AddressSpace,
        index: u16::MAX,
        generation: u32::MAX,
    };
    // These types pin both public signatures to a mandatory nonzero address.
    let constructor: fn(ObjectId, NonZero<u64>, InvocationStackExtent) -> KeyEntry =
        KeyEntry::new_invocation;
    let target_getter: fn(&KeyEntry) -> Result<(ObjectId, NonZero<u64>), CapError> =
        KeyEntry::invocation_target;
    let extent_getter: fn(&KeyEntry) -> Result<InvocationStackExtent, CapError> =
        KeyEntry::invocation_stack_extent;
    for extent in [
        stack_extent(0, 16, 16),
        stack_extent(0x1010, 0x1080, 48),
        stack_extent(0x1010, 0x1080, 112),
        stack_extent(
            ArchObjectsImpl::USER_VA_END - 48,
            ArchObjectsImpl::USER_VA_END,
            32,
        ),
    ] {
        for address in [1, 0x1234_5678_9ABC_DEF0, u64::MAX] {
            let function = NonZero::new(address).unwrap();
            let entry = constructor(address_space, function, extent);
            let target = target_getter(&entry)
                .unwrap_or_else(|error| panic!("Invocation target: {:?}", error.code()));
            assert_eq!(target, (address_space, function));
            assert_eq!(entry.object_type(), ObjectType::INVOCATION);
            assert_eq!(entry.rights(), Rights(Rights::CALL));
            assert_eq!(entry.badge(), 0);
            assert!(!entry.is_thread_return_key());
            assert!(entry.object_id().is_err());
            assert_eq!(
                extent_getter(&entry)
                    .unwrap_or_else(|error| panic!("stored stack extent: {:?}", error.code())),
                extent
            );
            // derive is a representation helper, not permission to distribute
            // Invocation authority through the management allowlist.
            for rights in [Rights::empty(), Rights(Rights::CALL)] {
                let derived = entry.derive(rights);
                assert!(matches!(derived.invocation_target(), Ok(found) if found == target));
                let stored = derived
                    .invocation_stack_extent()
                    .unwrap_or_else(|error| panic!("derived stack extent: {:?}", error.code()));
                assert_eq!(stored, extent);
                assert!(stored.validate_sp(stored.end()).is_ok());
                assert_eq!(derived.rights(), rights);
                assert_eq!(derived.badge(), 0);
            }
            assert_eq!(entry.rights(), Rights(Rights::CALL));
        }
    }
}

#[test_case]
fn thread_selector_rejects_non_thread_payloads() {
    let address_space = ObjectId {
        pool: PoolTag::AddressSpace,
        index: 3,
        generation: 7,
    };
    for entry in [
        KeyEntry::null(),
        KeyEntry::new_untyped(0x1000, 12, false, Rights::all()),
        KeyEntry::new_frame(0x2000, 12, false, Rights::all()),
        KeyEntry::new_keytable(TEST_BACKING, CALLER_GUARD, SIZE_BITS, Rights::all(), 0),
        KeyEntry::from_id(ObjectType::ADDRESS_SPACE, address_space, Rights::all(), 0)
            .unwrap_or_else(|error| panic!("AddressSpace construction: {:?}", error.code())),
        KeyEntry::new_invocation(
            address_space,
            NonZero::new(0x80000).unwrap(),
            stack_extent(0x1010, 0x1080, 48),
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
    ] {
        for id in [
            ObjectId {
                pool: PoolTag::Region,
                index: 0,
                generation: 0,
            },
            ObjectId {
                pool: PoolTag::Thread,
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
fn thread_return_representation_preserves_entry_and_payload_layout() {
    assert_eq!(size_of::<ThreadSelector>(), 12);
    assert_eq!(align_of::<ThreadSelector>(), 4);
    assert_eq!(size_of::<InvocationStackExtent>(), 24);
    assert_eq!(align_of::<InvocationStackExtent>(), 8);
    assert_eq!(size_of::<InvocationPayload>(), 40);
    assert_eq!(align_of::<InvocationPayload>(), 8);
    assert_eq!(offset_of!(InvocationPayload, function_address), 0);
    assert_eq!(offset_of!(InvocationPayload, address_space_pool), 8);
    assert_eq!(offset_of!(InvocationPayload, address_space_index), 10);
    assert_eq!(offset_of!(InvocationPayload, address_space_generation), 12);
    assert_eq!(offset_of!(InvocationPayload, stack_extent), 16);
    assert_eq!(size_of::<FramePayload>(), 24);
    assert_eq!(size_of::<KeyEntry>(), 64);
    assert_eq!(align_of::<KeyEntry>(), 32);
    assert_eq!(size_of::<[KeyEntry; 2]>(), 128);
    let entries = [KeyEntry::null(); 2];
    assert_eq!(
        core::ptr::from_ref(&entries[1]) as usize - core::ptr::from_ref(&entries[0]) as usize,
        64
    );
    // The compiled production module also asserts its private KeyPayload union
    // is 40 bytes; do not substitute a test-only mirror of that union.
}

#[test_case]
fn fixture_table_carves_use_literal_non_overlapping_grown_entry_stride() {
    let mut fx = Fixture::new(FULL);
    assert_eq!(KeyTable::carve_size(SIZE_BITS), 17_440);
    assert_eq!(fx.caller_table_addr, TEST_BACKING);
    assert_eq!(fx.dst_table_addr, TEST_BACKING + 17_440);
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
            fx.caller_table_addr,
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
            .keytable_address()
            .unwrap_or_else(|error| panic!("self-table address preserved: {:?}", error.code())),
        fx.caller_table_addr
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
        KeyEntry::new_keytable(TEST_BACKING, CALLER_GUARD, SIZE_BITS, Rights::all(), 0),
        KeyEntry::new_thread_return(),
        KeyEntry::new::<Thread>(
            ObjectId {
                pool: PoolTag::Thread,
                index: 1,
                generation: 9,
            },
            Rights::all(),
            0xBEEF,
        ),
        KeyEntry::from_id(
            ObjectType::ADDRESS_SPACE,
            ObjectId {
                pool: PoolTag::AddressSpace,
                index: 3,
                generation: 7,
            },
            Rights::all(),
            0,
        )
        .unwrap_or_else(|error| panic!("AddressSpace construction: {:?}", error.code())),
    ] {
        assert!(matches!(
            entry.invocation_target(),
            Err(CapError::TypeMismatch { expected, found })
                if expected == ObjectType::INVOCATION && found == entry.object_type()
        ));
        assert!(matches!(
            entry.invocation_stack_extent(),
            Err(CapError::TypeMismatch { expected, found })
                if expected == ObjectType::INVOCATION && found == entry.object_type()
        ));
    }
}

// Numeric validation only: these tests neither provision mappings nor enable
// Call admission, continuation storage, migration, Return or distribution.
#[test_case]
fn stack_extent_errors_report_each_submitted_field_and_literal_reason() {
    use InvalidStackReason as Reason;

    let ceiling = ArchObjectsImpl::USER_VA_END;
    assert_eq!(ceiling, 1_u64 << 48);
    let constructor: fn(u64, u64, u64, u64) -> Result<InvocationStackExtent, CapError> =
        InvocationStackExtent::new;
    for (base, end, minimum, value, reason, id) in [
        (0x1000, 0x1000, 16, 0x1000, Reason::ExtentEmpty, 1),
        (0x2000, 0x1000, 16, 0x1000, Reason::ExtentInverted, 2),
        (
            ceiling,
            ceiling + 16,
            16,
            ceiling,
            Reason::BaseOutsideUserRange,
            3,
        ),
        (
            0,
            ceiling + 16,
            16,
            ceiling + 16,
            Reason::EndOutsideUserRange,
            4,
        ),
        (0x1001, 0x1200, 16, 0x1001, Reason::BaseMisaligned, 5),
        (0x1000, 0x1201, 16, 0x1201, Reason::EndMisaligned, 6),
        (0x1000, 0x1200, 0, 0, Reason::MinimumHeadroomZero, 7),
        (0x1000, 0x1200, 17, 17, Reason::MinimumHeadroomMisaligned, 8),
        (
            0x1000,
            0x1200,
            0x210,
            0x210,
            Reason::MinimumHeadroomTooLarge,
            9,
        ),
    ] {
        assert_invalid_stack(constructor(base, end, minimum, ceiling), value, reason, id);
    }
}

#[test_case]
fn stack_extent_first_error_precedence_is_structure_range_alignment_then_headroom() {
    use InvalidStackReason as Reason;

    let ceiling = ArchObjectsImpl::USER_VA_END;
    for (base, end, minimum, value, reason, id) in [
        // Structure wins over range, alignment and headroom defects.
        (0x1001, 0x1001, 0, 0x1001, Reason::ExtentEmpty, 1),
        (u64::MAX, u64::MAX, 0, u64::MAX, Reason::ExtentEmpty, 1),
        (u64::MAX, 0, u64::MAX, 0, Reason::ExtentInverted, 2),
        (0x2001, 0x1001, 0, 0x1001, Reason::ExtentInverted, 2),
        // Base range wins over end range; both win over boundary alignment.
        (
            ceiling + 1,
            ceiling + 17,
            0,
            ceiling + 1,
            Reason::BaseOutsideUserRange,
            3,
        ),
        (
            1,
            ceiling + 1,
            0,
            ceiling + 1,
            Reason::EndOutsideUserRange,
            4,
        ),
        // Base alignment wins over end alignment, then zero minimum.
        (0x1001, 0x1201, 0, 0x1001, Reason::BaseMisaligned, 5),
        (0x1000, 0x1201, 0, 0x1201, Reason::EndMisaligned, 6),
        (0x1000, 0x1200, 0, 0, Reason::MinimumHeadroomZero, 7),
        // An oversized misaligned minimum is not reported as a fit failure.
        (
            0x1000,
            0x1200,
            0x211,
            0x211,
            Reason::MinimumHeadroomMisaligned,
            8,
        ),
    ] {
        assert_invalid_stack(
            InvocationStackExtent::new(base, end, minimum, ceiling),
            value,
            reason,
            id,
        );
    }
}

#[test_case]
fn stack_extent_accepts_zero_base_ceiling_subpage_and_equal_headroom() {
    let ceiling = ArchObjectsImpl::USER_VA_END;
    for (base, end, minimum) in [
        (0, 16, 16),
        (0, ceiling, ceiling),
        (ceiling - 16, ceiling, 16),
        (0x1010, 0x1080, 16),
        (0x1010, 0x1080, 48),
        (0x1010, 0x1080, 112),
    ] {
        let extent = stack_extent(base, end, minimum);
        assert_eq!(
            (extent.base(), extent.end(), extent.minimum_headroom()),
            (base, end, minimum)
        );
        assert!(extent.validate_sp(end).is_ok());
        assert!(extent.validate_sp(base + minimum).is_ok());
        if minimum == end - base {
            assert_invalid_stack(
                extent.validate_sp(end - 16),
                end - 16,
                if minimum == 16 {
                    InvalidStackReason::SpOutOfRange
                } else {
                    InvalidStackReason::SpInsufficientHeadroom
                },
                if minimum == 16 { 11 } else { 12 },
            );
        }
    }
    // Different requirements are distinct even for the same byte extent.
    assert_ne!(
        stack_extent(0x1010, 0x1080, 16),
        stack_extent(0x1010, 0x1080, 48)
    );
}

#[test_case]
fn stack_sp_errors_pin_alignment_bounds_headroom_and_precedence() {
    use InvalidStackReason as Reason;

    let extent = stack_extent(0x1010, 0x1080, 48);
    let validator: fn(InvocationStackExtent, u64) -> Result<(), CapError> =
        InvocationStackExtent::validate_sp;
    for (sp, reason, id) in [
        (0x1041, Reason::SpMisaligned, 10), // In range with enough headroom.
        (0x1021, Reason::SpMisaligned, 10), // Also insufficient headroom.
        (0x1001, Reason::SpMisaligned, 10), // Also below the base.
        (0x1081, Reason::SpMisaligned, 10), // Also above the end.
        (u64::MAX, Reason::SpMisaligned, 10),
        (0, Reason::SpOutOfRange, 11),
        (0x1000, Reason::SpOutOfRange, 11),
        (0x1010, Reason::SpOutOfRange, 11), // Base is excluded, not a headroom error.
        (0x1090, Reason::SpOutOfRange, 11),
        (u64::MAX - 15, Reason::SpOutOfRange, 11),
        (0x1020, Reason::SpInsufficientHeadroom, 12),
        (0x1030, Reason::SpInsufficientHeadroom, 12),
    ] {
        assert_invalid_stack(validator(extent, sp), sp, reason, id);
    }
    assert!(validator(extent, 0x1040).is_ok()); // Exactly M bytes below SP.
    assert!(validator(extent, 0x1050).is_ok());
    assert!(validator(extent, 0x1080).is_ok()); // Exclusive byte end is a valid SP.
}

#[test_case]
fn stack_sp_enforces_each_extents_own_minimum_without_a_page_floor() {
    for (base, end) in [(0, 112), (0x1010, 0x1080), (0x2000, 0x2100)] {
        for minimum in [16, 48, end - base] {
            let extent = stack_extent(base, end, minimum);
            for sp in (base..=end).step_by(16) {
                if sp == base {
                    assert_invalid_stack(
                        extent.validate_sp(sp),
                        sp,
                        InvalidStackReason::SpOutOfRange,
                        11,
                    );
                } else if sp - base < minimum {
                    assert_invalid_stack(
                        extent.validate_sp(sp),
                        sp,
                        InvalidStackReason::SpInsufficientHeadroom,
                        12,
                    );
                } else {
                    assert!(extent.validate_sp(sp).is_ok());
                }
            }
            assert_invalid_stack(
                extent.validate_sp(end + 16),
                end + 16,
                InvalidStackReason::SpOutOfRange,
                11,
            );
            assert_eq!(extent.minimum_headroom(), minimum);
        }
    }
    let small = stack_extent(0x1010, 0x1080, 16);
    let large = stack_extent(0x1010, 0x1080, 48);
    assert!(small.validate_sp(0x1020).is_ok());
    assert_invalid_stack(
        large.validate_sp(0x1020),
        0x1020,
        InvalidStackReason::SpInsufficientHeadroom,
        12,
    );
}

#[test_case]
fn stack_arithmetic_near_u64_max_compares_before_subtracting_or_adding() {
    use InvalidStackReason as Reason;

    // A synthetic numeric ceiling tests arithmetic independently of AArch64's
    // supported user interval. It grants no authority over high/privileged VAs.
    let base = u64::MAX - 63;
    let end = u64::MAX - 15;
    let extent = InvocationStackExtent::new(base, end, 32, u64::MAX)
        .unwrap_or_else(|error| panic!("synthetic extent: {:?}", error.code()));
    assert_eq!(
        (extent.base(), extent.end(), extent.minimum_headroom()),
        (base, end, 32)
    );
    assert!(extent.validate_sp(base + 32).is_ok());
    assert!(extent.validate_sp(end).is_ok());
    assert_invalid_stack(extent.validate_sp(base), base, Reason::SpOutOfRange, 11);
    assert_invalid_stack(extent.validate_sp(0), 0, Reason::SpOutOfRange, 11);
    assert_invalid_stack(
        extent.validate_sp(base + 16),
        base + 16,
        Reason::SpInsufficientHeadroom,
        12,
    );
    assert_invalid_stack(
        extent.validate_sp(u64::MAX),
        u64::MAX,
        Reason::SpMisaligned,
        10,
    );
    assert_invalid_stack(
        InvocationStackExtent::new(base, end, end, u64::MAX),
        end,
        Reason::MinimumHeadroomTooLarge,
        9,
    ); // base + M would overflow; M must be compared to the ordered difference.
    assert_invalid_stack(
        InvocationStackExtent::new(base, end, u64::MAX, u64::MAX),
        u64::MAX,
        Reason::MinimumHeadroomMisaligned,
        8,
    );
    assert_invalid_stack(
        InvocationStackExtent::new(end, 0, 16, u64::MAX),
        0,
        Reason::ExtentInverted,
        2,
    ); // end - base would underflow if evaluated before ordering.
    assert_invalid_stack(
        InvocationStackExtent::new(base, u64::MAX, 16, u64::MAX),
        u64::MAX,
        Reason::EndMisaligned,
        6,
    );
    let almost_full = InvocationStackExtent::new(0, end, end, u64::MAX)
        .unwrap_or_else(|error| panic!("large synthetic extent: {:?}", error.code()));
    assert!(almost_full.validate_sp(end).is_ok());
    assert_invalid_stack(
        almost_full.validate_sp(end - 16),
        end - 16,
        Reason::SpInsufficientHeadroom,
        12,
    );
    let below_top = InvocationStackExtent::new(base, end - 16, 16, u64::MAX)
        .unwrap_or_else(|error| panic!("short synthetic extent: {:?}", error.code()));
    assert_invalid_stack(below_top.validate_sp(end), end, Reason::SpOutOfRange, 11);
    assert_invalid_stack(
        InvocationStackExtent::new(base, end, 16, ArchObjectsImpl::USER_VA_END),
        base,
        Reason::BaseOutsideUserRange,
        3,
    );
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

    // Frame is allowlisted for capability-only derivation (2026-09-15): a
    // mapped frame derives into an unmapped capability with no mapping
    // association, while the original keeps its mapping.
    let mut fx = Fixture::new(FULL);
    let mut mapped_frame = KeyEntry::new_frame(0x1000, 12, false, Rights::all());
    {
        let frame = mapped_frame
            .as_frame_mut()
            .unwrap_or_else(|_| panic!("not a frame"));
        frame.set_mapped(
            objects::access::ObjectId {
                pool: objects::access::PoolTag::AddressSpace,
                index: 0,
                generation: 1,
            },
            0x1000_0000,
        );
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
fn thread_and_invocation_entries_remain_outside_copy_derive_and_move_allowlist() {
    let thread_id = ObjectId {
        pool: PoolTag::Thread,
        index: 1,
        generation: 9,
    };
    let address_space = ObjectId {
        pool: PoolTag::AddressSpace,
        index: 2,
        generation: 7,
    };
    for entry in [
        KeyEntry::new::<Thread>(thread_id, Rights::all(), 0xBEEF),
        KeyEntry::new_thread_return(),
        KeyEntry::new_thread_return().derive(Rights::all()),
        KeyEntry::new_invocation(
            address_space,
            NonZero::new(0x80000).unwrap(),
            stack_extent(0x1010, 0x1080, 48),
        ),
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
                        source.invocation_stack_extent().unwrap_or_else(|error| {
                            panic!("source Invocation extent: {:?}", error.code())
                        }),
                        entry.invocation_stack_extent().unwrap_or_else(|error| {
                            panic!("original Invocation extent: {:?}", error.code())
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
// GUARDED KEY SPACE (selected 2026-09-23)
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
    let table = unsafe { &mut *(address as *mut KeyTable) };
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
    let table = unsafe { &mut *(address as *mut KeyTable) };
    let foreign = table
        .insert(
            KeySlot::THREAD_RETURN,
            KeyEntry::new_keytable(address, CALLER_GUARD, SIZE_BITS, Rights::all(), 0),
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

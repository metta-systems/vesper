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
    api::KeyEntry,
    core::mem::size_of,
    libobject::{CapError, KeySlot, ObjectType, RawKey, Rights, domain::DomainId},
    objects::{KeyTable, Nucleus, access::Access},
};

// ═══════════════════════════════════════════════════════════════════
// FIXTURES
// ═══════════════════════════════════════════════════════════════════

const FULL: u8 = Rights::DERIVE | Rights::REMOVE | Rights::INSTALL;

/// Fixed RAM address for test-carved `KeyTable`s (QEMU rpi3: 1 GiB RAM at 0).
///
/// The test binary loads at `0x80000` and the DTB sits at `0x8000000`; 32 MiB
/// is clear of both. Carving from a fixed address keeps the large `KeyTable`
/// storage out of the test's stack frame.
const TEST_BACKING: u64 = 0x2000_0000;

/// Carve a `KeyTable` into the fixed test backing at `index`, returning its
/// kernel address.
///
/// Mirrors the boot carve / runtime Retype: the table's storage is the carved
/// region and capabilities reference it by address.
fn carve(index: usize) -> u64 {
    let obj = (TEST_BACKING + (index as u64) * (size_of::<KeyTable>() as u64)) as *mut KeyTable;
    // SAFETY: TEST_BACKING is RAM, aligned for KeyTable, and exclusively owned
    // by the test fixture for its lifetime.
    unsafe {
        obj.write(KeyTable::new(DomainId(0)));
    }
    obj as u64
}

/// A carved-table test fixture: a caller table (with a self-table capability at
/// `CAPTBL_SELF`) and a second, initially vacant table for cross-table
/// operations.
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
                KeySlot::CAPTBL_SELF,
                KeyEntry::new_keytable(caller_table_addr, Rights(table_rights), 0),
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

    /// Install an entry into a table by fixture index.
    fn install(&mut self, index: usize, slot: KeySlot, entry: KeyEntry) -> RawKey {
        let addr = self.table_addr(index);
        // SAFETY: the address names a live carved table owned by the fixture.
        unsafe { &mut *(addr as *mut KeyTable) }
            .insert(slot, entry)
            .unwrap_or_else(|_| panic!("fixture installation failed"))
    }

    /// Look up an entry in a table by fixture index.
    fn lookup(&self, index: usize, key: RawKey) -> Result<&KeyEntry, CapError> {
        let addr = self.table_addr(index);
        // SAFETY: the address names a live carved table owned by the fixture.
        Ok(unsafe { &*(addr as *const KeyTable) }.lookup(key)?)
    }

    /// Remove an entry from a table by fixture index.
    fn remove(&mut self, index: usize, key: RawKey) -> Result<KeyEntry, CapError> {
        let addr = self.table_addr(index);
        // SAFETY: the address names a live carved table owned by the fixture.
        unsafe { &mut *(addr as *mut KeyTable) }.remove(key)
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
        api::key_table::invoke(&access, self.caller_table_addr, table_key, op, args)
    }
}

/// A `KeyTable` capability naming a distinct carved table object (used as a
/// source entry and as a destination-table capability).
fn table_cap(addr: u64, rights: Rights, badge: u16) -> KeyEntry {
    KeyEntry::new_keytable(addr, rights, badge)
}

fn args(a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> [u64; 6] {
    [a0, a1, a2, a3, a4, a5]
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
    assert_eq!(dst_key.slot(), KeySlot(20));
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

    // A non-allowlisted kind (Frame) cannot be derived.
    let mut fx = Fixture::new(FULL);
    let frame = fx.install(
        0,
        KeySlot(11),
        KeyEntry::new_frame(0x1000, 12, false, Rights::all()),
    );
    let call = args(frame.to_wire(), fx.self_table_key.to_wire(), 21, 1, 0, 0);
    assert!(matches!(
        fx.invoke(fx.self_table_key, 0, &call),
        Err(CapError::InvalidObjectType(ObjectType::FRAME))
    ));
}

// ═══════════════════════════════════════════════════════════════════
// MOVE
// ═══════════════════════════════════════════════════════════════════

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
    assert_eq!(dst_key.slot(), KeySlot(20));

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
    assert_eq!(dst_key.slot(), KeySlot(20));

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
    assert_eq!(dst_key.slot(), KeySlot(20));

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
    let restored = RawKey::new(KeySlot(10), 2);
    assert!(fx.lookup(0, restored).is_ok());
}

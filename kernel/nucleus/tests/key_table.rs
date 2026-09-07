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
    libobject::{CapError, KeySlot, ObjectType, RawKey, Rights, domain::DomainId},
    objects::{
        Domain, KeyTable, Nucleus,
        access::{ObjectId, PoolTag},
    },
};

// ═══════════════════════════════════════════════════════════════════
// FIXTURES
// ═══════════════════════════════════════════════════════════════════

/// A domain whose implicit table holds a self-table capability at
/// `KeySlot::CAPTBL_SELF` with the given table-management rights.
fn domain_with_self_table(table_rights: u8) -> (Domain, RawKey) {
    let mut domain = Domain {
        keytable: KeyTable::new(DomainId(0)),
    };
    let self_id = ObjectId {
        pool: PoolTag::KeyTable,
        index: 0,
        generation: 1,
    };
    let key = domain
        .keytable
        .insert(
            KeySlot::CAPTBL_SELF,
            KeyEntry::from_id(ObjectType::KEY_TABLE, self_id, Rights(table_rights), 0),
        )
        .unwrap_or_else(|_| panic!("self-table installation failed"));
    (domain, key)
}

/// Install a derivable entry into the domain's table, returning its key.
///
/// Uses a `KeyTable` capability (always on the approved target-kind
/// allowlist, independent of the `debug_kernel` gate) naming a distinct
/// table object; the entry is a management fixture only — no table object is
/// resolved through it in these tests.
fn install_console(domain: &mut Domain, slot: KeySlot, rights: Rights, badge: u16) -> RawKey {
    domain
        .keytable
        .insert(
            slot,
            KeyEntry::from_id(
                ObjectType::KEY_TABLE,
                ObjectId {
                    pool: PoolTag::KeyTable,
                    index: 1,
                    generation: 1,
                },
                rights,
                badge,
            ),
        )
        .unwrap_or_else(|_| panic!("fixture installation failed"))
}

fn args(a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> [u64; 6] {
    [a0, a1, a2, a3, a4, a5]
}

const FULL: u8 = Rights::DERIVE | Rights::REMOVE | Rights::INSTALL;

// ═══════════════════════════════════════════════════════════════════
// COPYDERIVE
// ═══════════════════════════════════════════════════════════════════

#[test_case]
fn copy_derive_attenuates_rights_and_preserves_badge() {
    let (mut domain, table_key) = domain_with_self_table(FULL);
    let src = install_console(&mut domain, KeySlot(10), Rights::all(), 0xBEEF);
    let dst_slot = KeySlot(20);

    let result = api::key_table::invoke(
        &mut domain,
        table_key,
        0, // CopyDerive
        &args(
            src.to_wire(),
            table_key.to_wire(),
            u64::from(dst_slot.0),
            u64::from(Rights::READ),
            0,
            0,
        ),
    );
    let (key_wire, zero) = result.unwrap_or_else(|_| panic!("copy_derive failed"));
    assert_eq!(zero, 0);
    let dst_key = RawKey::from_wire(key_wire);
    assert_eq!(dst_key.slot(), dst_slot);
    assert_eq!(dst_key.incarnation(), 1);

    let derived = domain
        .keytable
        .lookup(dst_key)
        .unwrap_or_else(|_| panic!("derived entry missing"));
    assert_eq!(derived.object_type(), ObjectType::KEY_TABLE);
    assert_eq!(derived.rights(), Rights(Rights::READ));
    assert_eq!(derived.badge(), 0xBEEF);

    // Source is unchanged.
    let source = domain
        .keytable
        .lookup(src)
        .unwrap_or_else(|_| panic!("source entry lost"));
    assert_eq!(source.rights(), Rights::all());
    assert_eq!(source.badge(), 0xBEEF);
}

#[test_case]
fn copy_derive_rejects_rights_amplification() {
    let (mut domain, table_key) = domain_with_self_table(FULL);
    let src = install_console(&mut domain, KeySlot(10), Rights(Rights::READ), 0);
    let result = api::key_table::invoke(
        &mut domain,
        table_key,
        0,
        &args(
            src.to_wire(),
            table_key.to_wire(),
            20,
            u64::from(Rights::READ | Rights::WRITE),
            0,
            0,
        ),
    );
    assert!(matches!(result, Err(CapError::InsufficientRights)));
    // Failed operation leaves the destination vacant and source unchanged.
    assert_eq!(domain.keytable.len(), 2); // self-table + source
}

#[test_case]
fn copy_derive_requires_derive_on_source_and_install_on_destination() {
    // Source table without DERIVE.
    let (mut domain, table_key) = domain_with_self_table(Rights::INSTALL);
    let src = install_console(&mut domain, KeySlot(10), Rights::all(), 0);
    let call = args(src.to_wire(), table_key.to_wire(), 20, 1, 0, 0);
    assert!(matches!(
        api::key_table::invoke(&mut domain, table_key, 0, &call),
        Err(CapError::InsufficientRights)
    ));

    // Destination table without INSTALL: source has DERIVE but not INSTALL.
    let (mut domain, table_key) = domain_with_self_table(Rights::DERIVE);
    let src = install_console(&mut domain, KeySlot(10), Rights::all(), 0);
    let call = args(src.to_wire(), table_key.to_wire(), 20, 1, 0, 0);
    assert!(matches!(
        api::key_table::invoke(&mut domain, table_key, 0, &call),
        Err(CapError::InsufficientRights)
    ));
}

#[test_case]
fn copy_derive_rejects_occupied_destination_and_non_allowlisted_kinds() {
    let (mut domain, table_key) = domain_with_self_table(FULL);
    let src = install_console(&mut domain, KeySlot(10), Rights::all(), 0);
    let _occupant = install_console(&mut domain, KeySlot(20), Rights::all(), 0);
    let call = args(src.to_wire(), table_key.to_wire(), 20, 1, 0, 0);
    assert!(matches!(
        api::key_table::invoke(&mut domain, table_key, 0, &call),
        Err(CapError::SlotOccupied(KeySlot(20)))
    ));

    // A non-allowlisted kind (Frame) cannot be derived.
    let mut frame_domain = Domain {
        keytable: KeyTable::new(DomainId(0)),
    };
    let fkey = frame_domain
        .keytable
        .insert(
            KeySlot::CAPTBL_SELF,
            KeyEntry::from_id(
                ObjectType::KEY_TABLE,
                ObjectId {
                    pool: PoolTag::KeyTable,
                    index: 0,
                    generation: 1,
                },
                Rights(FULL),
                0,
            ),
        )
        .unwrap_or_else(|_| panic!("self-table failed"));
    let frame = frame_domain
        .keytable
        .insert(
            KeySlot(11),
            KeyEntry::new_frame(0x1000, 12, false, Rights::all()),
        )
        .unwrap_or_else(|_| panic!("frame failed"));
    let call = args(frame.to_wire(), fkey.to_wire(), 21, 1, 0, 0);
    assert!(matches!(
        api::key_table::invoke(&mut frame_domain, fkey, 0, &call),
        Err(CapError::InvalidObjectType(ObjectType::FRAME))
    ));
}

// ═══════════════════════════════════════════════════════════════════
// MOVE
// ═══════════════════════════════════════════════════════════════════

#[test_case]
fn move_preserves_state_and_invalidates_source() {
    let (mut domain, table_key) = domain_with_self_table(FULL);
    let src = install_console(&mut domain, KeySlot(10), Rights(Rights::READ), 0x1234);
    let dst_slot = KeySlot(20);

    let result = api::key_table::invoke(
        &mut domain,
        table_key,
        1, // Move
        &args(
            src.to_wire(),
            table_key.to_wire(),
            u64::from(dst_slot.0),
            0,
            0,
            0,
        ),
    );
    let (key_wire, zero) = result.unwrap_or_else(|_| panic!("move failed"));
    assert_eq!(zero, 0);
    let dst_key = RawKey::from_wire(key_wire);
    assert_eq!(dst_key.slot(), dst_slot);

    let moved = domain
        .keytable
        .lookup(dst_key)
        .unwrap_or_else(|_| panic!("moved entry missing"));
    assert_eq!(moved.rights(), Rights(Rights::READ));
    assert_eq!(moved.badge(), 0x1234);

    // Source key is invalidated (capability invalidated, not just vacant).
    assert!(matches!(
        domain.keytable.lookup(src),
        Err(CapError::InconsistentKey { .. })
    ));
}

#[test_case]
fn move_rejects_same_slot_and_requires_derive_plus_remove() {
    let (mut domain, table_key) = domain_with_self_table(FULL);
    let src = install_console(&mut domain, KeySlot(10), Rights::all(), 0);
    // Same table, same slot: rejected, not a no-op.
    let call = args(
        src.to_wire(),
        table_key.to_wire(),
        u64::from(KeySlot(10).0),
        0,
        0,
        0,
    );
    assert!(matches!(
        api::key_table::invoke(&mut domain, table_key, 1, &call),
        Err(CapError::SlotOccupied(KeySlot(10)))
    ));

    // Missing REMOVE on the source table.
    let (mut domain, table_key) = domain_with_self_table(Rights::DERIVE | Rights::INSTALL);
    let src = install_console(&mut domain, KeySlot(10), Rights::all(), 0);
    let call = args(src.to_wire(), table_key.to_wire(), 20, 0, 0, 0);
    assert!(matches!(
        api::key_table::invoke(&mut domain, table_key, 1, &call),
        Err(CapError::InsufficientRights)
    ));
}

// ═══════════════════════════════════════════════════════════════════
// DELETE
// ═══════════════════════════════════════════════════════════════════

#[test_case]
fn delete_removes_entry_without_object_retirement() {
    let (mut domain, table_key) = domain_with_self_table(FULL);
    let src = install_console(&mut domain, KeySlot(10), Rights::all(), 0);
    let call = args(src.to_wire(), 0, 0, 0, 0, 0);
    let result = api::key_table::invoke(&mut domain, table_key, 2, &call);
    assert_eq!(result.unwrap_or_else(|_| panic!("delete failed")), (0, 0));
    assert!(matches!(
        domain.keytable.lookup(src),
        Err(CapError::InconsistentKey { .. })
    ));
    assert_eq!(domain.keytable.len(), 1); // only the self-table cap remains
}

#[test_case]
fn delete_requires_remove_and_rejects_stale_selector() {
    // Missing REMOVE.
    let (mut domain, table_key) = domain_with_self_table(Rights::DERIVE);
    let src = install_console(&mut domain, KeySlot(10), Rights::all(), 0);
    let call = args(src.to_wire(), 0, 0, 0, 0, 0);
    assert!(matches!(
        api::key_table::invoke(&mut domain, table_key, 2, &call),
        Err(CapError::InsufficientRights)
    ));

    // Stale incarnation must not delete a replacement occupant.
    let (mut domain, table_key) = domain_with_self_table(FULL);
    let old = install_console(&mut domain, KeySlot(10), Rights::all(), 0);
    domain
        .keytable
        .remove(old)
        .unwrap_or_else(|_| panic!("remove failed"));
    let replacement = install_console(&mut domain, KeySlot(10), Rights::all(), 0);
    assert_ne!(old.incarnation(), replacement.incarnation());
    let call = args(old.to_wire(), 0, 0, 0, 0, 0);
    assert!(matches!(
        api::key_table::invoke(&mut domain, table_key, 2, &call),
        Err(CapError::InconsistentKey { .. })
    ));
    // The replacement survives the stale delete attempt.
    assert!(domain.keytable.lookup(replacement).is_ok());
}

// ═══════════════════════════════════════════════════════════════════
// REVOKE AND CROSS-TABLE REJECTION
// ═══════════════════════════════════════════════════════════════════

#[test_case]
fn revoke_is_rejected_and_non_self_tables_are_unsupported() {
    let (mut domain, table_key) = domain_with_self_table(FULL);
    let src = install_console(&mut domain, KeySlot(10), Rights::all(), 0);
    // Revoke (op 4) is not part of the approved minimal lifecycle.
    let call = args(src.to_wire(), 0, 0, 0, 0, 0);
    assert!(matches!(
        api::key_table::invoke(&mut domain, table_key, 4, &call),
        Err(CapError::InvalidOperation)
    ));

    // A KeyTable capability in a non-CAPTBL_SELF slot does not resolve to
    // the caller's table.
    let other_table = domain
        .keytable
        .insert(
            KeySlot(30),
            KeyEntry::from_id(
                ObjectType::KEY_TABLE,
                ObjectId {
                    pool: PoolTag::KeyTable,
                    index: 1,
                    generation: 1,
                },
                Rights(FULL),
                0,
            ),
        )
        .unwrap_or_else(|_| panic!("other table cap failed"));
    let call = args(src.to_wire(), 0, 0, 0, 0, 0);
    assert!(matches!(
        api::key_table::invoke(&mut domain, other_table, 2, &call),
        Err(CapError::UnsupportedCoreType(libobject::CoreType::KeyTable))
    ));
}

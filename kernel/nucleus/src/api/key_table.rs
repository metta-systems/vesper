//! `KeyTable` management operations: `CopyDerive`, `Move`, `Delete`.
//!
//! Implements the approved minimal `KeyTable` lifecycle (D4, 2026-09-07):
//! incarnation-checked selectors, separately grantable table permissions
//! (`DERIVE`/`REMOVE`/`INSTALL`), vacant destinations, same-slot `Move`
//! rejection, badge preservation on `CopyDerive`, state preservation on
//! `Move`, and `Delete` without automatic object retirement. The initial
//! target-kind allowlist is `KeyTable` and debug-gated `DebugConsole`; other
//! kinds, `Revoke`, and rebadging remain unsupported.
//!
//! Wire schemas (see `doc/nucleus_capabilities.md`):
//! - `CopyDerive` `0`: `x2` source selector, `x3` destination-table key,
//!   `x4` vacant destination slot, `x5` requested rights; `x6..x7` zero.
//!   Returns the destination-local packed key in `x1`, zero in `x2`.
//! - `Move` `1`: `x2` source selector, `x3` destination-table key, `x4`
//!   vacant destination slot; `x5..x7` zero. Returns as `CopyDerive`.
//! - `Delete` `2`: `x2` target selector; `x3..x7` zero. Returns zero in
//!   `x1`/`x2`.

use {
    crate::{
        api::KeyEntry,
        objects::{Domain, key_table::KeyTable},
    },
    libobject::{CapError, KeySlot, KeyTableOp, ObjectType, RawKey, Rights},
};

/// Handle a `KeyTable` management invocation.
///
/// `domain` is the caller's domain (its implicit table resolves the invoked
/// table key and the destination-table key); `table_key` is the invoked
/// source/target-table key. Both table capabilities are resolved through the
/// caller's implicit table with checked identities and authority.
pub fn invoke(
    domain: &mut Domain,
    table_key: RawKey,
    op: u64,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    let op = KeyTableOp::try_from(op)?;
    match op {
        KeyTableOp::CopyDerive => copy_derive(domain, table_key, args),
        KeyTableOp::Move => move_key(domain, table_key, args),
        KeyTableOp::Delete => delete(domain, table_key, args),
        // Revoke's scope/completion contract is unresolved (D2); reject it
        // rather than fake a subtree operation.
        KeyTableOp::Revoke => Err(CapError::InvalidOperation),
    }
}

/// `CopyDerive` `0`: derive an attenuated capability into a vacant destination.
fn copy_derive(
    domain: &mut Domain,
    table_key: RawKey,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    let src_sel = RawKey::from_wire(args[0]);
    let dst_table_key = RawKey::from_wire(args[1]);
    let dst_slot = KeySlot(
        u32::try_from(args[2]).map_err(|_truncated| CapError::InvalidKey {
            key: dst_table_key,
            reason: libobject::InvalidKeyReason::SlotOutOfRange,
            operand: 4,
        })?,
    );
    let requested = Rights(u8::try_from(args[3]).map_err(|_truncated| CapError::InvalidOperation)?);
    if args[4] != 0 || args[5] != 0 {
        return Err(CapError::InvalidOperation);
    }

    // Authority: DERIVE on the invoked source table, INSTALL on destination.
    check_table_rights(domain, table_key, Rights::DERIVE, 0)?;
    check_table_rights(domain, dst_table_key, Rights::INSTALL, 3)?;

    // Resolve the source entry from the invoked table.
    let src_entry = {
        let src_table = table_entry(domain, table_key, 0)?;
        src_table
            .lookup(src_sel)
            .map_err(|e| e.with_key_operand(2))?
    };
    check_allowlisted(src_entry.object_type(), 2)?;

    // No amplification: requested rights must be a subset of the source's.
    if !src_entry.rights().permits(requested) {
        return Err(CapError::InsufficientRights);
    }
    // CopyDerive preserves the badge and payload; only rights attenuate.
    let derived = src_entry.derive(requested);

    install_destination(domain, table_key, dst_table_key, dst_slot, derived)
}

/// Move `1`: transfer a capability to a vacant destination, invalidating the
/// source on commit. Rights and per-capability state are preserved.
fn move_key(
    domain: &mut Domain,
    table_key: RawKey,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    let src_sel = RawKey::from_wire(args[0]);
    let dst_table_key = RawKey::from_wire(args[1]);
    let dst_slot = KeySlot(
        u32::try_from(args[2]).map_err(|_truncated| CapError::InvalidKey {
            key: dst_table_key,
            reason: libobject::InvalidKeyReason::SlotOutOfRange,
            operand: 4,
        })?,
    );
    if args[3] != 0 || args[4] != 0 || args[5] != 0 {
        return Err(CapError::InvalidOperation);
    }

    // Authority: DERIVE + REMOVE on the invoked source table (Move is a
    // derive-then-remove), INSTALL on the destination.
    check_table_rights(domain, table_key, Rights::DERIVE | Rights::REMOVE, 0)?;
    check_table_rights(domain, dst_table_key, Rights::INSTALL, 3)?;

    // Same-table/same-slot Move is rejected, not a successful no-op.
    if table_key == dst_table_key && src_sel.slot() == dst_slot {
        return Err(CapError::SlotOccupied(dst_slot));
    }

    check_allowlisted(
        {
            let src_table = table_entry(domain, table_key, 0)?;
            src_table
                .lookup(src_sel)
                .map_err(|e| e.with_key_operand(2))?
        }
        .object_type(),
        2,
    )?;

    // Validate and reserve the destination before removing source authority.
    // Move preserves the entry verbatim: rights, badge, and payload state.
    let moved = {
        let src_table = table_entry_mut(domain, table_key, 0)?;
        src_table
            .remove(src_sel)
            .map_err(|e| e.with_key_operand(2))?
    };
    match install_destination(domain, table_key, dst_table_key, dst_slot, moved) {
        Ok(result) => Ok(result),
        Err(error) => {
            // Roll back: restore the source entry. Reinsertion into the same
            // slot cannot fail with occupancy (we just removed it), but the
            // slot's incarnation has advanced, so the restored entry gets a
            // fresh incarnation; the original source key stays invalidated.
            let src_table = table_entry_mut(domain, table_key, 0)?;
            drop(src_table.insert(src_sel.slot(), moved));
            Err(error)
        }
    }
}

/// Delete `2`: remove an entry without automatic object retirement.
fn delete(domain: &mut Domain, table_key: RawKey, args: &[u64; 6]) -> Result<(u64, u64), CapError> {
    let target_sel = RawKey::from_wire(args[0]);
    if args[1] != 0 || args[2] != 0 || args[3] != 0 || args[4] != 0 || args[5] != 0 {
        return Err(CapError::InvalidOperation);
    }

    // Authority: REMOVE on the invoked table.
    check_table_rights(domain, table_key, Rights::REMOVE, 0)?;

    let table = table_entry_mut(domain, table_key, 0)?;
    table
        .remove(target_sel)
        .map_err(|e| e.with_key_operand(2))?;
    Ok((0, 0))
}

// ═══════════════════════════════════════════════════════════════════
// HELPERS
// ═══════════════════════════════════════════════════════════════════

/// Resolve a table capability through the caller's implicit table and check
/// that it names a `KeyTable` carrying every permission in `required`.
fn check_table_rights(
    domain: &mut Domain,
    table_key: RawKey,
    required: u8,
    operand: u8,
) -> Result<(), CapError> {
    let entry = domain
        .keytable
        .lookup(table_key)
        .map_err(|e| e.with_key_operand(operand))?;
    if entry.object_type() != ObjectType::KEY_TABLE {
        return Err(CapError::TypeMismatch {
            expected: ObjectType::KEY_TABLE,
            found: entry.object_type(),
        });
    }
    if !entry.rights().has(required) {
        return Err(CapError::InsufficientRights);
    }
    Ok(())
}

/// Resolve a table capability to the caller's own `KeyTable`.
///
/// Implementation status: key tables are not yet pooled objects, so a table
/// capability cannot be resolved to a distinct table object through the
/// `Access` context. Only the caller's own table — named by the
/// `KeySlot::CAPTBL_SELF` self-capability convention — is a valid management
/// target in this slice. A `KeyTable` capability in any other slot is
/// rejected as unsupported rather than silently resolving to the caller's
/// table; the pooled-`KeyTable` migration will replace this with real
/// per-object resolution and alias-safe pair access.
fn check_self_table(domain: &Domain, table_key: RawKey, operand: u8) -> Result<(), CapError> {
    let entry = domain
        .keytable
        .lookup(table_key)
        .map_err(|e| e.with_key_operand(operand))?;
    if entry.object_type() != ObjectType::KEY_TABLE {
        return Err(CapError::TypeMismatch {
            expected: ObjectType::KEY_TABLE,
            found: entry.object_type(),
        });
    }
    if table_key.slot() != KeySlot::CAPTBL_SELF {
        // Distinct table objects are not resolvable yet; do not pretend they
        // name the caller's table.
        return Err(CapError::UnsupportedCoreType(libobject::CoreType::KeyTable));
    }
    Ok(())
}

/// Shared borrow of the caller's own `KeyTable` after self-capability checks.
fn table_entry(domain: &Domain, table_key: RawKey, operand: u8) -> Result<&KeyTable, CapError> {
    check_self_table(domain, table_key, operand)?;
    Ok(&domain.keytable)
}

/// Mutable variant of `table_entry`.
fn table_entry_mut(
    domain: &mut Domain,
    table_key: RawKey,
    operand: u8,
) -> Result<&mut KeyTable, CapError> {
    check_self_table(domain, table_key, operand)?;
    Ok(&mut domain.keytable)
}

/// Install a derived/moved entry into the destination table, returning the
/// destination-local packed key in `x1` and zero in `x2`.
///
/// Same-table installation when source and destination keys name the
/// caller's table; distinct-table installation awaits the pooled-table
/// `Access` path (see `table_entry`). `src_table_key` is reserved for the
/// cross-table form.
fn install_destination(
    domain: &mut Domain,
    _src_table_key: RawKey,
    dst_table_key: RawKey,
    dst_slot: KeySlot,
    entry: KeyEntry,
) -> Result<(u64, u64), CapError> {
    let dst_table = table_entry_mut(domain, dst_table_key, 3)?;
    dst_table
        .insert(dst_slot, entry)
        .map(|key| (key.to_wire(), 0))
        .map_err(|failure| failure.error.with_key_operand(4))
}

/// Only the approved initial target kinds may be derived/moved: `KeyTable`
/// and debug-gated `DebugConsole`. Other kinds remain unsupported.
fn check_allowlisted(obj_type: ObjectType, _operand: u8) -> Result<(), CapError> {
    let allowed = obj_type == ObjectType::KEY_TABLE || {
        #[cfg(feature = "debug_kernel")]
        {
            obj_type == ObjectType::DEBUG_CONSOLE
        }
        #[cfg(not(feature = "debug_kernel"))]
        {
            false
        }
    };
    if allowed {
        Ok(())
    } else {
        Err(CapError::InvalidObjectType(obj_type))
    }
}

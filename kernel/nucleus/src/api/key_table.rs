//! `KeyTable` management operations: `CopyDerive`, `Move`, `Delete`.
//!
//! Implements the approved minimal `KeyTable` lifecycle (D4, 2026-09-07):
//! incarnation-checked selectors, separately grantable table permissions
//! (`DERIVE`/`REMOVE`/`INSTALL`), vacant destinations, same-slot `Move`
//! rejection, badge preservation on `CopyDerive`, state preservation on
//! `Move`, and `Delete` without automatic object retirement. The target-kind
//! allowlist is `KeyTable`, `Frame` (capability-only derivation; added
//! 2026-09-15), and debug-gated `DebugConsole`; other kinds, `Revoke`, and
//! rebadging remain unsupported.
//!
//! `KeyTable`s are Retype-created carved objects referenced by per-type
//! capability payloads (their kernel address). The invoked `table_key` and the
//! destination-table key are resolved through the caller's own table (its
//! `keytable_addr`) to distinct carved tables, so `CopyDerive`/`Move` can
//! target a table other than the caller's. Same-object operands are handled
//! through a single mutable guard; distinct objects use the alias-rejecting
//! pair-resolution form.
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
    crate::objects::{KeyTable, access::Access, key_table::CallerTable},
    libobject::{CapError, KeySlot, KeyTableOp, ObjectType, RawKey, Rights},
    libqemu::semihosting as semi,
};

/// Handle a `KeyTable` management invocation.
///
/// `caller` is the caller's own table context (its implicit table and guard),
/// through which the invoked `table_key` and the destination-table key are
/// resolved. `table_key` is the invoked source/target-table key. Both table
/// capabilities are resolved with checked identities and authority.
pub fn invoke(
    access: &Access,
    caller: CallerTable,
    table_key: RawKey,
    op: u64,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    let op = KeyTableOp::try_from(op)?;
    match op {
        KeyTableOp::CopyDerive => copy_derive(access, caller, table_key, args),
        KeyTableOp::Move => move_key(access, caller, table_key, args),
        KeyTableOp::Delete => delete(access, caller, table_key, args),
        // Revoke's scope/completion contract is unresolved (D2); reject it
        // rather than fake a subtree operation.
        KeyTableOp::Revoke => Err(CapError::InvalidOperation),
    }
}

/// `CopyDerive` `0`: derive an attenuated capability into a vacant destination.
fn copy_derive(
    access: &Access,
    caller: CallerTable,
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

    // Resolve the invoked source and destination table capabilities through
    // the caller's own table, checking type and per-table rights.
    let (src_cap, dst_cap) = {
        let caller_table = access.resolve_carved_mut::<KeyTable>(caller.addr)?;
        let src_cap = resolve_table_cap(&caller_table, table_key, caller.guard, 0)?;
        let dst_cap = resolve_table_cap(&caller_table, dst_table_key, caller.guard, 3)?;
        (src_cap, dst_cap)
    };
    // Authority: DERIVE on the invoked source table, INSTALL on destination.
    if !src_cap.rights.has(Rights::DERIVE) {
        return Err(CapError::InsufficientRights);
    }
    if !dst_cap.rights.has(Rights::INSTALL) {
        return Err(CapError::InsufficientRights);
    }

    if src_cap.address == dst_cap.address {
        // Same table: one mutable guard. No amplification: requested rights
        // must be a subset of the source's; CopyDerive preserves the badge and
        // payload and only attenuates rights.
        let mut table = access.resolve_carved_mut::<KeyTable>(src_cap.address)?;
        let derived = {
            let src_entry = table
                .lookup(src_sel, src_cap.guard)
                .map_err(|e| e.with_key_operand(2))?;
            check_allowlisted(src_entry.object_type(), 2)?;
            if !src_entry.rights().permits(requested) {
                return Err(CapError::InsufficientRights);
            }
            src_entry.derive(requested)
        };
        table
            .insert(dst_slot, derived, dst_cap.guard)
            .map(|key| {
                semi::println!("✅ KeyTable::CopyDerive()");
                (key.to_wire(), 0)
            })
            .map_err(|failure| failure.error.with_key_operand(4))
    } else {
        // Distinct tables: alias-safe pair resolution (destination mutable).
        let (mut dst_table, src_table) =
            access.resolve_carved_pair_mut::<KeyTable>(dst_cap.address, src_cap.address)?;
        let src_entry = src_table
            .lookup(src_sel, src_cap.guard)
            .map_err(|e| e.with_key_operand(2))?;
        check_allowlisted(src_entry.object_type(), 2)?;
        if !src_entry.rights().permits(requested) {
            return Err(CapError::InsufficientRights);
        }
        let derived = src_entry.derive(requested);
        dst_table
            .insert(dst_slot, derived, dst_cap.guard)
            .map(|key| {
                semi::println!("✅ KeyTable::CopyDerive()");
                (key.to_wire(), 0)
            })
            .map_err(|failure| failure.error.with_key_operand(4))
    }
}

/// Move `1`: transfer a capability to a vacant destination, invalidating the
/// source on commit. Rights and per-capability state are preserved.
fn move_key(
    access: &Access,
    caller: CallerTable,
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

    let (src_cap, dst_cap) = {
        let caller_table = access.resolve_carved_mut::<KeyTable>(caller.addr)?;
        let src_cap = resolve_table_cap(&caller_table, table_key, caller.guard, 0)?;
        let dst_cap = resolve_table_cap(&caller_table, dst_table_key, caller.guard, 3)?;
        (src_cap, dst_cap)
    };
    // Authority: DERIVE + REMOVE on the invoked source table (Move is a
    // derive-then-remove), INSTALL on the destination.
    if !src_cap.rights.has(Rights::DERIVE | Rights::REMOVE) {
        return Err(CapError::InsufficientRights);
    }
    if !dst_cap.rights.has(Rights::INSTALL) {
        return Err(CapError::InsufficientRights);
    }

    // Same-table/same-slot Move is rejected, not a successful no-op. Distinct
    // tables may use the same slot number in each. The selector's low
    // `size_bits` bits are its bare index; the guard above them is validated
    // against the source table during lookup.
    if src_cap.address == dst_cap.address && bare_index(&src_cap, src_sel) == dst_slot {
        return Err(CapError::SlotOccupied(dst_slot));
    }

    if src_cap.address == dst_cap.address {
        // Same table: validate and remove the source, then install, rolling
        // back into the source slot on installation failure.
        let mut table = access.resolve_carved_mut::<KeyTable>(src_cap.address)?;
        check_source_allowlisted(&table, src_sel, src_cap.guard, 2)?;
        let moved = table
            .remove(src_sel, src_cap.guard)
            .map_err(|e| e.with_key_operand(2))?;
        match table.insert(dst_slot, moved, dst_cap.guard) {
            Ok(key) => {
                semi::println!("✅ KeyTable::Move()");
                Ok((key.to_wire(), 0))
            }
            Err(failure) => {
                // Reinsertion into the same slot cannot fail with occupancy
                // (we just removed it), but the slot's incarnation has
                // advanced, so the restored entry gets a fresh incarnation;
                // the original source key stays invalidated.
                drop(table.insert(bare_index(&src_cap, src_sel), failure.entry, src_cap.guard));
                Err(failure.error.with_key_operand(4))
            }
        }
    } else {
        // Distinct tables: remove from the source, then install into the
        // destination, rolling back into the source on failure. The source
        // guard is dropped before the destination is resolved, so no aliased
        // mutable references are constructed.
        let moved = {
            let mut src_table = access.resolve_carved_mut::<KeyTable>(src_cap.address)?;
            check_source_allowlisted(&src_table, src_sel, src_cap.guard, 2)?;
            src_table
                .remove(src_sel, src_cap.guard)
                .map_err(|e| e.with_key_operand(2))?
        };
        let install_result = {
            let mut dst_table = access.resolve_carved_mut::<KeyTable>(dst_cap.address)?;
            dst_table.insert(dst_slot, moved, dst_cap.guard)
        };
        match install_result {
            Ok(key) => {
                semi::println!("✅ KeyTable::Move()");
                Ok((key.to_wire(), 0))
            }
            Err(failure) => {
                let mut src_table = access.resolve_carved_mut::<KeyTable>(src_cap.address)?;
                drop(src_table.insert(bare_index(&src_cap, src_sel), failure.entry, src_cap.guard));
                Err(failure.error.with_key_operand(4))
            }
        }
    }
}

/// Delete `2`: remove an entry without automatic object retirement.
fn delete(
    access: &Access,
    caller: CallerTable,
    table_key: RawKey,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    let target_sel = RawKey::from_wire(args[0]);
    if args[1] != 0 || args[2] != 0 || args[3] != 0 || args[4] != 0 || args[5] != 0 {
        return Err(CapError::InvalidOperation);
    }

    let src_cap = {
        let caller_table = access.resolve_carved_mut::<KeyTable>(caller.addr)?;
        resolve_table_cap(&caller_table, table_key, caller.guard, 0)?
    };
    // Authority: REMOVE on the invoked table.
    if !src_cap.rights.has(Rights::REMOVE) {
        return Err(CapError::InsufficientRights);
    }

    let mut table = access.resolve_carved_mut::<KeyTable>(src_cap.address)?;
    table
        .remove(target_sel, src_cap.guard)
        .map_err(|e| e.with_key_operand(2))?;
    semi::println!("✅ KeyTable::Delete()");
    Ok((0, 0))
}

// ═══════════════════════════════════════════════════════════════════
// HELPERS
// ═══════════════════════════════════════════════════════════════════

/// A resolved table capability: the carved table's address, its guard and
/// capacity exponent (copied verbatim by derivation, so every capability
/// naming a table carries them), and its rights.
pub(crate) struct TableCap {
    pub(crate) address: u64,
    pub(crate) guard: u32,
    pub(crate) size_bits: u8,
    pub(crate) rights: Rights,
}

/// The bare slot index of a selector against a table capability's layout: the
/// low `size_bits` bits of its table-relative address (the guard above them is
/// validated against the table during lookup).
fn bare_index(cap: &TableCap, key: RawKey) -> KeySlot {
    KeySlot(key.slot().0 & ((1_u32 << u32::from(cap.size_bits)) - 1))
}

/// Resolve a `KeyTable` capability through the caller's own table, checking
/// that it names a `KeyTable` and returning its carved address, guard, capacity
/// exponent, and rights.
pub(crate) fn resolve_table_cap(
    caller_table: &KeyTable,
    key: RawKey,
    caller_guard: u32,
    operand: u8,
) -> Result<TableCap, CapError> {
    let entry = caller_table
        .lookup(key, caller_guard)
        .map_err(|e| e.with_key_operand(operand))?;
    if entry.object_type() != ObjectType::KEY_TABLE {
        return Err(CapError::TypeMismatch {
            expected: ObjectType::KEY_TABLE,
            found: entry.object_type(),
        });
    }
    let address = entry
        .keytable_address()
        .map_err(|e| e.with_key_operand(operand))?;
    let (guard, size_bits) = entry
        .keytable_guard_and_size()
        .map_err(|e| e.with_key_operand(operand))?;
    Ok(TableCap {
        address,
        guard,
        size_bits,
        rights: entry.rights(),
    })
}

/// Check that the source entry's kind is on the approved derivation allowlist
/// before removing/deriving it.
fn check_source_allowlisted(
    table: &KeyTable,
    src_sel: RawKey,
    guard: u32,
    operand: u8,
) -> Result<(), CapError> {
    let entry = table
        .lookup(src_sel, guard)
        .map_err(|e| e.with_key_operand(operand))?;
    check_allowlisted(entry.object_type(), operand)
}

/// Only the approved initial target kinds may be derived/moved: `KeyTable`,
/// debug-gated `DebugConsole`, and `Frame` (added 2026-09-15: Copy is
/// capability-only derivation — a derived Frame starts unmapped, with no
/// active mapping association; Move preserves the mapping record; Delete of a
/// mapped frame leaves the mapping in place under the accepted-leak model).
/// Other kinds remain unsupported.
fn check_allowlisted(obj_type: ObjectType, _operand: u8) -> Result<(), CapError> {
    let allowed = obj_type == ObjectType::KEY_TABLE || obj_type == ObjectType::FRAME || {
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

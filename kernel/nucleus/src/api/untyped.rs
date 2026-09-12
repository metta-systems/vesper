//! `Untyped.Retype`: create objects from an Untyped's unused watermark range.
//!
//! Wire schema (approved 2026-09-13): invoked on the Untyped capability key;
//! `x2` object kind, `x3` `size_bits`, `x4` count, `x5` destination-table key,
//! `x6` first destination slot, `x7` requested rights. Returns the first
//! destination-local key in `x1`, zero in `x2`.
//!
//! Authority: WRITE on the invoked Untyped, INSTALL on the destination table.
//! The initial kind allowlist is `KeyTable`; other kinds remain unsupported.
//!
//! Transaction: validate → reserve (watermark fit) → initialize each object
//! kernel-privately in the carved region → install capabilities → advance the
//! watermark last. Any failure before the watermark advance leaves the Untyped
//! and the destination table unchanged.

use {
    crate::{
        api::{
            key_entry::{KeyEntry, MIN_ALIGN},
            key_table::resolve_table_cap,
        },
        objects::{KeyTable, access::Access},
    },
    libaddress::{PhysAddr, align},
    libobject::{CapError, KeySlot, ObjectType, RawKey, Rights, UntypedOp},
};

/// Handle an `Untyped` invocation.
///
/// `caller_table_addr` is the caller's own capability table (its implicit
/// table), through which the invoked `untyped_key` and the destination-table
/// key are resolved.
pub fn invoke(
    access: &Access,
    caller_table_addr: u64,
    untyped_key: RawKey,
    op: u64,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    let op = UntypedOp::try_from(op)?;
    match op {
        UntypedOp::Retype => retype(access, caller_table_addr, untyped_key, args),
    }
}

/// `Retype` `0`: carve `count` objects of one kind from the Untyped's unused
/// watermark range and install capabilities into consecutive destination slots.
fn retype(
    access: &Access,
    caller_table_addr: u64,
    untyped_key: RawKey,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    let kind = ObjectType::from(
        u8::try_from(args[0])
            .ok()
            .ok_or(CapError::InvalidOperation)?,
    );
    let size_bits = u8::try_from(args[1])
        .ok()
        .ok_or(CapError::InvalidOperation)?;
    let count = u32::try_from(args[2])
        .ok()
        .ok_or(CapError::InvalidOperation)?;
    let dst_table_key = RawKey::from_wire(args[3]);
    let dst_slot = KeySlot(
        u32::try_from(args[4])
            .ok()
            .ok_or(CapError::InvalidOperation)?,
    );
    let requested = Rights(
        u8::try_from(args[5])
            .ok()
            .ok_or(CapError::InvalidOperation)?,
    );

    // Initial kind allowlist: only KeyTable. Other kinds are rejected, not
    // silently created with wrong semantics.
    if kind != ObjectType::KEY_TABLE {
        return Err(CapError::InvalidObjectType(kind));
    }
    if size_bits != 0 {
        return Err(CapError::InvalidSize(usize::from(size_bits)));
    }
    if count == 0 {
        return Err(CapError::InvalidOperation);
    }

    // Phase 1 — read the Untyped entry and the destination-table capability
    // through the caller's own table, copying out the values needed later.
    let (untyped, dst_cap) = {
        let caller_table = access.resolve_carved_mut::<KeyTable>(caller_table_addr)?;
        let untyped = caller_table.lookup(untyped_key)?;
        if untyped.object_type() != ObjectType::UNTYPED {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::UNTYPED,
                found: untyped.object_type(),
            });
        }
        if !untyped.rights().has(Rights::WRITE) {
            return Err(CapError::InsufficientRights);
        }
        let dst_cap = resolve_table_cap(&caller_table, dst_table_key, 5)?;
        if !dst_cap.rights.has(Rights::INSTALL) {
            return Err(CapError::InsufficientRights);
        }
        (*untyped.as_untyped()?, dst_cap)
    };

    // Phase 2 — reserve: align the watermark to the object alignment and the
    // watermark encoding granularity, and check the whole run fits in the
    // Untyped's unused range. The candidate bytes have no outstanding access:
    // carved regions are kernel-private and never exposed to userspace.
    //
    // The stored watermark is `MIN_ALIGN`-granular: both the base and the
    // committed end are aligned up to it, so the encoding can never discard
    // sub-granularity bytes (which would let the next carve overlap this
    // allocation). The end's padding bytes are consumed, not lost.
    let wm = untyped.watermark_bytes();
    let align = u64::try_from(core::mem::align_of::<KeyTable>().max(MIN_ALIGN)).unwrap();
    let aligned_wm = align::align_up(u64::try_from(wm).unwrap(), align);
    let obj_size = u64::try_from(core::mem::size_of::<KeyTable>()).unwrap();
    let total = obj_size
        .checked_mul(u64::from(count))
        .ok_or(CapError::InvalidSize(0))?;
    let end = align::align_up(
        aligned_wm
            .checked_add(total)
            .ok_or(CapError::InvalidSize(0))?,
        align,
    );
    if end > u64::try_from(untyped.size()).unwrap() {
        return Err(CapError::InsufficientMemory);
    }

    // Phase 3 — resolve the destination table and pre-validate the run of
    // destination slots (in range, vacant, incarnation not exhausted) so the
    // installs below cannot fail.
    let mut dst_table = access.resolve_carved_mut::<KeyTable>(dst_cap.address)?;
    let owner = dst_table.owner();
    let first_slot = u64::from(dst_slot.0);
    for i in 0..u64::from(count) {
        let slot = KeySlot(
            u32::try_from(first_slot + i)
                .ok()
                .ok_or(CapError::InvalidOperation)?,
        );
        dst_table.check_insert(slot)?;
    }

    // Phase 4+5 — initialize each object kernel-privately in the carved region
    // and install its capability. The region is not yet committed (watermark
    // unchanged); stale writes are overwritten by the next carve if the
    // transaction aborts.
    let base = untyped.paddr + aligned_wm;
    let mut installed: [RawKey; KeyTable::NUM_SLOTS] =
        [RawKey::new(KeySlot(0), 0); KeyTable::NUM_SLOTS];
    let mut installed_count = 0_usize;
    let mut first_key = None;
    for i in 0..u64::from(count) {
        let paddr = base + obj_size * i;
        let addr = PhysAddr::new(paddr)
            .user_to_kernel()
            .as_mut_ptr::<KeyTable>();
        // SAFETY: the region lies within the Untyped's unused watermark range,
        // is kernel-private, and is exclusively owned by this invocation.
        unsafe {
            addr.write(KeyTable::new(owner));
        }
        let slot = KeySlot(
            u32::try_from(first_slot + i)
                .ok()
                .ok_or(CapError::InvalidOperation)?,
        );
        let entry = KeyEntry::new_keytable(addr as u64, requested, 0);
        match dst_table.insert(slot, entry) {
            Ok(key) => {
                installed[installed_count] = key;
                installed_count += 1;
                if first_key.is_none() {
                    first_key = Some(key);
                }
            }
            Err(failure) => {
                // Defensive rollback: remove the already-installed capabilities.
                // The watermark is unchanged, so the Untyped's accounting is
                // preserved and the destination table is restored.
                for key in installed[..installed_count].iter().rev() {
                    drop(dst_table.remove(*key));
                }
                return Err(failure.error.with_key_operand(6));
            }
        }
    }

    // Phase 6 — commit: advance the watermark last. The destination table may
    // be the caller's own table; re-resolve it only when they differ.
    let new_watermark = usize::try_from(end).unwrap();
    if dst_cap.address == caller_table_addr {
        dst_table.advance_untyped_watermark(untyped_key, new_watermark)?;
    } else {
        // Distinct tables: the destination guard's borrow ended at its last
        // use above, so resolving the caller's own table cannot alias it.
        let mut caller_table = access.resolve_carved_mut::<KeyTable>(caller_table_addr)?;
        caller_table.advance_untyped_watermark(untyped_key, new_watermark)?;
    }

    Ok((first_key.ok_or(CapError::InvalidOperation)?.to_wire(), 0))
}

//! `Untyped.Retype`: create objects from an Untyped's unused watermark range.
//!
//! Wire schema (approved 2026-09-13): invoked on the Untyped capability key;
//! `x2` object kind, `x3` `size_bits`, `x4` count, `x5` destination-table key,
//! `x6` first destination slot, `x7` requested rights. Returns the first
//! destination-local key in `x1`, zero in `x2`.
//!
//! Authority: WRITE on the invoked Untyped, INSTALL on the destination table.
//! The kind allowlist is `KeyTable` (`size_bits` reserved zero), `Frame`
//! (architecture-validated `size_bits`, added 2026-09-15), `PageTable`
//! (fixed architecture-validated 4 KiB carve, added 2026-09-15), and
//! `Notification` (pure kernel synchronization state, added 2026-09-16: no
//! Untyped bytes are carved — the object is allocated from the
//! bootstrap-carved notification pool and the capability is a checked pool
//! identity); other kinds remain unsupported. Device Untypeds are rejected
//! as sources: no creatable kind is device-capable yet (per-kind device
//! policy is D6).
//!
//! Transaction: validate → reserve (watermark fit) → initialize each object
//! kernel-privately in the carved region (a `KeyTable` is written there; a
//! `Frame`'s and a `PageTable`'s contents are sanitized by zeroing — stale
//! descriptors would leak prior contents into hardware walks) → install
//! capabilities → advance the watermark last. Any failure before the watermark
//! advance leaves the Untyped and the destination table unchanged.

use {
    crate::{
        api::{
            key_entry::{KeyEntry, MIN_ALIGN},
            key_table::resolve_table_cap,
        },
        objects::{
            ArchObjects, KeyTable, Notification, Nucleus,
            access::{Access, ObjectId},
        },
    },
    libaddress::{PhysAddr, align},
    libobject::{ArchType, CapError, KeySlot, ObjectType, RawKey, Rights, UntypedOp},
    libqemu::semihosting as semi,
};

/// Handle an `Untyped` invocation.
///
/// `caller_table_addr` is the caller's own capability table (its implicit
/// table), through which the invoked `untyped_key` and the destination-table
/// key are resolved.
pub fn invoke<A: ArchObjects>(
    access: &Access,
    caller_table_addr: u64,
    untyped_key: RawKey,
    op: u64,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    let op = UntypedOp::try_from(op)?;
    match op {
        UntypedOp::Retype => retype::<A>(access, caller_table_addr, untyped_key, args, nucleus),
    }
}

/// How one carved object is sized, aligned, and kernel-privately initialized.
enum Carve {
    /// A carved `KeyTable`: kernel bookkeeping storage, written at the carve.
    KeyTable,
    /// A carved `Frame`: a raw physical region of `bytes` bytes. There is no
    /// kernel object at the carve; the capability stores the region inline,
    /// and the contents are sanitized (zeroed) before installation.
    Frame { bytes: usize },
    /// A carved `PageTable`: a sanitized (zeroed) 4 KiB hardware-format table.
    /// The capability is a checked pool identity over kernel metadata (carve
    /// address, installation record); the metadata slot is allocated from the
    /// architecture page-table pool, whose backing is charged at bootstrap.
    PageTable { bytes: usize },
    /// A `Notification`: pure kernel synchronization state (bitmap + bounded
    /// wait queue). No Untyped bytes are carved (`size_bits` zero); the
    /// object is allocated from the bootstrap-carved notification pool and
    /// the capability is a checked pool identity over it.
    Notification,
}

/// `Retype` `0`: carve `count` objects of one kind from the Untyped's unused
/// watermark range and install capabilities into consecutive destination slots.
fn retype<A: ArchObjects>(
    access: &Access,
    caller_table_addr: u64,
    untyped_key: RawKey,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
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

    // Kind allowlist and per-kind sizing: only KeyTable bookkeeping storage,
    // raw Frame regions, and PageTable translation storage are creatable from
    // memory. Frame sizes are architecture-validated (AArch64 4 KiB-granule
    // baseline: 12/21/30) and a frame is its own alignment; a PageTable is a
    // fixed architecture-validated 4 KiB carve; other kinds are rejected, not
    // silently created with wrong semantics.
    let carve = match kind {
        ObjectType::KEY_TABLE => {
            if size_bits != 0 {
                return Err(CapError::InvalidSize(usize::from(size_bits)));
            }
            Carve::KeyTable
        }
        ObjectType::FRAME => Carve::Frame {
            bytes: A::validate_frame_size(size_bits)?,
        },
        ObjectType::PAGE_TABLE => Carve::PageTable {
            bytes: A::validate_retype(ArchType::PageTable, size_bits)?,
        },
        ObjectType::NOTIFICATION => {
            if size_bits != 0 {
                return Err(CapError::InvalidSize(usize::from(size_bits)));
            }
            Carve::Notification
        }
        _ => return Err(CapError::InvalidObjectType(kind)),
    };
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
        // Device-memory restriction: no creatable kind is device-capable yet
        // (device regions are MMIO, not kernel-object storage; per-kind device
        // policy is D6). Reject before any reservation, initialization, or
        // watermark change; when a device-capable kind is approved, this
        // becomes a per-kind check.
        if untyped.as_untyped()?.is_device {
            return Err(CapError::InvalidObjectType(kind));
        }
        let dst_cap = resolve_table_cap(&caller_table, dst_table_key, 5)?;
        if !dst_cap.rights.has(Rights::INSTALL) {
            return Err(CapError::InsufficientRights);
        }
        (*untyped.as_untyped()?, dst_cap)
    };

    // Phase 2 — reserve: validate the region extent, align the absolute
    // carve address, and check the whole run fits the Untyped's unused and
    // watermark-representable range. The candidate bytes have no outstanding
    // access: carved regions are kernel-private and never exposed to
    // userspace.
    //
    // The extent must be representable before any shift or addition:
    // `size_bits` below the address width and `paddr + size` within `u64`.
    // Malformed extents are rejected with the region's own size instead of
    // panicking in the size shift.
    let region_bits = untyped.size_bits;
    let region_size = 1_u64
        .checked_shl(u32::from(region_bits))
        .ok_or(CapError::InvalidSize(usize::from(region_bits)))?;
    untyped
        .paddr
        .checked_add(region_size)
        .ok_or(CapError::InvalidSize(usize::from(region_bits)))?;
    // The watermark state field stores `offset >> MIN_ALIGN_BITS` in a `u32`,
    // so the usable range ends at `u32::MAX << MIN_ALIGN_BITS` (that is,
    // `u32::MAX × MIN_ALIGN`) even in larger regions.
    let usable_end = region_size.min(u64::from(u32::MAX) * u64::try_from(MIN_ALIGN).unwrap());
    // A frame and a page table are each their own alignment; a KeyTable uses
    // its type alignment (at least the watermark encoding granularity); a
    // Notification carves no bytes.
    let (obj_size, align) = match carve {
        Carve::KeyTable => (
            u64::try_from(core::mem::size_of::<KeyTable>()).unwrap(),
            u64::try_from(core::mem::align_of::<KeyTable>().max(MIN_ALIGN)).unwrap(),
        ),
        Carve::Frame { bytes } | Carve::PageTable { bytes } => {
            let size = u64::try_from(bytes).unwrap();
            (size, size)
        }
        // Zero bytes at unit alignment: the watermark computation is a
        // no-op and the commit below re-advances it to the same value.
        Carve::Notification => (0, 1),
    };
    // The absolute carve address (`paddr + watermark`) must be aligned, not
    // just the watermark: a region whose base is not aligned still yields
    // aligned objects. The base's misalignment is folded into the watermark
    // computation, so both the carve base and the committed end stay
    // `MIN_ALIGN`-granular and the encoding can never discard
    // sub-granularity bytes (which would let the next carve overlap this
    // allocation). The end's padding bytes are consumed, not lost.
    let base_misalign = untyped.paddr & (align - 1);
    let aligned_wm = align::align_up(
        base_misalign + u64::try_from(untyped.watermark_bytes()).unwrap(),
        align,
    ) - base_misalign;
    let total = obj_size
        .checked_mul(u64::from(count))
        .ok_or(CapError::InvalidSize(0))?;
    let end = align::align_up(
        aligned_wm
            .checked_add(total)
            .ok_or(CapError::InvalidSize(0))?,
        align,
    );
    if end > usable_end {
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

    // Phase 3.5 — for pool-backed kinds (PageTable metadata, Notification
    // state), allocate the kernel pool slots before any memory is touched.
    // Pool backing is explicitly charged at bootstrap; on exhaustion the
    // already-taken slots are released, leaving every part of the
    // transaction unchanged.
    let mut pt_ids: [Option<ObjectId>; KeyTable::NUM_SLOTS] = [const { None }; KeyTable::NUM_SLOTS];
    if matches!(carve, Carve::PageTable { .. }) {
        let base = untyped.paddr + aligned_wm;
        for i in 0..u64::from(count) {
            let index = usize::try_from(i).ok().ok_or(CapError::InvalidOperation)?;
            match nucleus
                .pools
                .arch
                .page_tables
                .allocate(A::new_page_table(base + obj_size * i))
            {
                Some((id, _)) => pt_ids[index] = Some(id),
                None => {
                    for id in pt_ids[..index].iter().flatten().copied() {
                        drop(nucleus.pools.arch.page_tables.deallocate(id));
                    }
                    return Err(CapError::PoolExhausted);
                }
            }
        }
    }
    let mut n_ids: [Option<ObjectId>; KeyTable::NUM_SLOTS] = [const { None }; KeyTable::NUM_SLOTS];
    if matches!(carve, Carve::Notification) {
        for i in 0..u64::from(count) {
            let index = usize::try_from(i).ok().ok_or(CapError::InvalidOperation)?;
            match nucleus.pools.notifications.allocate(Notification::new()) {
                Some((id, _)) => n_ids[index] = Some(id),
                None => {
                    for id in n_ids[..index].iter().flatten().copied() {
                        drop(nucleus.pools.notifications.deallocate(id));
                    }
                    return Err(CapError::PoolExhausted);
                }
            }
        }
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
        let slot = KeySlot(
            u32::try_from(first_slot + i)
                .ok()
                .ok_or(CapError::InvalidOperation)?,
        );
        let entry = match carve {
            Carve::KeyTable => {
                let addr = PhysAddr::new(paddr)
                    .user_to_kernel()
                    .as_mut_ptr::<KeyTable>();
                // SAFETY: the region lies within the Untyped's unused watermark range,
                // is kernel-private, and is exclusively owned by this invocation.
                unsafe {
                    addr.write(KeyTable::new(owner));
                };
                KeyEntry::new_keytable(addr as u64, requested, 0)
            }
            Carve::Frame { bytes } => {
                // Sanitization (selected 2026-09-15): the kernel zeroes the
                // carved frame contents before the capability is installed,
                // so a fresh frame never carries prior-owner or kernel data
                // when first exposed across a protection boundary.
                let addr = PhysAddr::new(paddr).user_to_kernel().as_mut_ptr::<u8>();
                // SAFETY: the region lies within the Untyped's unused
                // watermark range, is ordinary RAM (device sources are
                // rejected above), and has no outstanding access.
                unsafe {
                    core::ptr::write_bytes(addr, 0, bytes);
                };
                KeyEntry::new_frame(paddr, size_bits, untyped.is_device, requested)
            }
            Carve::PageTable { bytes } => {
                // Sanitization: a zeroed table so stale descriptors can never
                // leak prior contents into hardware walks or across protection
                // boundaries.
                let addr = PhysAddr::new(paddr).user_to_kernel().as_mut_ptr::<u8>();
                // SAFETY: the region lies within the Untyped's unused
                // watermark range, is ordinary RAM (device sources are
                // rejected above), and has no outstanding access.
                unsafe {
                    core::ptr::write_bytes(addr, 0, bytes);
                }
                let id = pt_ids[usize::try_from(i).ok().ok_or(CapError::InvalidOperation)?]
                    .expect("page-table metadata slot pre-allocated");
                KeyEntry::from_id(ObjectType::PAGE_TABLE, id, requested, 0)
            }
            Carve::Notification => {
                let id = n_ids[usize::try_from(i).ok().ok_or(CapError::InvalidOperation)?]
                    .expect("notification pool slot pre-allocated");
                KeyEntry::from_id(ObjectType::NOTIFICATION, id, requested, 0)
            }
        };
        match dst_table.insert(slot, entry) {
            Ok(key) => {
                installed[installed_count] = key;
                installed_count += 1;
                if first_key.is_none() {
                    first_key = Some(key);
                }
            }
            Err(failure) => {
                // Defensive rollback: remove the already-installed capabilities
                // and release any pre-allocated pool slots. The
                // watermark is unchanged, so the Untyped's accounting is
                // preserved and the destination table is restored.
                for key in installed[..installed_count].iter().rev() {
                    drop(dst_table.remove(*key));
                }
                for id in pt_ids.iter().flatten().copied() {
                    drop(nucleus.pools.arch.page_tables.deallocate(id));
                }
                for id in n_ids.iter().flatten().copied() {
                    drop(nucleus.pools.notifications.deallocate(id));
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

    let kind_name = match carve {
        Carve::KeyTable => "KeyTable",
        Carve::Frame { .. } => "Frame",
        Carve::PageTable { .. } => "PageTable",
        Carve::Notification => "Notification",
    };
    semi::println!("✅ Untyped::Retype({kind_name}, count {count})");

    Ok((first_key.ok_or(CapError::InvalidOperation)?.to_wire(), 0))
}

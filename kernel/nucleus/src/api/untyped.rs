//! `Untyped.Retype`: create objects from an Untyped's unused watermark range.
//!
//! Wire schema:
//! invoked on the Untyped capability key; `x2` object kind,
//! `x3` `size_bits` — with, for the `KeyTable` kind only, the userspace-chosen
//! table guard packed in bits 39:8 above the `size_bits` byte (bits 63:40
//! reserved zero; every other kind requires bits 63:8 zero) — `x4` count,
//! `x5` destination-table key, `x6` first destination slot (a bare index),
//! `x7` requested rights. Returns the first destination-local key in `x1`,
//! zero in `x2`.
//!
//! Authority: WRITE on the invoked Untyped, INSTALL on the destination table.
//! The kind allowlist is `KeyTable` (a variable-size carve: `size_bits` selects
//! the capacity, 1..=20 entries as `2^size_bits`, and the packed guard — fixed
//! for the table's lifetime, recorded in the created capability — must fit
//! `32 − size_bits` bits), `Frame` (architecture-validated `size_bits`),
//! `PageTable` (a fixed architecture-validated 4 KiB table plus its kernel
//! metadata record), the carved kernel objects `Notification`, `EventCount`,
//! `Time` (an empty budget on the creating core), `Thread` (inert) and
//! `AddressSpace` (inert, with no keytable until `AddressSpace.Bind`), each
//! with `size_bits` zero, and `Untyped` itself (a split into smaller Untypeds
//! — the child region is `2^size_bits` bytes, at least the watermark encoding
//! granularity, its own alignment, with the child watermark starting at zero;
//! no bytes are initialized or sanitized because the split is pure
//! bookkeeping); other kinds remain unsupported. A device Untyped is a source
//! only for the `Untyped` split (which propagates `is_device`) and for device
//! `Frame`s, which are never zeroed; every other kind is rejected. A batch is
//! bounded by `MAX_RETYPE_BATCH` (256 objects; larger counts are rejected with
//! `InvalidOperation` and split by the caller).
//!
//! Transaction: validate → reserve (watermark fit) → initialize each object
//! kernel-privately in the carved region (a `KeyTable`'s header, entries, and
//! counters are written/zeroed there; a `Frame`'s and a `PageTable`'s contents
//! are sanitized by zeroing — stale descriptors would leak prior contents into
//! hardware walks; a carved kernel object's header and state are written) →
//! install capabilities → advance the watermark last. Any failure before the
//! watermark advance leaves the Untyped and the destination table unchanged.

use {
    crate::{
        api::{
            key_entry::{KeyEntry, MIN_ALIGN},
            key_table::resolve_table_cap,
        },
        objects::{
            ArchObjects, EventCount, ExecutionContext, KeyTable, Notification, Nucleus, Thread,
            Time,
            access::Access,
            carved::{Carved, CarvedId, CarvedObject},
            key_table::CallerTable,
        },
    },
    libaddress::PhysAddr,
    libobject::{
        ArchType, CapError, FrameAttribute, KeySlot, ObjectType, RawKey, Rights, UntypedOp,
    },
    libqemu::semihosting as semi,
};

/// Upper bound on one Retype batch: the transaction's
/// defensive rollback records are kernel stack arrays sized by this bound.
/// Larger counts are rejected with `InvalidOperation`; callers split them.
const MAX_RETYPE_BATCH: u32 = 256;
/// `MAX_RETYPE_BATCH` in array-length form (the `u32 → usize` `From` impl is
/// not const-callable, so the widening cast is explicit).
const MAX_RETYPE_BATCH_LEN: usize = MAX_RETYPE_BATCH as usize;

/// Handle an `Untyped` invocation.
///
/// `caller` is the caller's own table context (its implicit table and guard),
/// through which the invoked `untyped_key` and the destination-table key are
/// resolved.
pub fn invoke<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    untyped_key: RawKey,
    op: u64,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    let op = UntypedOp::try_from(op)?;
    match op {
        UntypedOp::Retype => retype::<A>(access, caller, untyped_key, args, nucleus),
    }
}

/// How one carved object is sized, aligned, and kernel-privately initialized.
enum Carve {
    /// A carved `KeyTable`: kernel bookkeeping storage — the header, entries,
    /// and incarnation counters are written/zeroed at the carve. The capacity
    /// is `2^size_bits` entries.
    KeyTable { size_bits: u8 },
    /// A carved `Frame`: a raw physical region of `bytes` bytes. There is no
    /// kernel object at the carve; the capability stores the region inline,
    /// and the contents are sanitized (zeroed) before installation.
    Frame { bytes: usize },
    /// A carved `PageTable`: a sanitized (zeroed) 4 KiB hardware-format table
    /// plus its kernel metadata record (table address, walk level,
    /// installation record), both carved from the Untyped. The capability
    /// carries the record's carved identity.
    PageTable { bytes: usize },
    /// A `Notification`: pure kernel synchronization state (bitmap + bounded
    /// wait queue), carved from the Untyped (`size_bits` zero).
    Notification,
    /// An `EventCount`: pure kernel synchronization state (a monotonic
    /// counter with a bounded await queue), carved from the Untyped
    /// (`size_bits` zero).
    EventCount,
    /// A `Time`: an empty budget (zero budget, empty priority chain) on the
    /// creating core, carved from the Untyped (`size_bits` zero).
    Time,
    /// A carved `Untyped`: a child region of `bytes` bytes split off from the
    /// source. No bytes are initialized or sanitized — an Untyped's contents
    /// are never directly exposed; they become observable only through later
    /// carves (a `Frame` or `PageTable` is zeroed, a `KeyTable` is written), each
    /// of which sanitizes in its own transaction. The capability stores the
    /// region inline and the child's watermark starts at zero (fully unused).
    Untyped { bytes: usize },
    /// An inert `Thread`: no `AddressSpace`, no execution state, never
    /// runnable until the root Scheduler configures and starts it. Carved from
    /// the Untyped (`size_bits` zero).
    Thread,
    /// An inert `AddressSpace`: no keytable, translation root or ASID until
    /// `AddressSpace.Bind`, `PageTable.Map` and `ASIDPool.Assign`. Carved from
    /// the Untyped (`size_bits` zero).
    AddressSpace,
}

/// `Retype` `0`: carve `count` objects of one kind from the Untyped's unused
/// watermark range and install capabilities into consecutive destination slots.
fn retype<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    untyped_key: RawKey,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    let kind = ObjectType::from(
        u8::try_from(args[0])
            .ok()
            .ok_or(CapError::InvalidOperation)?,
    );
    // `x3` carries `size_bits` in its low byte. For the `KeyTable` kind the
    // userspace-chosen table guard is packed in bits 39:8 above it;
    // bits 63:40 are reserved zero. Every other kind requires
    // bits 63:8 zero (the strict zero convention).
    let (size_bits, table_guard, ceiling_request) = if kind == ObjectType::KEY_TABLE {
        let size_bits = u8::try_from(args[1] & 0xFF)
            .ok()
            .ok_or(CapError::InvalidOperation)?;
        let guard_word = args[1] >> 8;
        let guard = u32::try_from(guard_word)
            .map_err(|_bits_63_40_set| CapError::InvalidSize(usize::from(size_bits)))?;
        (size_bits, guard, FrameAttribute::Default)
    } else if kind == ObjectType::FRAME || kind == ObjectType::UNTYPED {
        // Bits 15:8 hold the memory-attribute ceiling code of the objects
        // created (`Default`: inherit the source's); bits 63:16 are reserved.
        if args[1] >> 16 != 0 {
            return Err(CapError::InvalidOperation);
        }
        let ceiling =
            FrameAttribute::from_code((args[1] >> 8) & 0xFF).ok_or(CapError::InvalidOperation)?;
        (
            u8::try_from(args[1] & 0xFF)
                .ok()
                .ok_or(CapError::InvalidOperation)?,
            0,
            ceiling,
        )
    } else {
        (
            u8::try_from(args[1])
                .ok()
                .ok_or(CapError::InvalidOperation)?,
            0,
            FrameAttribute::Default,
        )
    };
    let count = u32::try_from(args[2])
        .ok()
        .ok_or(CapError::InvalidOperation)?;
    let dst_table_key = RawKey::from_wire(args[3]);
    let dst_slot = KeySlot(
        u32::try_from(args[4])
            .ok()
            .ok_or(CapError::InvalidOperation)?,
    );
    let requested = Rights::from_wire(args[5])?;

    // Kind allowlist and per-kind sizing: only KeyTable bookkeeping storage,
    // raw Frame regions, and PageTable translation storage are creatable from
    // memory. Frame sizes are architecture-validated (AArch64 4 KiB-granule
    // baseline: 12/21/30) and a frame is its own alignment; a PageTable is a
    // fixed architecture-validated 4 KiB carve; other kinds are rejected, not
    // silently created with wrong semantics.
    let carve = match kind {
        ObjectType::KEY_TABLE => {
            // A variable-size carve: `size_bits` selects
            // the capacity (2^size_bits entries), and the packed guard — fixed
            // for the table's lifetime — must fit the table-relative address
            // layout: `32 − size_bits` bits above the slot index.
            if !(KeyTable::MIN_SIZE_BITS..=KeyTable::MAX_SIZE_BITS).contains(&size_bits) {
                return Err(CapError::InvalidSize(usize::from(size_bits)));
            }
            if table_guard >> (32_u32 - u32::from(size_bits)) != 0 {
                return Err(CapError::InvalidSize(usize::from(size_bits)));
            }
            Carve::KeyTable { size_bits }
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
        ObjectType::EVENT_COUNT => {
            if size_bits != 0 {
                return Err(CapError::InvalidSize(usize::from(size_bits)));
            }
            Carve::EventCount
        }
        ObjectType::TIME => {
            if size_bits != 0 {
                return Err(CapError::InvalidSize(usize::from(size_bits)));
            }
            Carve::Time
        }
        ObjectType::THREAD => {
            if size_bits != 0 {
                return Err(CapError::InvalidSize(usize::from(size_bits)));
            }
            Carve::Thread
        }
        ObjectType::ADDRESS_SPACE => {
            if size_bits != 0 {
                return Err(CapError::InvalidSize(usize::from(size_bits)));
            }
            Carve::AddressSpace
        }
        ObjectType::UNTYPED => {
            // A split: the child is its own region of
            // `2^size_bits` bytes. A region smaller than the watermark
            // encoding granularity could not keep its committed carve ends
            // encodable — the encoding would discard sub-granular bytes and
            // let the next carve overlap — so smaller exponents are rejected
            // with the child's own `size_bits`.
            let bytes = 1_usize
                .checked_shl(u32::from(size_bits))
                .ok_or(CapError::InvalidSize(usize::from(size_bits)))?;
            if bytes < MIN_ALIGN {
                return Err(CapError::InvalidSize(usize::from(size_bits)));
            }
            Carve::Untyped { bytes }
        }
        _ => return Err(CapError::InvalidObjectType(kind)),
    };
    if count == 0 {
        return Err(CapError::InvalidOperation);
    }
    // The transaction's defensive rollback records are stack arrays sized by
    // the batch bound; a larger run is malformed input, not a resource limit.
    if count > MAX_RETYPE_BATCH {
        return Err(CapError::InvalidOperation);
    }

    // Phase 1 — read the Untyped entry and the destination-table capability
    // through the caller's own table, copying out the values needed later.
    let (untyped, dst_cap) = {
        let caller_table = access.resolve_caller_table(caller)?;
        let untyped = caller_table.lookup(untyped_key, caller.guard)?;
        if untyped.object_type() != ObjectType::UNTYPED {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::UNTYPED,
                found: untyped.object_type(),
            });
        }
        if !untyped.rights().has(Rights::WRITE) {
            return Err(CapError::InsufficientRights);
        }
        // Device memory: a device Untyped
        // splits into device Untypeds and carves into device Frames, which
        // are never touched; every other kind would write kernel state into
        // MMIO and is rejected before any reservation, initialization, or
        // watermark change. `is_device` propagates to the children.
        if untyped.as_untyped()?.is_device
            && kind != ObjectType::UNTYPED
            && kind != ObjectType::FRAME
        {
            return Err(CapError::InvalidObjectType(kind));
        }
        let dst_cap = resolve_table_cap(&caller_table, dst_table_key, caller.guard, 5)?;
        if !dst_cap.rights.has(Rights::INSTALL) {
            return Err(CapError::InsufficientRights);
        }
        (*untyped.as_untyped()?, dst_cap)
    };

    // The memory-attribute ceiling of the Frames and child Untypeds created:
    // the source's, or a lower one the caller asks for. RAM has no ceiling (its
    // Frames map `Cacheable` only), and a ceiling never rises: a request
    // looser than the source's, `Cacheable`, or any request on a RAM source is
    // refused before anything is reserved.
    let ceiling = if ceiling_request == FrameAttribute::Default {
        untyped.ceiling()
    } else if untyped.is_device && ceiling_request.within_device_ceiling(untyped.ceiling()) {
        ceiling_request
    } else {
        return Err(CapError::InvalidOperation);
    };

    // Phase 2 — reserve: validate the region extent, align the absolute
    // carve address, and check the whole run fits the Untyped's unused and
    // watermark-representable range, through the shared
    // `RegionPayload::reserve` primitive (the same internal allocation
    // `ObjectPool::carve` and the bootstrap carves perform). The candidate
    // bytes have no outstanding access: carved regions are kernel-private
    // and never exposed to userspace.
    //
    // A frame, a page table, and a split-off Untyped are each their own
    // alignment; a KeyTable's carve size and alignment derive from its
    // capacity exponent (at least the watermark encoding granularity); a
    // carved kernel object takes its carved record, aligned to at least the
    // watermark granularity.
    let (obj_size, align) = match carve {
        Carve::KeyTable { size_bits } => (
            u64::try_from(KeyTable::carve_size(size_bits)).unwrap(),
            u64::try_from(Carved::<KeyTable>::ALIGN.max(MIN_ALIGN)).unwrap(),
        ),
        Carve::Frame { bytes } | Carve::PageTable { bytes } | Carve::Untyped { bytes } => {
            let size = u64::try_from(bytes).unwrap();
            (size, size)
        }
        Carve::Notification => carved_layout::<Notification>(),
        Carve::EventCount => carved_layout::<EventCount>(),
        Carve::Time => carved_layout::<Time>(),
        Carve::Thread => carved_layout::<Thread>(),
        Carve::AddressSpace => carved_layout::<A::AddressSpace>(),
    };
    // A PageTable also carves its kernel metadata record: the records of a
    // batch follow all its tables, so each table keeps its page alignment
    // without padding between them.
    let record_stride = match carve {
        Carve::PageTable { .. } => carved_layout::<A::PageTable>().0,
        _ => 0,
    };
    let tables = obj_size
        .checked_mul(u64::from(count))
        .ok_or(CapError::InvalidSize(0))?;
    let total = record_stride
        .checked_mul(u64::from(count))
        .and_then(|records| tables.checked_add(records))
        .ok_or(CapError::InvalidSize(0))?;
    let reservation = untyped.reserve(align, total)?;

    // Phase 3 — resolve the destination table and pre-validate the run of
    // destination slots (in range, vacant, incarnation not exhausted) so the
    // installs below cannot fail.
    let mut dst_table = dst_cap.resolve(access)?;
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
    let base = untyped.paddr + u64::try_from(reservation.start).unwrap();
    let records_base = base + tables;
    let core = nucleus.current_cpu();
    let mut installed: [RawKey; MAX_RETYPE_BATCH_LEN] =
        [RawKey::new(KeySlot(0), 0); MAX_RETYPE_BATCH_LEN];
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
            Carve::KeyTable { size_bits } => {
                let addr = PhysAddr::new(paddr).user_to_kernel().as_mut_ptr::<u8>();
                // SAFETY: the region lies within the Untyped's unused
                // watermark range, is kernel-private, is exclusively owned by
                // this invocation, and is aligned to the table's 32-byte
                // header/entry alignment with room for the full carve.
                let table = unsafe { KeyTable::initialize(addr, size_bits) };
                KeyEntry::new_keytable(table, table_guard, size_bits, requested, 0)
            }
            Carve::Frame { bytes } => {
                // A device Frame names MMIO registers: it is never zeroed or
                // otherwise touched.
                if !untyped.is_device {
                    // Sanitization: the kernel zeroes
                    // the carved frame contents before the capability is
                    // installed, so a fresh frame never carries prior-owner
                    // or kernel data when first exposed across a protection
                    // boundary.
                    let addr = PhysAddr::new(paddr).user_to_kernel().as_mut_ptr::<u8>();
                    // SAFETY: the region lies within the Untyped's unused
                    // watermark range, is ordinary RAM (checked just above),
                    // and has no outstanding access.
                    unsafe {
                        core::ptr::write_bytes(addr, 0, bytes);
                    };
                }
                if untyped.is_device {
                    KeyEntry::new_device_frame(paddr, size_bits, ceiling, requested)
                } else {
                    KeyEntry::new_frame(paddr, size_bits, false, requested)
                }
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
                // The table's kernel metadata record follows every table of
                // the batch, so the tables stay contiguous and page-aligned.
                let record = records_base + record_stride * i;
                KeyEntry::new_carved(
                    // SAFETY: as for `carve_object`: the record lies in this
                    // batch's reservation after the tables, at a multiple of
                    // its carved stride from a page-aligned base.
                    unsafe { carve_object(record, A::new_page_table(paddr)) },
                    requested,
                    0,
                )
            }
            Carve::Notification => KeyEntry::new_carved(
                // SAFETY: as for `carve_object`.
                unsafe { carve_object(paddr, Notification::new()) },
                requested,
                0,
            ),
            Carve::EventCount => KeyEntry::new_carved(
                // SAFETY: as for `carve_object`.
                unsafe { carve_object(paddr, EventCount::new()) },
                requested,
                0,
            ),
            Carve::Time => KeyEntry::new_carved(
                // SAFETY: as for `carve_object`.
                unsafe { carve_object(paddr, Time::empty(core)) },
                requested,
                0,
            ),
            Carve::AddressSpace => KeyEntry::new_carved(
                // SAFETY: as for `carve_object`.
                unsafe { carve_object(paddr, A::new_address_space()) },
                requested,
                0,
            ),
            Carve::Thread => KeyEntry::new_thread(
                // SAFETY: as for `carve_object`.
                unsafe {
                    carve_object(
                        paddr,
                        Thread::new(CarvedId::UNBOUND, ExecutionContext::Inactive),
                    )
                },
                requested,
                0,
            ),
            Carve::Untyped { .. } => {
                // Pure bookkeeping: no initialization or sanitization (see
                // the `Carve::Untyped` note). The child's extent is
                // representable by construction — it fits inside the
                // source's checked extent — and its watermark starts at
                // zero. `is_device` propagates from the source.
                if untyped.is_device {
                    KeyEntry::new_device_untyped(paddr, size_bits, ceiling, requested)
                } else {
                    KeyEntry::new_untyped(paddr, size_bits, false, requested)
                }
            }
        };
        match dst_table.insert(slot, entry, dst_cap.guard) {
            Ok(key) => {
                installed[installed_count] = key;
                installed_count += 1;
                if first_key.is_none() {
                    first_key = Some(key);
                }
            }
            Err(failure) => {
                // Defensive rollback: remove the already-installed
                // capabilities. The watermark is unchanged, so the Untyped's
                // accounting is preserved, the destination table is restored,
                // and the records written above stay unreachable until the
                // next carve overwrites them.
                for key in installed[..installed_count].iter().rev() {
                    drop(dst_table.remove(*key, dst_cap.guard));
                }
                return Err(failure.error.with_key_operand(6));
            }
        }
    }

    // Phase 6 — commit: advance the watermark last. The destination table may
    // be the caller's own table; re-resolve it only when they differ.
    let new_watermark = reservation.end;
    if dst_cap.table.address == caller.table.address {
        dst_table.advance_untyped_watermark(untyped_key, caller.guard, new_watermark)?;
    } else {
        // Distinct tables: the destination guard's borrow ended at its last
        // use above, so resolving the caller's own table cannot alias it.
        let mut caller_table = access.resolve_caller_table(caller)?;
        caller_table.advance_untyped_watermark(untyped_key, caller.guard, new_watermark)?;
    }

    let kind_name = match carve {
        Carve::KeyTable { .. } => "KeyTable",
        Carve::Frame { .. } => "Frame",
        Carve::PageTable { .. } => "PageTable",
        Carve::Notification => "Notification",
        Carve::EventCount => "EventCount",
        Carve::Time => "Time",
        Carve::Thread => "Thread",
        Carve::AddressSpace => "AddressSpace",
        Carve::Untyped { .. } => "Untyped",
    };
    semi::println!("✅ Untyped::Retype({kind_name}, count {count})");

    Ok((first_key.ok_or(CapError::InvalidOperation)?.to_wire(), 0))
}

/// Size and alignment one carved `T` takes from an Untyped: the carved
/// record, aligned to at least the watermark granularity.
fn carved_layout<T: CarvedObject>() -> (u64, u64) {
    let align = Carved::<T>::ALIGN.max(MIN_ALIGN);
    (
        u64::try_from(Carved::<T>::SIZE.next_multiple_of(align)).unwrap(),
        u64::try_from(align).unwrap(),
    )
}

/// Write `object` as a fresh carved record at physical address `paddr` and
/// return its identity.
///
/// # Safety
/// `paddr` must start a region of the invoking Untyped's unused watermark
/// range sized and aligned by [`carved_layout`], exclusively owned by this
/// Retype and ordinary RAM (device sources are rejected before carving).
unsafe fn carve_object<T: CarvedObject>(paddr: u64, object: T) -> CarvedId {
    let address = PhysAddr::new(paddr).user_to_kernel().as_mut_ptr::<u8>();
    // SAFETY: forwarded caller guarantee; the direct map makes the region
    // kernel-writable.
    unsafe { Carved::<T>::initialize(address, object) }
}

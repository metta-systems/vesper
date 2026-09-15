//! `Frame.Map`/`Unmap`/`GetAddress`: real descriptor installation through the
//! target Domain's translation context (selected 2026-09-15).
//!
//! Wire schemas (see `doc/nucleus_capabilities.md`):
//! - `Map` `0`: `x2` target Domain key, `x3` virtual address, `x4` requested
//!   rights, `x5` attributes (zero = normal write-back cacheable; other values
//!   rejected), `x6..x7` zero. **The explicit target-Domain argument is
//!   bootstrap-era mechanism**: it lets an authorized builder populate a
//!   Domain's address space before that Domain can run; self-context mapping is
//!   the intended ordinary path once syscall caller identity exists. The
//!   requested rights must be a subset of the frame capability's rights
//!   (permission ceiling); execute is not grantable yet, so every mapping is
//!   PXN|UXN. The walk requires every intermediate table to be present. The
//!   alias policy is enforced ahead of the hardware transition: the frame's
//!   physical extent must not overlap any live mapping in the target Domain,
//!   whatever capability installed it (`PhysicalAlias` otherwise);
//!   cross-Domain aliases are distinct PTEs and remain allowed.
//! - `Unmap` `1`: no arguments. The frame's recorded mapping identity (owning
//!   Domain, full virtual address) locates and clears the leaf descriptor.
//! - `GetAddress` `2`: no arguments; requires `GRANT`. Returns the physical
//!   extent — the base in `x1` and the size in bytes in `x2` (the client
//!   wrapper names it `get_extent`).
//! - `Remap` `3`: unsupported; origin-only remap authority remains open
//!   (D4/D6). Returns a defined error rather than fake success.
//!
//! Carved tables are not yet active in any hardware translation context (no
//! Domain context switching yet), so unmap performs no TLB invalidation; this
//! must become a real invalidation when Domain activation installs these tables
//! into a TTBR.

use {
    crate::objects::{ArchObjects, Domain, KeyTable, Nucleus, access::Access},
    libobject::{CapError, FrameOp, ObjectType, RawKey, Rights},
    libqemu::semihosting as semi,
};

/// Handle a `Frame` capability invocation.
///
/// `caller_table_addr` is the caller's own table, through which the invoked
/// `frame_key` and the target Domain key are resolved.
pub fn invoke<A: ArchObjects>(
    access: &Access,
    caller_table_addr: u64,
    frame_key: RawKey,
    op: u64,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    let op = FrameOp::try_from(op)?;
    match op {
        FrameOp::Map => map::<A>(access, caller_table_addr, frame_key, args, nucleus),
        FrameOp::Unmap => unmap::<A>(access, caller_table_addr, frame_key, args, nucleus),
        FrameOp::GetAddress => get_extent(access, caller_table_addr, frame_key, args),
        // Origin-only remap authority, descendant effects, and the
        // virtual-relocation-versus-physical-replacement question remain open
        // (D4/D6); reject rather than fake an attribute change.
        FrameOp::Remap => Err(CapError::InvalidOperation),
    }
}

/// `Map` `0`: install the frame's descriptor in the target Domain's context.
fn map<A: ArchObjects>(
    access: &Access,
    caller_table_addr: u64,
    frame_key: RawKey,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    let domain_key = RawKey::from_wire(args[0]);
    let vaddr = args[1];
    let requested = Rights(
        u8::try_from(args[2])
            .ok()
            .ok_or(CapError::InvalidOperation)?,
    );
    let attrs = args[3];
    if args[4] != 0 || args[5] != 0 {
        return Err(CapError::InvalidOperation);
    }
    // Attributes accept only zero (normal write-back cacheable, MAIR index 0)
    // until the cache/device attribute dimension is contracted.
    if attrs != 0 {
        return Err(CapError::InvalidOperation);
    }

    // Resolve the frame entry and the target Domain capability through the
    // caller's own table, copying out what is needed later.
    let (frame, domain_id) = {
        let caller_table = access.resolve_carved_mut::<KeyTable>(caller_table_addr)?;
        let entry = caller_table
            .lookup(frame_key)
            .map_err(|e| e.with_key_operand(0))?;
        if entry.object_type() != ObjectType::FRAME {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::FRAME,
                found: entry.object_type(),
            });
        }
        // Permission ceiling: the requested mask must be a subset of the
        // capability's rights, and a mapping without READ is meaningless.
        if !requested.has(Rights::READ) || !entry.rights().permits(requested) {
            return Err(CapError::InsufficientRights);
        }
        let frame = *entry.as_frame().map_err(|e| e.with_key_operand(0))?;
        if frame.is_mapped() {
            return Err(CapError::AlreadyMapped);
        }
        // Device frames cannot be created by Retype today; keep the defensive
        // rejection until a device-memory policy is contracted (D6).
        if frame.is_device() {
            return Err(CapError::InvalidOperation);
        }
        let domain_entry = caller_table
            .lookup(domain_key)
            .map_err(|e| e.with_key_operand(2))?;
        if domain_entry.object_type() != ObjectType::DOMAIN {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::DOMAIN,
                found: domain_entry.object_type(),
            });
        }
        // Authority over the target mapping context.
        if !domain_entry.rights().has(Rights::MAP) {
            return Err(CapError::InsufficientRights);
        }
        let domain_id = domain_entry
            .object_id()
            .map_err(|e| e.with_key_operand(2))?;
        (frame, domain_id)
    };

    // Resolve the target Domain and its translation root.
    let domain = access.resolve::<Domain>(&nucleus.pools.domains, domain_id)?;
    let root = domain
        .translation_root
        .ok_or(CapError::MissingIntermediate { vaddr })?;

    // Alias policy: no two virtual addresses for overlapping physical backing
    // within one Domain. The check is physical, not capability-based, so it
    // also rejects overlaps through different derived caps and different frame
    // sizes; it walks only the target Domain's tables, so cross-Domain aliases
    // remain distinct PTEs. Ahead of the hardware transition: a rejection
    // leaves every table and record unchanged.
    if let Some(existing) = A::find_physical_overlap(root, frame.paddr, frame.size_bits) {
        return Err(CapError::PhysicalAlias { paddr: existing });
    }

    // Hardware transition: the arch layer validates the virtual-address width
    // and alignment, walks the installed tables (missing levels fail with
    // `MissingIntermediate`), checks leaf vacancy, and writes the descriptor.
    let writable = requested.has(Rights::WRITE);
    A::install_frame_pte(root, vaddr, frame.paddr, frame.size_bits, writable)?;

    // Commit: record the mapping identity on the frame entry. The destination
    // is the caller's own table; the entry was validated above and only the
    // mapping record changes.
    let mut caller_table = access.resolve_carved_mut::<KeyTable>(caller_table_addr)?;
    caller_table.record_frame_mapping(frame_key, domain_id, vaddr)?;

    semi::println!("✅ Frame::Map()");
    Ok((0, 0))
}

/// `Unmap` `1`: clear the frame's descriptor through its recorded mapping.
fn unmap<A: ArchObjects>(
    access: &Access,
    caller_table_addr: u64,
    frame_key: RawKey,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    if args.iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }

    let frame = {
        let caller_table = access.resolve_carved_mut::<KeyTable>(caller_table_addr)?;
        let entry = caller_table
            .lookup(frame_key)
            .map_err(|e| e.with_key_operand(0))?;
        if entry.object_type() != ObjectType::FRAME {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::FRAME,
                found: entry.object_type(),
            });
        }
        *entry.as_frame().map_err(|e| e.with_key_operand(0))?
    };
    let mapping = frame.mapping().ok_or(CapError::NotMapped)?;

    // Resolve the recorded owning Domain (generation-checked) and its root.
    let domain = access.resolve::<Domain>(&nucleus.pools.domains, mapping.domain)?;
    let root = domain.translation_root.ok_or(CapError::InvalidOperation)?;

    // Hardware transition: verify the descriptor still points at this frame,
    // then clear it.
    A::clear_frame_pte(root, mapping.vaddr, frame.paddr, frame.size_bits)?;

    // Commit: clear the mapping record.
    let mut caller_table = access.resolve_carved_mut::<KeyTable>(caller_table_addr)?;
    caller_table.clear_frame_mapping(frame_key)?;
    semi::println!("✅ Frame::Unmap()");
    Ok((0, 0))
}

/// `GetExtent` `2`: the frame's physical extent.
fn get_extent(
    access: &Access,
    caller_table_addr: u64,
    frame_key: RawKey,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    if args.iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }
    let caller_table = access.resolve_carved_mut::<KeyTable>(caller_table_addr)?;
    let entry = caller_table
        .lookup(frame_key)
        .map_err(|e| e.with_key_operand(0))?;
    if entry.object_type() != ObjectType::FRAME {
        return Err(CapError::TypeMismatch {
            expected: ObjectType::FRAME,
            found: entry.object_type(),
        });
    }
    if !entry.rights().has(Rights::GRANT) {
        return Err(CapError::InsufficientRights);
    }
    let frame = entry.as_frame().map_err(|e| e.with_key_operand(0))?;
    semi::println!("✅ Frame::GetExtent({}, {})", frame.paddr, frame.size());
    Ok((frame.paddr, frame.size() as u64))
}

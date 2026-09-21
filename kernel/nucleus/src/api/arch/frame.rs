//! `Frame.Map`/`Unmap`/`GetAddress`: real descriptor installation through the
//! target `AddressSpace`'s translation context (selected 2026-09-15).
//!
//! Wire schemas (see `doc/nucleus_capabilities.md`):
//! - `Map` `0`: `x2` target `AddressSpace` key, `x3` virtual address, `x4`
//!   requested rights, `x5` attributes (zero = normal write-back cacheable;
//!   other values rejected), `x6..x7` zero. **The explicit target-`AddressSpace`
//!   argument is bootstrap-era mechanism**: it lets an authorized builder
//!   populate an `AddressSpace` before its Thread can run; self-context mapping
//!   is the intended ordinary path once syscall caller identity exists. The
//!   requested rights must be a subset of the frame capability's rights
//!   (permission ceiling); requesting `EXECUTE` (within the ceiling) clears
//!   PXN|UXN for the descriptor, and without it every mapping stays
//!   execute-never (selected 2026-09-15). The walk requires every
//!   intermediate table to be present. The
//!   alias policy is enforced ahead of the hardware transition: the frame's
//!   physical extent must not overlap any live mapping in the target
//!   `AddressSpace`, whatever capability installed it (`PhysicalAlias`
//!   otherwise); cross-`AddressSpace` aliases are distinct PTEs and remain
//!   allowed.
//! - `Unmap` `1`: no arguments. The frame's recorded mapping identity (owning
//!   `AddressSpace`, full virtual address) locates and clears the leaf
//!   descriptor, and the arch layer withdraws the cached translation for that
//!   address under the owning `AddressSpace`'s bound ASID (if any) before
//!   returning.
//! - `GetAddress` `2`: no arguments; requires `GRANT`. Returns the physical
//!   extent — the base in `x1` and the size in bytes in `x2` (the client
//!   wrapper names it `get_extent`).
//! - `Remap` `3`: unsupported; origin-only remap authority remains open
//!   (D4/D6). Returns a defined error rather than fake success.
//!
//! Carved tables become hardware-live through `AddressSpace.Activate`, which
//! installs the bound root into `TTBR0_EL1` with the
//! bound ASID. The invalidation is executed whenever the owning `AddressSpace`
//! has a bound ASID, and is observable on the live context: the boot test maps
//! a frame, activates, reads through the mapping, unmaps (withdrawing the
//! cached translation), remaps different backing at the same address, and
//! verifies the freshly walked contents. Gating the invalidation on live
//! TTBR installation may be more efficient in the long term once Thread
//! scheduling exists (maintainer remark, 2026-09-15).

use {
    crate::objects::{
        ArchObjects, KeyTable, Nucleus, access::Access, arch_objects::AddressSpaceObject,
    },
    libobject::{CapError, FrameOp, ObjectType, RawKey, Rights},
    libqemu::semihosting as semi,
};

/// Handle a `Frame` capability invocation.
///
/// `caller_table_addr` is the caller's own table, through which the invoked
/// `frame_key` and the target `AddressSpace` key are resolved.
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

/// `Map` `0`: install the frame's descriptor in the target `AddressSpace`'s
/// context.
fn map<A: ArchObjects>(
    access: &Access,
    caller_table_addr: u64,
    frame_key: RawKey,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    let as_key = RawKey::from_wire(args[0]);
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

    // Resolve the frame entry and the target AddressSpace capability through
    // the caller's own table, copying out what is needed later.
    let (frame, as_id) = {
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
        let as_entry = caller_table
            .lookup(as_key)
            .map_err(|e| e.with_key_operand(2))?;
        if as_entry.object_type() != ObjectType::ADDRESS_SPACE {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::ADDRESS_SPACE,
                found: as_entry.object_type(),
            });
        }
        // Authority over the target mapping context.
        if !as_entry.rights().has(Rights::MAP) {
            return Err(CapError::InsufficientRights);
        }
        let as_id = as_entry.object_id().map_err(|e| e.with_key_operand(2))?;
        (frame, as_id)
    };

    // Resolve the target AddressSpace and its translation root.
    let address_space =
        access.resolve::<A::AddressSpace>(&nucleus.pools.arch.address_spaces, as_id)?;
    let root = address_space
        .translation_root()
        .ok_or(CapError::MissingIntermediate { vaddr })?;

    // Alias policy: no two virtual addresses for overlapping physical backing
    // within one AddressSpace. The check is physical, not capability-based, so
    // it also rejects overlaps through different derived caps and different
    // frame sizes; it walks only the target AddressSpace's tables, so
    // cross-AddressSpace aliases remain distinct PTEs. Ahead of the hardware
    // transition: a rejection leaves every table and record unchanged.
    if let Some(existing) = A::find_physical_overlap(root, frame.paddr, frame.size_bits) {
        return Err(CapError::PhysicalAlias { paddr: existing });
    }

    // Hardware transition: the arch layer validates the virtual-address width
    // and alignment, walks the installed tables (missing levels fail with
    // `MissingIntermediate`), checks leaf vacancy, and writes the descriptor.
    let writable = requested.has(Rights::WRITE);
    let executable = requested.has(Rights::EXECUTE);
    A::install_frame_pte(
        root,
        vaddr,
        frame.paddr,
        frame.size_bits,
        writable,
        executable,
    )?;

    // Commit: record the mapping identity on the frame entry. The destination
    // is the caller's own table; the entry was validated above and only the
    // mapping record changes.
    let mut caller_table = access.resolve_carved_mut::<KeyTable>(caller_table_addr)?;
    caller_table.record_frame_mapping(frame_key, as_id, vaddr)?;

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

    // Resolve the recorded owning AddressSpace (generation-checked) and its
    // root.
    let address_space = access
        .resolve::<A::AddressSpace>(&nucleus.pools.arch.address_spaces, mapping.address_space)?;
    let root = address_space
        .translation_root()
        .ok_or(CapError::InvalidOperation)?;
    let bound_asid = address_space.asid();

    // Hardware transition: verify the descriptor still points at this frame,
    // then clear it and withdraw the cached translation under the owning
    // AddressSpace's bound ASID, so a stale translation cannot survive the
    // unmap.
    A::clear_frame_pte(root, mapping.vaddr, frame.paddr, frame.size_bits)?;
    if let Some(asid) = bound_asid {
        A::invalidate_tlb_by_vaddr(asid, mapping.vaddr);
    }

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

//! `Frame.Map`/`Unmap`/`GetAddress`: real descriptor installation through the
//! target `AddressSpace`'s translation context (selected 2026-09-15).
//!
//! Wire schemas (see `doc/capabilities-contract.md`):
//! - `Map` `0`: `x2` target `AddressSpace` key, `x3` virtual address, `x4`
//!   requested rights, `x5` the memory `FrameAttribute` (within the Frame's
//!   ceiling; zero is the Frame's default), `x6..x7` zero. **The explicit target-`AddressSpace`
//!   argument is bootstrap-era mechanism**: it lets an authorized builder
//!   populate an `AddressSpace` before its Thread can run; self-context mapping
//!   is the intended ordinary path once syscall caller identity exists. The
//!   requested rights must be a subset of the frame capability's rights
//!   (permission ceiling); requesting `EXECUTE` (within the ceiling) makes
//!   the mapping executable — at EL1 only with `WRITE`, at EL0 only without
//!   it — and without it every mapping stays execute-never. A device Frame
//!   maps with an attribute not looser than its ceiling (Device-nGnRnE by
//!   default) and never executable (`EXECUTE` is rejected with
//!   `InvalidOperation`); a RAM Frame maps `Cacheable` only. The walk requires every
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
//! - `CleanData` `3`, `InvalidateData` `4`, `CleanInvalidateData` `5`,
//!   `UnifyInstruction` `6`: cache maintenance over bytes `x2..x3` of the
//!   frame (`x4..x7` zero, `start < end <= size`) through the kernel direct
//!   map, so the frame need not be mapped. `InvalidateData` needs `WRITE`
//!   (discarding a dirty line loses data like a write), the others `READ`. A
//!   device Frame has no cacheable memory and is rejected.
//! - Operation ID `7` is unassigned and rejected by checked decoding.
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
    crate::{
        api::key_entry::FramePayload,
        objects::{
            ArchObjects, KeyTable, Nucleus,
            access::Access,
            arch_objects::{AddressSpaceObject, CacheMaintenance},
            key_table::CallerTable,
            object_pool::IdentityError,
        },
    },
    libobject::{CapError, FrameAttribute, FrameOp, ObjectType, RawKey, Rights},
    libqemu::semihosting as semi,
};

/// Handle a `Frame` capability invocation.
///
/// `caller` is the caller's own table context, through which the invoked
/// `frame_key` and the target `AddressSpace` key are resolved.
pub fn invoke<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    frame_key: RawKey,
    op: u64,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    let op = FrameOp::try_from(op)?;
    match op {
        FrameOp::Map => map::<A>(access, caller, frame_key, args, nucleus),
        FrameOp::Unmap => unmap::<A>(access, caller, frame_key, args, nucleus),
        FrameOp::GetAddress => get_extent(access, caller, frame_key, args),
        FrameOp::CleanData => {
            maintain_cache::<A>(access, caller, frame_key, CacheMaintenance::Clean, args)
        }
        FrameOp::InvalidateData => maintain_cache::<A>(
            access,
            caller,
            frame_key,
            CacheMaintenance::Invalidate,
            args,
        ),
        FrameOp::CleanInvalidateData => maintain_cache::<A>(
            access,
            caller,
            frame_key,
            CacheMaintenance::CleanInvalidate,
            args,
        ),
        FrameOp::UnifyInstruction => maintain_cache::<A>(
            access,
            caller,
            frame_key,
            CacheMaintenance::UnifyInstruction,
            args,
        ),
    }
}

/// `Map` `0`: install the frame's descriptor in the target `AddressSpace`'s
/// context.
fn map<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    frame_key: RawKey,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    let as_key = RawKey::from_wire(args[0]);
    let vaddr = args[1];
    let requested = Rights::from_wire(args[2])?;
    let requested_attribute =
        FrameAttribute::from_code(args[3]).ok_or(CapError::InvalidOperation)?;
    if args[4] != 0 || args[5] != 0 {
        return Err(CapError::InvalidOperation);
    }

    // Resolve the frame entry and the target AddressSpace capability through
    // the caller's own table, copying out what is needed later.
    let attribute;
    let (frame, as_id) = {
        let caller_table = access.resolve_caller_table(caller)?;
        let entry = caller_table
            .lookup(frame_key, caller.guard)
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
        attribute = resolve_attribute(&frame, requested_attribute)?;
        // A device Frame is never executable (device memory policy,
        // 2026-10-10).
        if frame.is_device() && requested.has(Rights::EXECUTE) {
            return Err(CapError::InvalidOperation);
        }
        let as_entry = caller_table
            .lookup(as_key, caller.guard)
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
        let as_id = as_entry
            .address_space_id()
            .map_err(|e| e.with_key_operand(2))?;
        (frame, as_id)
    };

    // Resolve the target AddressSpace and its translation root.
    let address_space = access
        .resolve_object::<A::AddressSpace>(as_id)
        .map_err(|error| error.for_key(as_key, 2))?;
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
        attribute,
    )?;

    // Commit: record the mapping identity on the frame entry. The destination
    // is the caller's own table; the entry was validated above and only the
    // mapping record changes.
    let mut caller_table = access.resolve_caller_table(caller)?;
    caller_table.record_frame_mapping(frame_key, caller.guard, as_id, vaddr)?;

    semi::println!("✅ Frame::Map()");
    Ok((0, 0))
}

/// The attribute a mapping of `frame` gets for the requested `x5`: `Default`
/// is the Frame's own default, and anything else must be allowed. A RAM Frame
/// maps `Cacheable` only (the kernel's direct map aliases RAM as write-back); a
/// device Frame maps with any attribute not looser than its ceiling, and never
/// `Cacheable`. A refused request changes nothing.
fn resolve_attribute(
    frame: &FramePayload,
    requested: FrameAttribute,
) -> Result<FrameAttribute, CapError> {
    if !frame.is_device() {
        return match requested {
            FrameAttribute::Default | FrameAttribute::Cacheable => Ok(FrameAttribute::Cacheable),
            _ => Err(CapError::InvalidOperation),
        };
    }
    match requested {
        FrameAttribute::Default => Ok(FrameAttribute::DeviceStrict),
        attribute if attribute.within_device_ceiling(frame.ceiling()) => Ok(attribute),
        _ => Err(CapError::InvalidOperation),
    }
}

/// `Unmap` `1`: clear the frame's descriptor through its recorded mapping.
fn unmap<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    frame_key: RawKey,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    if args.iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }

    let frame = {
        let caller_table = access.resolve_caller_table(caller)?;
        let entry = caller_table
            .lookup(frame_key, caller.guard)
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
        .resolve_object::<A::AddressSpace>(mapping.address_space)
        .map_err(IdentityError::internal)?;
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
    let mut caller_table = access.resolve_caller_table(caller)?;
    caller_table.clear_frame_mapping(frame_key, caller.guard)?;
    semi::println!("✅ Frame::Unmap()");
    Ok((0, 0))
}

/// `GetExtent` `2`: the frame's physical extent.
fn get_extent(
    access: &Access,
    caller: CallerTable,
    frame_key: RawKey,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    if args.iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }
    let caller_table = access.resolve_caller_table(caller)?;
    let entry = caller_table
        .lookup(frame_key, caller.guard)
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

/// `CleanData` `3`, `InvalidateData` `4`, `CleanInvalidateData` `5`,
/// `UnifyInstruction` `6`: cache maintenance over bytes `x2..x3` of the frame.
fn maintain_cache<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    frame_key: RawKey,
    operation: CacheMaintenance,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    let (start, end) = (args[0], args[1]);
    if args[2..].iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }
    let frame = {
        let caller_table = access.resolve_caller_table(caller)?;
        let entry = caller_table
            .lookup(frame_key, caller.guard)
            .map_err(|e| e.with_key_operand(0))?;
        if entry.object_type() != ObjectType::FRAME {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::FRAME,
                found: entry.object_type(),
            });
        }
        // Discarding a dirty line loses data like a write does; the other
        // operations leave every byte's value unchanged.
        let required = if operation == CacheMaintenance::Invalidate {
            Rights::WRITE
        } else {
            Rights::READ
        };
        if !entry.rights().has(required) {
            return Err(CapError::InsufficientRights);
        }
        *entry.as_frame().map_err(|e| e.with_key_operand(0))?
    };
    // Device memory is never cached; the range lies inside the frame.
    if frame.is_device() || start >= end || end > frame.size() as u64 {
        return Err(CapError::InvalidOperation);
    }
    A::maintain_cache(operation, frame.paddr + start, end - start);
    let name = match operation {
        CacheMaintenance::Clean => "CleanData",
        CacheMaintenance::Invalidate => "InvalidateData",
        CacheMaintenance::CleanInvalidate => "CleanInvalidateData",
        CacheMaintenance::UnifyInstruction => "UnifyInstruction",
    };
    semi::println!("✅ Frame::{name}({start:#x}..{end:#x})");
    Ok((0, 0))
}

//! `AddressSpace.Activate`/`Retire`: hardware translation-context
//! installation and address-space teardown (selected 2026-09-21; `Activate`
//! was selected 2026-09-15 as `Domain.Activate` and moved to the `AddressSpace`
//! kind with the Domain split).
//!
//! Wire schemas (see `doc/nucleus_capabilities.md`):
//! - `Activate` `0`: no arguments (`x2..x7` zero). Installs the invoked
//!   `AddressSpace`'s bound translation root into the current hardware
//!   translation context (`TTBR0_EL1` with the bound ASID). Success returns
//!   zeros.
//! - `Retire` `1`: no arguments (`x2..x7` zero). Tears down the invoked
//!   `AddressSpace`: whole-ASID TLB invalidation, ASID release to the
//!   originating pool, root/ASID fields cleared, pool slot reclaimed.
//!   Success returns zeros.
//!
//! Authority: `Activate` requires `MAP` on the invoked `AddressSpace`
//! capability (authority over the mapping context, consistent with root
//! installation, frame mapping, and ASID binding). `Retire` requires
//! `RETIRE` — the same delegable lifecycle-control right under its per-kind
//! interpretation.
//!
//! `Activate` is the translation-context installation step of activation
//! only; until Thread scheduling exists, only the current caller's own
//! `AddressSpace` may be activated (`InvalidOperation` otherwise). Conversely,
//! the current caller's own `AddressSpace` may not be retired
//! (`InvalidOperation`), and a translation root must not still be installed
//! (`InvalidOperation`) — the root is torn down first through the
//! empty-table-gated `PageTable.Unmap` path. Full Thread Start/Suspend/Resume
//! — initialized execution contexts, execution budget, legal state
//! transitions, and EL0 entry — remains Phase 7 work (D8).

use {
    crate::objects::{
        ArchObjects, KeyTable, Nucleus,
        access::Access,
        arch_objects::{AddressSpaceObject, AsidPoolObject},
    },
    libobject::{CapError, ObjectType, RawKey, Rights},
    libqemu::semihosting as semi,
};

/// Handle an `AddressSpace` capability invocation.
///
/// `caller_table_addr` is the caller's own table, through which the invoked
/// `as_key` is resolved.
pub fn invoke<A: ArchObjects>(
    access: &Access,
    caller_table_addr: u64,
    as_key: RawKey,
    op: u64,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    match op {
        0 => activate::<A>(access, caller_table_addr, as_key, args, nucleus),
        1 => retire::<A>(access, caller_table_addr, as_key, args, nucleus),
        _ => Err(CapError::InvalidOperation),
    }
}

/// Resolve the invoked `AddressSpace` capability through the caller's own
/// table, checking `right` and copying out the checked identity.
fn resolve(
    access: &Access,
    caller_table_addr: u64,
    as_key: RawKey,
    right: u8,
) -> Result<crate::objects::access::ObjectId, CapError> {
    let caller_table = access.resolve_carved_mut::<KeyTable>(caller_table_addr)?;
    let entry = caller_table
        .lookup(as_key)
        .map_err(|e| e.with_key_operand(0))?;
    if entry.object_type() != ObjectType::ADDRESS_SPACE {
        return Err(CapError::TypeMismatch {
            expected: ObjectType::ADDRESS_SPACE,
            found: entry.object_type(),
        });
    }
    if !entry.rights().has(right) {
        return Err(CapError::InsufficientRights);
    }
    entry.object_id().map_err(|e| e.with_key_operand(0))
}

/// `Activate` `0`: install this `AddressSpace`'s bound translation root and
/// ASID as the current hardware translation context.
fn activate<A: ArchObjects>(
    access: &Access,
    caller_table_addr: u64,
    as_key: RawKey,
    args: &[u64; 6],
    nucleus: &Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    if args.iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }

    // Authority over the mapping context.
    let as_id = resolve(access, caller_table_addr, as_key, Rights::MAP)?;

    // Bootstrap-era restriction: only the current caller's own AddressSpace
    // activates. Switching the caller's own hardware context to a different
    // address space is a scheduling transition that does not exist yet.
    let current_thread = nucleus.current_thread.ok_or(CapError::InvalidDomain)?;
    let current_as = {
        let index = usize::try_from(current_thread)
            .ok()
            .ok_or(CapError::InvalidDomain)?;
        let thread = nucleus
            .pools
            .threads
            .get_live(index)
            .ok_or(CapError::InvalidDomain)?;
        thread.address_space
    };
    if as_id != current_as {
        return Err(CapError::InvalidOperation);
    }

    // Both binding preconditions must hold before any hardware transition:
    // a root without an ASID (or neither) establishes no hardware context.
    let address_space =
        access.resolve::<A::AddressSpace>(&nucleus.pools.arch.address_spaces, as_id)?;
    let root = address_space
        .translation_root()
        .ok_or(CapError::NotMapped)?;
    let root = address_space
        .translation_root()
        .ok_or(CapError::NotMapped)?;
    let bound_asid = address_space.asid().ok_or(CapError::NotMapped)?;

    // Hardware transition: idempotent installation of the same context.
    A::install_translation_context(root, bound_asid);
    semi::println!("✅ AddressSpace::Activate()");
    Ok((0, 0))
}

/// `Retire` `1` (selected 2026-09-21): tear down the invoked `AddressSpace`.
///
/// Teardown scope: if an ASID is bound, execute the whole-ASID TLB
/// invalidation, then release the ASID back to its originating pool (the
/// boot pool is the only one today; multi-pool partitioning remains D6).
/// Clear the root/ASID fields and reclaim the `AddressSpace`-pool slot.
/// Threads still referencing the retired `AddressSpace` fail generation
/// validation on their next resolution (the stale-identity rule).
fn retire<A: ArchObjects>(
    access: &Access,
    caller_table_addr: u64,
    as_key: RawKey,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    if args.iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }

    // Delegable lifecycle-control authority.
    let as_id = resolve(access, caller_table_addr, as_key, Rights::RETIRE)?;

    // The caller must be a surviving user of a different address space:
    // retiring the current caller's own context has no sound return path.
    let current_thread = nucleus.current_thread.ok_or(CapError::InvalidDomain)?;
    let current_as = {
        let index = usize::try_from(current_thread)
            .ok()
            .ok_or(CapError::InvalidDomain)?;
        let thread = nucleus
            .pools
            .threads
            .get_live(index)
            .ok_or(CapError::InvalidDomain)?;
        thread.address_space
    };
    if as_id == current_as {
        return Err(CapError::InvalidOperation);
    }

    // A still-installed translation root means the address space has live
    // translation structures: they are torn down first through the
    // empty-table-gated `PageTable.Unmap` path.
    let bound_asid = {
        let mut address_space =
            access.resolve_mut::<A::AddressSpace>(&mut nucleus.pools.arch.address_spaces, as_id)?;
        if address_space.translation_root().is_some() {
            return Err(CapError::InvalidOperation);
        }
        let released_asid = address_space.asid();
        // Clear the root/ASID fields before the slot is reclaimed.
        address_space.set_asid(None);
        released_asid
    };

    // Withdraw every cached translation of the whole context, then release
    // the ASID back to its originating pool. The boot pool (index 0) is the
    // only pool today; recording the originating pool per binding is part of
    // the open multi-pool partitioning (D6).
    if let Some(released_asid) = bound_asid {
        A::invalidate_tlb_asid(released_asid);
        if let Some(pool) = nucleus.pools.arch.asid_pools.get_live_mut(0) {
            pool.release(released_asid);
        }
    }

    // Reclaim the pool slot. A stale identity (already retired) fails pool
    // validation with a defined error.
    nucleus
        .pools
        .arch
        .address_spaces
        .deallocate(as_id)
        .map_err(|e| e.with_key_operand(0))?;
    semi::println!("✅ AddressSpace::Retire()");
    Ok((0, 0))
}

//! `AddressSpace.Activate`/`Retire` and Invocation-capability construction.
//!
//! Wire schemas (see `doc/capabilities-contract.md`):
//! - `Activate` `0`: no arguments (`x2..x7` zero). Installs the invoked
//!   `AddressSpace`'s bound translation root into the current hardware
//!   translation context (`TTBR0_EL1` with the bound ASID). Success returns
//!   zeros.
//! - `Retire` `1`: no arguments (`x2..x7` zero). Tears down the invoked
//!   `AddressSpace`: whole-ASID TLB invalidation, ASID release to the
//!   originating pool, root/ASID fields cleared, pool slot reclaimed.
//!   Success returns zeros.
//! - `CreateInvocation` `3`: `x2` nonzero function address, `x3` destination
//!   `KeyTable` capability, `x4` vacant destination slot, `x5` stack base,
//!   `x6` exclusive stack end, `x7` positive minimum headroom in bytes. Requires
//!   `GRANT` on this `AddressSpace` and `INSTALL` on the destination `KeyTable`.
//!   Installs an Invocation capability with only `CALL` authority; returns its
//!   destination-local key in `x1` and zero in `x2`. The function address is
//!   stored as supplied without mapping/executable validation; a zero address is
//!   rejected with `InvalidPointer`. Invocation has a mandatory nonzero entry;
//!   current-relative Thread.Return authority is kernel-constructed separately.
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
//! Implementation status: the bounded wait/resume fixture now switches
//! contexts, but `Activate` remains own-AS-only, not public Thread control.
//! It returns prepared metadata; entry installs after the kernel lock ends.

use {
    crate::{
        api::{InvokeOutcome, KeyEntry, key_table::resolve_table_cap},
        objects::{
            ArchObjects, KeyTable, Nucleus,
            access::Access,
            arch_objects::{AddressSpaceObject, AsidPoolObject},
            invocation::InvocationStackExtent,
            key_table::CallerTable,
            resume::{PreparedTranslationContext, prepare_translation_context},
        },
    },
    core::num::NonZero,
    libobject::{CapError, InvalidKeyReason, KeySlot, ObjectType, RawKey, Rights},
    libqemu::semihosting as semi,
};

/// Handle an `AddressSpace` capability invocation.
///
/// `caller` is the caller's own table context, through which the invoked
/// `as_key` is resolved.
pub fn invoke<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    as_key: RawKey,
    op: u64,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<InvokeOutcome, CapError> {
    match op {
        0 => activate::<A>(access, caller, as_key, args, nucleus).map(InvokeOutcome::Activate),
        1 => retire::<A>(access, caller, as_key, args, nucleus).map(InvokeOutcome::Complete),
        3 => create_invocation::<A>(access, caller, as_key, args, nucleus)
            .map(InvokeOutcome::Complete),
        _ => Err(CapError::InvalidOperation),
    }
}

/// Resolve the invoked `AddressSpace` capability through the caller's own
/// table, checking `right` and copying out the checked identity.
fn resolve(
    access: &Access,
    caller: CallerTable,
    as_key: RawKey,
    right: u8,
) -> Result<crate::objects::access::ObjectId, CapError> {
    let caller_table = access.resolve_carved_mut::<KeyTable>(caller.addr)?;
    let entry = caller_table
        .lookup(as_key, caller.guard)
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

/// `CreateInvocation` `3`: install an Invocation capability into a destination
/// `KeyTable`. All validation completes before the table insertion commit.
fn create_invocation<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    as_key: RawKey,
    args: &[u64; 6],
    nucleus: &Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    let destination_key = RawKey::from_wire(args[1]);

    let (address_space_id, destination) = {
        let caller_table = access.resolve_carved_mut::<KeyTable>(caller.addr)?;
        let address_space_entry = caller_table
            .lookup(as_key, caller.guard)
            .map_err(|error| error.with_key_operand(0))?;
        if address_space_entry.object_type() != ObjectType::ADDRESS_SPACE {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::ADDRESS_SPACE,
                found: address_space_entry.object_type(),
            });
        }
        if !address_space_entry.rights().has(Rights::GRANT) {
            return Err(CapError::InsufficientRights);
        }
        let address_space_id = address_space_entry
            .object_id()
            .map_err(|error| error.with_key_operand(0))?;
        let destination = resolve_table_cap(&caller_table, destination_key, caller.guard, 3)?;
        (address_space_id, destination)
    };

    if !destination.rights.has(Rights::INSTALL) {
        return Err(CapError::InsufficientRights);
    }

    // The target identity must still name a live AddressSpace. The entry point
    // itself is intentionally stored without mapping or executable validation.
    {
        let _target = access
            .resolve::<A::AddressSpace>(&nucleus.pools.arch.address_spaces, address_space_id)?;
    }

    // Supplied-value diagnostics follow authority/liveness, but precede
    // destination admission. No entry/mapping validation is performed.
    // Invocation is Call-only with a mandatory nonzero entry. Userspace
    // cannot construct current-relative Thread.Return authority here.
    let function_address = NonZero::new(args[0]).ok_or(CapError::InvalidPointer)?;
    let stack_extent = InvocationStackExtent::new(args[3], args[4], args[5], A::USER_VA_END)?;
    let destination_slot =
        KeySlot(
            u32::try_from(args[2]).map_err(|_truncated| CapError::InvalidKey {
                key: destination_key,
                reason: InvalidKeyReason::SlotOutOfRange,
                operand: 4,
            })?,
        );
    let invocation = KeyEntry::new_invocation(address_space_id, function_address, stack_extent);
    let mut destination_table = access.resolve_carved_mut::<KeyTable>(destination.address)?;
    destination_table
        .insert(destination_slot, invocation, destination.guard)
        .map(|key| {
            semi::println!("✅ AddressSpace::CreateInvocation()");
            (key.to_wire(), 0)
        })
        .map_err(|failure| failure.error.with_key_operand(4))
}

/// `Activate` `0`: install this `AddressSpace`'s bound translation root and
/// ASID as the current hardware translation context.
/// Implementation status: prepare here, install/trace at syscall entry only
/// after the Access context, object guards, and kernel lock have ended.
fn activate<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    as_key: RawKey,
    args: &[u64; 6],
    nucleus: &Nucleus<A>,
) -> Result<PreparedTranslationContext, CapError> {
    if args.iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }

    // Authority over the mapping context.
    let as_id = resolve(access, caller, as_key, Rights::MAP)?;

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
    // Hardware transition: idempotent installation of the same context.
    // Implementation status: the entry performs that transition after this
    // prepared value has left the locked invocation; no guard escapes here.
    prepare_translation_context::<A>(access, &nucleus.pools.arch.address_spaces, as_id)
}

/// `Retire` `1`: tear down the invoked `AddressSpace`.
///
/// Teardown scope: if an ASID is bound, execute the whole-ASID TLB
/// invalidation, then release the ASID back to its originating pool (the
/// boot pool is the only one today; multi-pool partitioning remains D6).
/// Clear the root/ASID fields and reclaim the `AddressSpace`-pool slot.
/// Threads still referencing the retired `AddressSpace` fail generation
/// validation on their next resolution (the stale-identity rule).
fn retire<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    as_key: RawKey,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    if args.iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }

    // Delegable lifecycle-control authority.
    let as_id = resolve(access, caller, as_key, Rights::RETIRE)?;

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

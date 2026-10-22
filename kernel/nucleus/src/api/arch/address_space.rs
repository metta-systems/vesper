//! `AddressSpace.Activate`/`Retire`/`Bind` and Invocation-capability
//! construction.
//!
//! Wire schemas (see `doc/capabilities-contract.md`):
//! - `Activate` `0`: no arguments (`x2..x7` zero). Installs the invoked
//!   `AddressSpace`'s bound translation root into the current hardware
//!   translation context (`TTBR0_EL1` with the bound ASID). Success returns
//!   zeros.
//! - `Retire` `1`: no arguments (`x2..x7` zero). Tears down the invoked
//!   `AddressSpace`: whole-ASID TLB invalidation, ASID release to the
//!   originating pool, root/ASID fields cleared, the carved record retired.
//!   Success returns zeros.
//! - `Bind` `2`: `x2` a `KeyTable` capability, `x3..x7` zero. Binds that
//!   table to the invoked, still unbound `AddressSpace` (`AlreadyMapped`
//!   otherwise), once and for its lifetime; one table may be bound to several
//!   `AddressSpace`s. The table's `SELF_KEYTABLE` capability must name it
//!   (`InvalidSelfReference` naming slot 4 and the `x2` key otherwise); Bind
//!   installs the `THREAD_RETURN` sentinel. Requires `MAP` on this
//!   `AddressSpace` and `INSTALL` on the table. Success returns zeros.
//! - `CreateInvocation` `3`: `x2` nonzero function address, `x3` destination
//!   `KeyTable` capability, `x4` vacant destination slot, `x5` stack pool handle,
//!   `x6` and `x7` zero, and `x8`
//!   the key-placement window (first slot in bits 31:0, count in bits 63:32;
//!   count 0 accepts no lent key; the window must lie within this
//!   `AddressSpace`'s `KeyTable`, else `InvalidSlot`). Requires
//!   `GRANT` on this `AddressSpace` and `INSTALL` on the destination `KeyTable`.
//!   Installs an Invocation capability with only `CALL` authority; returns its
//!   destination-local key in `x1` and zero in `x2`. The function address is
//!   stored as supplied without mapping/executable validation; a zero address is
//!   rejected with `InvalidPointer`. Invocation has a mandatory nonzero entry;
//!   current-relative Thread.Return authority is kernel-constructed separately.
//!
//! - `CreateStackPool` `4`: `x2` base, `x3` bytes per stack, `x4` stack count,
//!   `x5..x7` zero. Requires `GRANT`. Returns the pool handle in `x1`.
//! - `DestroyStackPool` `5`: `x2` pool handle, `x3..x7` zero. Requires `GRANT`.
//!   Fails with `InvalidOperation` while a stack of the pool is in use.
//!
//! Authority: `Activate` requires `MAP` on the invoked `AddressSpace`
//! capability (authority over the mapping context, consistent with root
//! installation, frame mapping, and ASID binding). `Retire` requires
//! `RETIRE` — the same delegable lifecycle-control right under its per-kind
//! interpretation. Activate and `CreateInvocation` on an unbound
//! `AddressSpace` fail with `NotBound`.
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
            arch_objects::{AddressSpaceId, AddressSpaceObject, AsidPoolObject},
            carved::Carved,
            key_table::CallerTable,
            resume::{PreparedTranslationContext, prepare_translation_context},
        },
    },
    core::num::NonZero,
    libobject::{
        CapError, InvalidKeyReason, KeySlot, ObjectType, PlacementWindow, RawKey, Rights,
        StackPoolHandle, syscall_status::InvalidSelfReference,
    },
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
    x8: u64,
    nucleus: &mut Nucleus<A>,
) -> Result<InvokeOutcome, CapError> {
    match op {
        0 => activate::<A>(access, caller, as_key, args, nucleus).map(InvokeOutcome::Activate),
        1 => retire::<A>(access, caller, as_key, args, nucleus).map(InvokeOutcome::Complete),
        2 => bind::<A>(access, caller, as_key, args).map(InvokeOutcome::Complete),
        3 => create_invocation::<A>(access, caller, as_key, args, x8).map(InvokeOutcome::Complete),
        4 => create_stack_pool::<A>(access, caller, as_key, args, nucleus)
            .map(InvokeOutcome::Complete),
        5 => destroy_stack_pool::<A>(access, caller, as_key, args, nucleus)
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
) -> Result<AddressSpaceId, CapError> {
    let caller_table = access.resolve_caller_table(caller)?;
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
    entry.address_space_id().map_err(|e| e.with_key_operand(0))
}

/// `CreateStackPool` `4`: `x2` base, `x3` bytes per stack, `x4` stack count,
/// `x5..x7` zero. Requires `GRANT`. Returns the pool handle in `x1`.
fn create_stack_pool<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    as_key: RawKey,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    if args[3..].iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }
    let address_space_id = resolve(access, caller, as_key, Rights::GRANT)?;
    let handle = nucleus
        .address_space_mut(address_space_id)
        .ok_or(CapError::InvalidAddressSpace)?
        .stack_pools_mut()
        .create(args[0], args[1], args[2], A::USER_VA_END)?;
    semi::println!("✅ AddressSpace::CreateStackPool()");
    Ok((handle.to_wire(), 0))
}

/// `DestroyStackPool` `5`: `x2` pool handle, `x3..x7` zero. Requires `GRANT`.
fn destroy_stack_pool<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    as_key: RawKey,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    if args[1..].iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }
    let handle = StackPoolHandle::from_wire(args[0]).ok_or(CapError::InvalidOperation)?;
    let address_space_id = resolve(access, caller, as_key, Rights::GRANT)?;
    nucleus
        .address_space_mut(address_space_id)
        .ok_or(CapError::InvalidAddressSpace)?
        .stack_pools_mut()
        .destroy(handle)?;
    semi::println!("✅ AddressSpace::DestroyStackPool()");
    Ok((0, 0))
}

/// `Bind` `2`: bind the `KeyTable` named by `x2` to the invoked, still unbound
/// `AddressSpace`. Every check precedes the two commits: the table's
/// `THREAD_RETURN` sentinel, then the binding itself, which cannot fail.
fn bind<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    as_key: RawKey,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    if args[1..].iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }
    let table_key = RawKey::from_wire(args[0]);

    // Authority: the mapping context, and installation into the table (Bind
    // writes the Return sentinel there). The caller's table guard ends with
    // this block, so the bound table may be the caller's own.
    let as_id = resolve(access, caller, as_key, Rights::MAP)?;
    let table = {
        let caller_table = access.resolve_caller_table(caller)?;
        resolve_table_cap(&caller_table, table_key, caller.guard, 2)?
    };
    if !table.rights.has(Rights::INSTALL) {
        return Err(CapError::InsufficientRights);
    }

    let mut address_space = access
        .resolve_object::<A::AddressSpace>(as_id)
        .map_err(|error| error.for_key(as_key, 0))?;
    if address_space.keytable().is_some() {
        return Err(CapError::AlreadyMapped);
    }

    // Syscall entry reads a table's guard from its `SELF_KEYTABLE`
    // capability, so the table must name itself there; the check and its
    // `InvalidSelfReference` status are the same as at entry.
    let mut keytable = table.resolve(access)?;
    let table_size_bits = keytable.size_bits();
    if keytable
        .self_table_capability()
        .is_none_or(|(named, _guard, size_bits)| {
            named != table.table || size_bits != table_size_bits
        })
    {
        return Err(CapError::InvalidSelfReference {
            reference: InvalidSelfReference::KeyTableKey(KeySlot::SELF_KEYTABLE),
            key: table_key,
        });
    }
    // SAFETY: the table was carved by Retype or bootstrap and is
    // kernel-private; carved memory is never reclaimed or reinitialized, so
    // the binding stays valid for every copy.
    let binding = unsafe { keytable.bind_address_space() }?;
    address_space.bind(binding)?;
    semi::println!("✅ AddressSpace::Bind()");
    Ok((0, 0))
}

/// `CreateInvocation` `3`: install an Invocation capability into a destination
/// `KeyTable`. All validation completes before the table insertion commit.
fn create_invocation<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    as_key: RawKey,
    args: &[u64; 6],
    x8: u64,
) -> Result<(u64, u64), CapError> {
    let destination_key = RawKey::from_wire(args[1]);

    let (address_space_id, destination) = {
        let caller_table = access.resolve_caller_table(caller)?;
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
            .address_space_id()
            .map_err(|error| error.with_key_operand(0))?;
        let destination = resolve_table_cap(&caller_table, destination_key, caller.guard, 3)?;
        (address_space_id, destination)
    };

    if !destination.rights.has(Rights::INSTALL) {
        return Err(CapError::InsufficientRights);
    }

    // The target identity must still name a live, bound AddressSpace. The
    // entry point itself is intentionally stored without mapping or
    // executable validation.
    {
        let target = access
            .resolve_object::<A::AddressSpace>(address_space_id)
            .map_err(|error| error.for_key(as_key, 0))?;
        if target.keytable().is_none() {
            return Err(CapError::NotBound);
        }
    }

    // Supplied-value diagnostics follow authority/liveness, but precede
    // destination admission. No entry/mapping validation is performed.
    // Invocation is Call-only with a mandatory nonzero entry. Userspace
    // cannot construct current-relative Thread.Return authority here.
    let function_address = NonZero::new(args[0]).ok_or(CapError::InvalidPointer)?;
    let stack_pool = StackPoolHandle::from_wire(args[3]).ok_or(CapError::InvalidOperation)?;
    if args[4] != 0 || args[5] != 0 {
        return Err(CapError::InvalidOperation);
    }
    // The exporter's key-placement window must lie within the target's table
    // (an empty window needs no table); it fails like an out-of-range slot.
    let placement_window = PlacementWindow::from_wire(x8);
    if placement_window.accepts_keys() {
        let target_table = Nucleus::<A>::address_space_table(access, address_space_id, as_key)?;
        let table = access.resolve_caller_table(target_table)?;
        if !table.contains_window(placement_window) {
            return Err(CapError::InvalidSlot(KeySlot(placement_window.first())));
        }
    }
    let destination_slot =
        KeySlot(
            u32::try_from(args[2]).map_err(|_truncated| CapError::InvalidKey {
                key: destination_key,
                reason: InvalidKeyReason::SlotOutOfRange,
                operand: 4,
            })?,
        );
    let invocation = KeyEntry::new_invocation(
        address_space_id,
        function_address,
        stack_pool,
        placement_window,
    );
    let mut destination_table = destination.resolve(access)?;
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
    // A retired or unbound AddressSpace is reported before it is compared
    // with the caller's own. The guard ends with this block, before
    // translation preparation resolves the AddressSpace again.
    {
        let address_space = access
            .resolve_object::<A::AddressSpace>(as_id)
            .map_err(|error| error.for_key(as_key, 0))?;
        if address_space.keytable().is_none() {
            return Err(CapError::NotBound);
        }
    }

    // Bootstrap-era restriction: only the current caller's own AddressSpace
    // activates. Switching the caller's own hardware context to a different
    // address space is a scheduling transition that does not exist yet.
    let current_as = nucleus.current_thread_ref()?.address_space;
    if as_id != current_as {
        return Err(CapError::InvalidOperation);
    }

    // Both binding preconditions must hold before any hardware transition:
    // a root without an ASID (or neither) establishes no hardware context.
    // Hardware transition: idempotent installation of the same context.
    // Implementation status: the entry performs that transition after this
    // prepared value has left the locked invocation; no guard escapes here.
    prepare_translation_context::<A>(access, as_id)
}

/// `Retire` `1`: tear down the invoked `AddressSpace`.
///
/// Teardown scope: if an ASID is bound, execute the whole-ASID TLB
/// invalidation, then release the ASID back to its originating pool (the
/// boot pool is the only one today; multi-pool partitioning remains D6).
/// Clear the root/ASID fields and retire the carved `AddressSpace`, bumping
/// its generation. Threads still referencing the retired `AddressSpace` fail
/// generation validation on their next resolution (the stale-identity rule).
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
    let current_as = nucleus.current_thread_ref()?.address_space;
    if as_id == current_as {
        return Err(CapError::InvalidOperation);
    }

    // A still-installed translation root means the address space has live
    // translation structures: they are torn down first through the
    // empty-table-gated `PageTable.Unmap` path.
    let bound_asid = {
        let mut address_space = access
            .resolve_object::<A::AddressSpace>(as_id)
            .map_err(|error| error.for_key(as_key, 0))?;
        if address_space.translation_root().is_some() {
            return Err(CapError::InvalidOperation);
        }
        let released_asid = address_space.asid();
        // Clear the root/ASID fields before the object is retired.
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

    // Retire the carved record: its generation advances, so every capability
    // and Thread still naming it fails validation from now on. Its memory
    // stays below the Untyped's watermark and is never re-carved before the
    // cleanup manager has deleted those capabilities.
    // SAFETY: the identity was resolved above, so it names a carved
    // `AddressSpace` record; the guard to it ended with its block.
    unsafe { Carved::<A::AddressSpace>::retire(as_id) }
        .map_err(|error| error.for_key(as_key, 0))?;
    semi::println!("✅ AddressSpace::Retire()");
    Ok((0, 0))
}

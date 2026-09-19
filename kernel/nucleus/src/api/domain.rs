//! `Domain.Activate`: hardware translation-context installation (selected
//! 2026-09-15). `Domain.Retire`: teardown of a non-current Domain (selected
//! 2026-09-19).
//!
//! Wire schema (see `doc/nucleus_capabilities.md`):
//! - `Activate` `0`: no arguments (`x2..x7` zero). Installs the invoked
//!   Domain's bound translation root into the current hardware translation
//!   context (`TTBR0_EL1` with the bound ASID). Success returns zeros.
//! - `Retire` `4`: no arguments (`x2..x7` zero). Tears down the invoked
//!   Domain: cancels every pending record naming it as waiter, purges its
//!   queued wakeup, and reclaims its Domain-pool slot. Success returns
//!   zeros.
//!
//! Authority: `Activate` requires `MAP` on the invoked Domain capability
//! (authority over the mapping context, consistent with root installation,
//! frame mapping, and ASID binding). `Retire` requires `RETIRE` (selected
//! 2026-09-19, D4): delegable lifecycle control — retirement authorization
//! follows capability permissions, not a privileged owner identity.
//!
//! `Activate` is the translation-context installation step of activation
//! only; until Domain scheduling exists, only the current (caller) Domain may
//! activate itself (`InvalidOperation` otherwise). Conversely, the current
//! Domain may not retire itself (`InvalidOperation`): the invocation must
//! return to a surviving caller. Never-returns self-retirement is wanted as
//! soon as feasible (recorded in the contract) but needs terminal
//! entry-path work. Full Activate/Suspend/Resume — initialized execution
//! contexts, execution budget, legal state transitions, and EL0 entry —
//! remains Phase 7 work (D8); the DCB-update sketch in
//! `Nucleus::activate_domain` records that intent and stays inactive here.

use {
    crate::objects::{ArchObjects, Domain, KeyTable, Nucleus, access::Access},
    libobject::{CapError, ObjectType, RawKey, Rights},
    libqemu::semihosting as semi,
};

/// Handle a `Domain` capability invocation.
///
/// `caller_table_addr` is the caller's own table, through which the invoked
/// `domain_key` is resolved. `Grant` `1`, `Suspend` `2`, and `Resume` `3`
/// remain unsupported operations and fail with `InvalidOperation`; their
/// design intent is recorded in the contract, not implemented here.
pub fn invoke<A: ArchObjects>(
    access: &Access,
    caller_table_addr: u64,
    domain_key: RawKey,
    op: u64,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    match op {
        0 => activate::<A>(access, caller_table_addr, domain_key, args, nucleus),
        4 => retire::<A>(access, caller_table_addr, domain_key, args, nucleus),
        _ => Err(CapError::InvalidOperation),
    }
}

/// `Activate` `0`: install this Domain's bound translation root and ASID as
/// the current hardware translation context.
fn activate<A: ArchObjects>(
    access: &Access,
    caller_table_addr: u64,
    domain_key: RawKey,
    args: &[u64; 6],
    nucleus: &Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    if args.iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }

    // Resolve the invoked Domain capability through the caller's own table,
    // copying out the checked identity.
    let domain_id = {
        let caller_table = access.resolve_carved_mut::<KeyTable>(caller_table_addr)?;
        let entry = caller_table
            .lookup(domain_key)
            .map_err(|e| e.with_key_operand(0))?;
        if entry.object_type() != ObjectType::DOMAIN {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::DOMAIN,
                found: entry.object_type(),
            });
        }
        // Authority over the mapping context.
        if !entry.rights().has(Rights::MAP) {
            return Err(CapError::InsufficientRights);
        }
        entry.object_id().map_err(|e| e.with_key_operand(0))?
    };

    // Bootstrap-era restriction: only the current Domain activates itself.
    // A cross-Domain activation would switch the caller's own hardware
    // context — a scheduling transition that does not exist yet.
    let current = nucleus.current_domain.ok_or(CapError::InvalidDomain)?;
    if u32::from(domain_id.index) != current {
        return Err(CapError::InvalidOperation);
    }

    // Both binding preconditions must hold before any hardware transition:
    // a root without an ASID (or neither) establishes no hardware context.
    let domain = access.resolve::<Domain>(&nucleus.pools.domains, domain_id)?;
    let root = domain.translation_root.ok_or(CapError::NotMapped)?;
    let asid = domain.asid.ok_or(CapError::NotMapped)?;

    // Hardware transition: idempotent installation of the same context.
    A::install_translation_context(root, asid);
    semi::println!("✅ Domain::Activate()");
    Ok((0, 0))
}

/// `Retire` `4` (selected 2026-09-19): tear down the invoked Domain.
///
/// Teardown scope: cancel every pending record naming the Domain as waiter
/// and purge its queued wakeup (`Nucleus::cancel_domain_pending`), then
/// deallocate the Domain-pool slot — the contract's teardown-before-reuse
/// rule. Carved backing (keytable, kernel stack) stays leaked per
/// accepted-leak; a bound ASID stays allocated (ASID release/hardware-safe
/// reuse remains open, D6); the DCB is untouched (D5). Subsequent
/// invocations of the retired Domain's capabilities fail pool validation
/// with a defined error.
fn retire<A: ArchObjects>(
    access: &Access,
    caller_table_addr: u64,
    domain_key: RawKey,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    if args.iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }

    // Resolve the invoked Domain capability through the caller's own table,
    // copying out the checked identity.
    let domain_id = {
        let caller_table = access.resolve_carved_mut::<KeyTable>(caller_table_addr)?;
        let entry = caller_table
            .lookup(domain_key)
            .map_err(|e| e.with_key_operand(0))?;
        if entry.object_type() != ObjectType::DOMAIN {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::DOMAIN,
                found: entry.object_type(),
            });
        }
        // Delegable lifecycle-control authority (selected 2026-09-19, D4).
        if !entry.rights().has(Rights::RETIRE) {
            return Err(CapError::InsufficientRights);
        }
        entry.object_id().map_err(|e| e.with_key_operand(0))?
    };

    // The caller must be a surviving Domain: retiring the current Domain
    // from inside its own invocation has no sound return path yet
    // (never-returns self-retirement is contract-recorded follow-up).
    let current = nucleus.current_domain.ok_or(CapError::InvalidDomain)?;
    if u32::from(domain_id.index) == current {
        return Err(CapError::InvalidOperation);
    }

    // Cancel-then-deallocate: the pending records and queued wakeups go
    // first, then the pool slot is reclaimed. A stale identity (already
    // retired) fails the cancellation's pool validation with a defined
    // error.
    nucleus.cancel_domain_pending(domain_id)?;
    nucleus
        .pools
        .domains
        .deallocate(domain_id)
        .map_err(|e| e.with_key_operand(0))?;
    semi::println!("✅ Domain::Retire()");
    Ok((0, 0))
}

//! `Domain.Activate`: hardware translation-context installation (selected
//! 2026-09-15).
//!
//! Wire schema (see `doc/nucleus_capabilities.md`):
//! - `Activate` `0`: no arguments (`x2..x7` zero). Installs the invoked
//!   Domain's bound translation root into the current hardware translation
//!   context (`TTBR0_EL1` with the bound ASID). Success returns zeros.
//!
//! Authority: `MAP` on the invoked Domain capability (authority over the
//! mapping context, consistent with root installation, frame mapping, and
//! ASID binding). The Domain must have a translation root installed and an
//! ASID bound (`NotMapped` otherwise — no hardware context can be
//! established without both). Until Domain scheduling exists, only the
//! current (caller) Domain may activate itself: switching the caller's own
//! hardware context to a different Domain is a scheduling transition, not
//! this bootstrap-era mechanism (`InvalidOperation` otherwise).
//!
//! This is the translation-context installation step of activation only.
//! Full Activate/Suspend/Resume — initialized execution contexts, execution
//! budget, legal state transitions, and EL0 entry — remains Phase 7 work
//! (D8); the DCB-update sketch in `Nucleus::activate_domain` records that
//! intent and stays inactive here.

use {
    crate::objects::{ArchObjects, Domain, KeyTable, Nucleus, access::Access},
    libobject::{CapError, ObjectType, RawKey, Rights},
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
    nucleus: &Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    match op {
        0 => activate::<A>(access, caller_table_addr, domain_key, args, nucleus),
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
    Ok((0, 0))
}

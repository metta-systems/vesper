//! `ASIDPool.Assign`: capability-protected ASID binding (selected 2026-09-15).
//!
//! Wire schema (see `doc/nucleus_capabilities.md`):
//! - `Assign` `0`: `x2` target Domain key, `x3..x7` zero. Allocates the lowest
//!   free ASID from the invoked pool and binds it to the target Domain's
//!   translation root. Success returns the ASID in `x1` and zero in `x2`.
//!
//! Authority: `GRANT` on the invoked `ASIDPool` capability (issuing a hardware
//! namespace resource), `MAP` on the target Domain capability (authority over
//! the mapping context, consistent with root installation and frame mapping).
//! The target Domain must have a translation root installed (`NotMapped`
//! otherwise) and no ASID yet (`AlreadyMapped` otherwise); pool exhaustion is
//! `ASIDPoolExhausted`. Allocation is the last failing step, so every
//! rejection leaves the pool and the Domain unchanged.
//!
//! ASID pools are boot-provided, not Retype-creatable: ASIDs are a hardware
//! namespace, not memory-backed, so memory authority cannot mint them.

use {
    crate::objects::{
        ArchObjects, Domain, KeyTable, Nucleus, access::Access, arch_objects::AsidPoolObject,
    },
    libobject::{ASIDPoolOp, CapError, ObjectType, RawKey, Rights},
    libqemu::semihosting as semi,
};

/// Handle an `ASIDPool` capability invocation.
///
/// `caller_table_addr` is the caller's own table, through which the invoked
/// `pool_key` and the target Domain key are resolved.
pub fn invoke<A: ArchObjects>(
    access: &Access,
    caller_table_addr: u64,
    pool_key: RawKey,
    op: u64,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    let op = ASIDPoolOp::try_from(op)?;
    match op {
        ASIDPoolOp::Assign => assign::<A>(access, caller_table_addr, pool_key, args, nucleus),
    }
}

/// `Assign` `0`: bind an ASID from this pool to the target Domain's root.
fn assign<A: ArchObjects>(
    access: &Access,
    caller_table_addr: u64,
    pool_key: RawKey,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    let domain_key = RawKey::from_wire(args[0]);
    if args[1] != 0 || args[2] != 0 || args[3] != 0 || args[4] != 0 || args[5] != 0 {
        return Err(CapError::InvalidOperation);
    }

    // Resolve the invoked ASIDPool capability and the target Domain
    // capability through the caller's own table, copying out the checked
    // identities.
    let (pool_id, domain_id) = {
        let caller_table = access.resolve_carved_mut::<KeyTable>(caller_table_addr)?;
        let entry = caller_table
            .lookup(pool_key)
            .map_err(|e| e.with_key_operand(0))?;
        if entry.object_type() != ObjectType::ASID_POOL {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::ASID_POOL,
                found: entry.object_type(),
            });
        }
        // Authority over the ASID namespace resource.
        if !entry.rights().has(Rights::GRANT) {
            return Err(CapError::InsufficientRights);
        }
        let pool_id = entry.object_id().map_err(|e| e.with_key_operand(0))?;
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
        (pool_id, domain_id)
    };

    // Resolve the pool and the target Domain (distinct pools, no alias) and
    // validate the binding preconditions. Allocation is the last failing
    // step; the commit below cannot fail.
    let mut pool =
        access.resolve_mut::<A::ASIDPool>(&mut nucleus.pools.arch.asid_pools, pool_id)?;
    let mut domain = access.resolve_mut::<Domain>(&mut nucleus.pools.domains, domain_id)?;
    if domain.translation_root.is_none() {
        return Err(CapError::NotMapped);
    }
    if domain.asid.is_some() {
        return Err(CapError::AlreadyMapped);
    }
    let asid = pool.allocate().ok_or(CapError::ASIDPoolExhausted)?;

    // Commit: record the binding on the Domain.
    domain.asid = Some(asid);
    semi::println!("✅ ASIDPool::Assign(asid {asid})");
    Ok((u64::from(asid), 0))
}

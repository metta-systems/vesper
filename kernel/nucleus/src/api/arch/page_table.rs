//! `PageTable.Map`/`Unmap`: installation of explicitly managed translation
//! tables (selected 2026-09-15).
//!
//! Wire schemas (see `doc/nucleus_capabilities.md`):
//! - `Map` `0`: `x2` parent key, `x3` virtual address, `x4..x7` zero. The
//!   parent capability's type selects the installation: a `Domain` capability
//!   installs the translation root (the virtual address must be zero; the
//!   Domain capability must carry `MAP`), a `PageTable` capability installs one
//!   intermediate level (the parent must be installed and below the leaf level;
//!   the parent capability must carry `MAP`; the slot selected by the virtual
//!   address must be vacant).
//! - `Unmap` `1`: no arguments. The table must be installed and empty (all
//!   descriptors zero); unmapping the root clears the Domain's translation-root
//!   field.
//!
//! Carved tables are not yet active in any hardware translation context (no
//! Domain context switching yet), so unmap performs no TLB invalidation; this
//! must become a real invalidation when Domain activation installs these tables
//! into a TTBR.

use {
    crate::objects::{
        ArchObjects, Domain, KeyTable, Nucleus,
        access::{Access, ObjectId},
        arch_objects::{PageTableObject, PtParent},
    },
    libobject::{CapError, ObjectType, PageTableOp, RawKey, Rights},
};

/// Handle a `PageTable` capability invocation.
///
/// `caller_table_addr` is the caller's own table, through which the invoked
/// `pt_key` and the parent key are resolved.
pub fn invoke<A: ArchObjects>(
    access: &Access,
    caller_table_addr: u64,
    pt_key: RawKey,
    op: u64,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    let op = PageTableOp::try_from(op)?;
    match op {
        PageTableOp::Map => map::<A>(access, caller_table_addr, pt_key, args, nucleus),
        PageTableOp::Unmap => unmap::<A>(access, caller_table_addr, pt_key, args, nucleus),
    }
}

/// `Map` `0`: install the invoked table into the parent named by `args[0]`.
fn map<A: ArchObjects>(
    access: &Access,
    caller_table_addr: u64,
    pt_key: RawKey,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    let parent_key = RawKey::from_wire(args[0]);
    let vaddr = args[1];
    if args[2] != 0 || args[3] != 0 || args[4] != 0 || args[5] != 0 {
        return Err(CapError::InvalidOperation);
    }

    // Resolve the invoked PageTable capability and the parent capability
    // through the caller's own table, copying out the checked identities.
    let (pt_id, parent_type, parent_id, parent_rights) = {
        let caller_table = access.resolve_carved_mut::<KeyTable>(caller_table_addr)?;
        let entry = caller_table
            .lookup(pt_key)
            .map_err(|e| e.with_key_operand(0))?;
        if entry.object_type() != ObjectType::PAGE_TABLE {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::PAGE_TABLE,
                found: entry.object_type(),
            });
        }
        let pt_id = entry.object_id().map_err(|e| e.with_key_operand(0))?;
        let parent_entry = caller_table
            .lookup(parent_key)
            .map_err(|e| e.with_key_operand(2))?;
        (
            pt_id,
            parent_entry.object_type(),
            parent_entry
                .object_id()
                .map_err(|e| e.with_key_operand(2))?,
            parent_entry.rights(),
        )
    };
    // Authority: the parent capability carries the installation authority.
    if !parent_rights.has(Rights::MAP) {
        return Err(CapError::InsufficientRights);
    }

    match parent_type {
        ObjectType::DOMAIN => {
            // Root installation: the virtual address is meaningless for a
            // whole-context root and must be zero.
            if vaddr != 0 {
                return Err(CapError::InvalidOperation);
            }
            // Distinct pools: the Domain and the page-table metadata object
            // cannot alias.
            let mut domain = access.resolve_mut::<Domain>(&mut nucleus.pools.domains, parent_id)?;
            if domain.translation_root.is_some() {
                return Err(CapError::AlreadyMapped);
            }
            let mut pt =
                access.resolve_mut::<A::PageTable>(&mut nucleus.pools.arch.page_tables, pt_id)?;
            if pt.is_installed() {
                return Err(CapError::AlreadyMapped);
            }
            // Commit: record the root on the Domain, then the installation on
            // the table. Neither step can fail after the checks above.
            domain.translation_root = Some(pt.paddr());
            pt.install_root(parent_id);
            Ok((0, 0))
        }
        ObjectType::PAGE_TABLE => {
            // Intermediate installation: same pool, so the child and parent
            // are resolved as an alias-rejecting pair (mapping a table into
            // itself is rejected, not attempted).
            let (mut pt, parent) = access.resolve_pair_mut::<A::PageTable>(
                &mut nucleus.pools.arch.page_tables,
                pt_id,
                parent_id,
            )?;
            if pt.is_installed() {
                return Err(CapError::AlreadyMapped);
            }
            if !parent.is_installed() {
                return Err(CapError::NotMapped);
            }
            if parent.level() >= 3 {
                // A level-3 table holds page descriptors; no table may be
                // installed beneath it.
                return Err(CapError::InvalidOperation);
            }
            // Hardware transition first: vacancy is checked and the table
            // descriptor written by the arch layer. The returned slot is part
            // of the installation record.
            let slot = A::install_table_entry(parent.paddr(), parent.level(), vaddr, pt.paddr())?;
            // Commit: record the installation. No step after the hardware
            // write can fail.
            pt.install_table(parent.paddr(), parent.level(), slot);
            Ok((0, 0))
        }
        found => Err(CapError::InvalidObjectType(found)),
    }
}

/// `Unmap` `1`: clear the installation of the invoked table.
fn unmap<A: ArchObjects>(
    access: &Access,
    caller_table_addr: u64,
    pt_key: RawKey,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    if args.iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }

    let pt_id = {
        let caller_table = access.resolve_carved_mut::<KeyTable>(caller_table_addr)?;
        let entry = caller_table
            .lookup(pt_key)
            .map_err(|e| e.with_key_operand(0))?;
        if entry.object_type() != ObjectType::PAGE_TABLE {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::PAGE_TABLE,
                found: entry.object_type(),
            });
        }
        entry.object_id().map_err(|e| e.with_key_operand(0))?
    };

    let mut pt = access.resolve_mut::<A::PageTable>(&mut nucleus.pools.arch.page_tables, pt_id)?;
    // A non-empty table would orphan its children; require every descriptor to
    // be zero before clearing the installation.
    if !A::page_table_is_empty(pt.paddr()) {
        return Err(CapError::InvalidOperation);
    }
    match pt.parent() {
        PtParent::Uninstalled => Err(CapError::NotMapped),
        PtParent::Root { domain } => {
            // Distinct pools: the Domain and the table metadata cannot alias.
            let mut domain = access.resolve_mut::<Domain>(&mut nucleus.pools.domains, domain)?;
            // Defensive: the recorded root must match this table.
            if domain.translation_root != Some(pt.paddr()) {
                return Err(CapError::InvalidOperation);
            }
            domain.translation_root = None;
            pt.uninstall();
            Ok((0, 0))
        }
        PtParent::Table { parent_paddr, slot } => {
            // Hardware transition: verify the descriptor still points at this
            // table, then clear it.
            A::clear_table_entry(parent_paddr, slot, pt.paddr())?;
            pt.uninstall();
            Ok((0, 0))
        }
    }
}

// =====================
// == Syscall handler ==
// =====================

#[inline]
pub fn invoke(cap: &KeyEntry, op: u32, arg0: u64, arg1: u64) -> SyscallResult {
    let buffer = cap.as_buffer()?;
    let caller = current_domain();

    match BufferOp::try_from(op)? {
        BufferOp::Map => map(cap, buffer, caller, arg0, arg1),
        BufferOp::Unmap => unmap(buffer, caller),
        BufferOp::Query => query(buffer, caller, arg0),
    }
}

fn map(cap: &KeyEntry, buffer: &Buffer, caller: &Domain, arg0: u64, arg1: u64) -> SyscallResult {
    // Check MAP right
    if !cap.rights.contains(Rights::MAP) {
        return Err(SyscallError::PermissionDenied);
    }

    // Check not already mapped by this domain
    if buffer.is_mapped_by(caller.id) {
        return Err(SyscallError::AlreadyMapped);
    }

    let hint = if arg0 != 0 {
        Some(VirtAddr::new(arg0))
    } else {
        None
    };
    let flags = MapFlags::from_bits_truncate(arg1 as u32);

    // Allocate virtual address range
    let vaddr =
        caller
            .address_space
            .allocate_range(hint, buffer.size, flags.contains(MapFlags::FIXED))?;

    // Compute page table permissions from cap rights
    let pte_flags = cap.rights.to_pte_flags() | buffer.flags.to_pte_flags();

    // Install page table mappings
    for offset in (0..buffer.size).step_by(PAGE_SIZE) {
        let paddr = buffer.phys_base + offset;
        let vaddr_page = vaddr + offset;

        caller
            .address_space
            .map_page(vaddr_page, paddr, pte_flags)?;
    }

    // Record mapping for revocation
    buffer.mappings.push(Mapping {
        domain_id: caller.id,
        virt_addr: vaddr,
        permissions: cap.rights,
    });

    Ok(vaddr.as_u64())
}

fn unmap(buffer: &mut Buffer, caller: &Domain) -> SyscallResult {
    // Find and remove mapping for this domain
    let mapping = buffer
        .mappings
        .iter()
        .position(|m| m.domain_id == caller.id)
        .ok_or(SyscallError::NotMapped)?;

    let mapping = buffer.mappings.remove(mapping);

    // Remove page table entries
    for offset in (0..buffer.size).step_by(PAGE_SIZE) {
        caller
            .address_space
            .unmap_page(mapping.virt_addr + offset)?;
    }

    // TLB invalidation (in single-address-space, this is local)
    tlb_invalidate_range(mapping.virt_addr, buffer.size);

    Ok(0)
}

fn query(buffer: &Buffer, caller: &Domain, arg0: u64) -> SyscallResult {
    let info_ptr = arg0 as *mut BufferInfo;

    // Validate user pointer
    if !caller.address_space.is_valid_user_ptr(info_ptr) {
        return Err(SyscallError::InvalidPointer);
    }

    let mapping = buffer.mappings.iter().find(|m| m.domain_id == caller.id);

    let info = BufferInfo {
        size: buffer.size,
        flags: buffer.flags,
        is_mapped: mapping.is_some(),
        mapped_addr: mapping.map(|m| m.virt_addr),
    };

    unsafe {
        info_ptr.write(info);
    }

    Ok(0)
}

#[repr(u8)]
pub enum VSpaceOp {
    /// Assign root page table
    SetRoot = 0,
    /// Assign ASID
    AssignASID = 1,
    /// Activate (switch to this address space)
    Activate = 2,
    /// Get current ASID
    GetASID = 3,
}

pub fn invoke<A: ArchObjects>(
    vspace: &mut A::VSpace,
    rights: Rights,
    op: u32,
    args: &[u64; 6],
    kernel: &mut Kernel<A>,
) -> Result<(u64, u64), CapError> {
    let op = VSpaceOp::try_from(op as u8).map_err(|_| CapError::InvalidOperation)?;

    match op {
        VSpaceOp::SetRoot => {
            // args[0] = page_table_slot
            todo!("vspace set_root")
        }
        VSpaceOp::AssignASID => {
            // args[0] = asid_pool_slot
            let pool_slot = KeySlot(args[0] as u16);

            let domain = kernel.current_domain_mut()?;
            let pool_entry = domain.keytable.lookup_mut(pool_slot)?;
            let pool = pool_entry.as_object_mut::<AArch64ASIDPool>()?;

            let asid = pool.allocate().ok_or(CapError::ASIDPoolExhausted)?;

            vspace.asid = Some(asid);
            Ok((asid as u64, 0))
        }
        VSpaceOp::Activate => {
            todo!("vspace activate")
        }
        VSpaceOp::GetASID => {
            let asid = vspace.asid.ok_or(CapError::NoASIDAssigned)?;
            Ok((asid as u64, 0))
        }
    }
}

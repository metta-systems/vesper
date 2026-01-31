// =====================
// == Syscall handler ==
// =====================

pub fn invoke(cap: &Cap, op: u32, arg0: u64, arg1: u64) -> SyscallResult {
    let domain = cap.as_domain()?;
    match op {
        DomainOp::Activate => {
            // Make domain runnable (usually combined with TimeCap donation)
            domain.activate()
        }
        DomainOp::Grant => {
            // Grant a capability to this domain's cspace
            let src_slot = arg0 as CapSlot;
            let dest_slot = arg1 as CapSlot;
            domain.grant_cap(src_slot, dest_slot)
        }
        DomainOp::Suspend => domain.suspend(),
        DomainOp::Resume => domain.resume(),
        _ => Err(SyscallError::InvalidOp),
    }
}

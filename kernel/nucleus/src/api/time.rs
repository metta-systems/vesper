// =====================
// == Syscall handler ==
// =====================

pub fn invoke(key: &TimeKey, op: u32, args: [u64; 4]) -> SyscallResult {
    match op {
        TimeOp::Donate => {
            let target = args[0] as KeySlot;
            let target_domain = lookup_domain_key(target)?;
            // Transfer time + switch to target
            key.donate(target_domain) // activate_domain
        }
        TimeOp::Split => {
            let amount_us = args[0];
            // Create new TimeCap with 'amount_us'
            // Reduce current cap by same
            key.split(amount_us)
        }
        TimeOp::Merge => {
            let other = args[0] as CapSlot;
            key.merge(other)
        }
        TimeOp::Query => Ok([key.remaining_us, 0]),
        _ => Err(SyscallError::InvalidOp),
    }
}

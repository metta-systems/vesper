use libobject::key_table::KeyTableOp;

// =====================
// == Syscall handler ==
// =====================

pub fn invoke(key: &Key, op: u32, args: &[u64]) -> SyscallResult {
    let captbl = key.as_keytable()?;
    match KeyTableOp::try_from(op)? {
        KeyTableOp::CopyDerive => {
            let (src, dst_captbl, dst_slot) = (args[0], args[1], args[2]);
            // Copy cap from this captbl[src] to dst_captbl[dst_slot]
            //...
        }
        KeyTableOp::Move => {
            // Copy cap from captbl[src] to dst_captbl[dst_slot]
            // Delete cap in captbl[src]
        }
        KeyTableOp::Delete => {
            // Delete cap in captbl[src]
        }
        KeyTableOp::Revoke => {
            // Revoke cap, by bumping it's epoch and making derived accesses invalid
        }
    }
}

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Time capability — represents a slice of CPU time
pub struct TimeKey {
    key: Key<Time>,
}

impl TimeKey {
    /// Donate this time slice to target domain.
    /// Current domain blocks until target exhausts time or yields.
    pub fn donate(self, target: DomainKey) -> Result<(), SchedError> {
        let (ret, _) = unsafe {
            protected_call1(
                self.key.slot as u64,
                TimeOp::Donate as u64,
                target.key.slot as u64,
            )
        };
        // Note: self is consumed (moved)
        Error::from_code(ret)
    }

    /// Split off 'amount_us' into a new TimeCap.
    /// Self retains the remainder.
    pub fn split(&mut self, amount_us: u64) -> Result<TimeKey, SchedError> {
        let new_slot = self.alloc_slot()?;
        let (ret, _) = unsafe {
            protected_call2(
                self.key.slot as u64,
                TimeOp::Split as u64,
                amount_us,
                new_slot as u64,
            )
        };
        if ret == 0 {
            Ok(TimeKey {
                key: Key::new(new_slot),
            })
        } else {
            Err(SchedError::from_code(ret))
        }
    }

    /// Query remaining time in microseconds.
    pub fn remaining(&self) -> u64 {
        let (us, _) = unsafe { protected_call0(self.key.slot as u64, TimeOp::Query as u64) };
        us
    }
}

// Yield is just dropping + returning to scheduler
impl Drop for TimeKey {
    fn drop(&mut self) {
        // Kernel: return remaining time to parent in derivation tree
        unsafe {
            protected_call1(CAPTBL_SELF, KeyTableOp::Delete, self.key.slot as u64);
        }
    }
}

// ==============================================
// == Kernel space object and syscall handling ==
// ==============================================

struct Time {
    remaining_us: u64,   // Microseconds left
    deadline: Instant,   // When this slice expires
    parent: Option<Key>, // For custom revocation tree (to easily return unused time to parent)
}

enum TimeOp {
    Donate = 0,
    Split = 1,
    Merge = 2,
    Query = 3,
}

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

impl NucleusObject for TimeSlice {
    const TYPE: ObjectType = ObjectType::Time;
}

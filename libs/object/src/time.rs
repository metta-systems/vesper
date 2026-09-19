// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Time capability — represents a slice of CPU time
pub struct TimeKey {
    key: Key<TimeType>,
}

enum TimeType {}

#[repr(u8)]
pub enum TimeOp {
    Donate = 0,
    Split = 1,
    Merge = 2,
    Query = 3,
}

impl TimeKey {
    /// Donate this time slice to target domain.
    /// Current domain blocks until target exhausts time or yields.
    pub fn donate(self, target: DomainKey) -> Result<(), SchedError> {
        let (ret, _, _) =
            unsafe { protected_call1(self.key.slot(), TimeOp::Donate as u64, target.key.slot()) };
        // Note: self is consumed (moved) - FIXME
        Error::from_code(ret)
    }

    /// Split off 'amount_us' into a new TimeCap.
    /// Self retains the remainder.
    pub fn split(&mut self, amount_us: u64) -> Result<TimeKey, SchedError> {
        let new_slot = self.alloc_slot()?; // FIXME: In-kernel memory allocation?
        let (ret, _, _) = unsafe {
            protected_call2(
                self.key.slot(),
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
        let (_, us, _) = unsafe { protected_call0(self.key.slot(), TimeOp::Query as u64) };
        us
    }
}

// Yield is just dropping + returning to scheduler
impl Drop for TimeKey {
    fn drop(&mut self) {
        // Kernel: return remaining time to parent in derivation tree
        unsafe {
            protected_call1(KeySlot::CAPTBL_SELF, KeyTableOp::Delete, self.key.slot());
        }
    }
}

// Reed-Kanodia event counts

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Event count capability - monotonic counter for exact event tracking.
/// Best for: streaming, flow control, producer-consumer coordination.
///
/// Unlike notifications, every advance() is counted - no coalescing.
/// This lets consumers know exactly how far behind they are.
pub struct EventCountKey {
    key: Key<EventCount>,
}

impl EventCountCap {
    /// Advance: atomically increment counter by delta.
    /// Returns new value. Never blocks. ~30 cycles.
    #[inline]
    pub fn advance(&self, delta: u64) -> u64 {
        protected_call1(self.key.slot as u64, EventOp::Advance as u64, delta)
    }

    /// Await: block until value >= target.
    /// Returns current value (may be > target if producer is fast).
    #[inline]
    pub fn await_ge(&self, target: u64) -> u64 {
        protected_call1(self.key.slot as u64, EventOp::Await as u64, target)
    }

    /// Read: get current value without blocking.
    #[inline]
    pub fn read(&self) -> u64 {
        protected_call0(self.key.slot as u64, EventOp::Read)
    }
}

/// Helper: tracks consumer position for a single reader
pub struct EventCountReader {
    ec: EventCountKey,
    last_seen: u64,
}

impl EventCountReader {
    pub fn new(ec: EventCountKey) -> Self {
        let initial = ec.read();
        Self {
            ec,
            last_seen: initial,
        }
    }

    /// Wait for next event(s), returns count since last wait
    pub fn wait_next(&mut self) -> u64 {
        let target = self.last_seen + 1;
        let current = self.ec.await_ge(target);
        let delta = current - self.last_seen;
        self.last_seen = current;
        delta
    }

    /// Check how many events pending without blocking
    pub fn pending(&self) -> u64 {
        self.ec.read() - self.last_seen
    }
}

// ==============================================
// == Kernel space object and syscall handling ==
// ==============================================

// Kernel object
struct EventCount {
    value: u64,         // Monotonically increasing counter
    waiters: WaitQueue, // Domains waiting for value >= target
}

#[repr(u8)]
enum EventCountOp {
    Advance = 0,
    Await = 1,
    Read = 2,
}

// =====================
// == Syscall handler ==
// =====================

pub fn invoke(cap: &Cap, op: u32, arg0: u64) -> SyscallResult {
    let ec = cap.as_event_count()?;
    match op {
        EventOp::Advance => {
            Ok(ec.advance(arg0)) // atomic ADD, returns new value
        }
        EventOp::Await => {
            Ok(ec.await_ge(arg0)) // blocks until >= arg0
        }
        EventOp::Read => {
            Ok(ec.read()) // non-blocking read
        }
        _ => Err(SyscallError::InvalidOp),
    }
}

impl KernelObject for EventCount {
    const TYPE: ObjectType = ObjectType::EventCount;
}

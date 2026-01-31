// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Event count capability - monotonic Reed-Kanodia counter for exact event tracking.
/// Best for: streaming, flow control, producer-consumer coordination.
///
/// Unlike notifications, every advance() is counted - no coalescing.
/// This lets consumers know exactly how far behind they are.
pub struct EventCountKey {
    key: Key<EventCountType>,
}

enum EventCountType {}

#[repr(u8)]
pub enum EventCountOp {
    Advance = 0,
    Await = 1,
    Read = 2,
}

impl EventCountKey {
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

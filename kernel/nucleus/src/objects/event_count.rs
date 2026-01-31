// ====================
// == Nucleus object ==
// ====================

struct EventCount {
    value: u64,         // Monotonically increasing counter
    waiters: WaitQueue, // Domains waiting for value >= target
}

impl NucleusObject for EventCount {
    const TYPE: ObjectType = ObjectType::EventCount;
}

// ====================
// == Nucleus object ==
// ====================

struct Time {
    remaining_us: u64,   // Microseconds left
    deadline: Instant,   // When this slice expires
    parent: Option<Key>, // For custom revocation tree (to easily return unused time to parent)
}

impl NucleusObject for TimeSlice {
    const TYPE: ObjectType = ObjectType::Time;
}

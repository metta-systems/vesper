// ====================
// == Nucleus object ==
// ====================

struct Notification {
    state: u64,         // Bitmap
    waiters: WaitQueue, // Blocked domains
    bound: Option<Key>, // Optional bound domain for fast wakeup
}

impl Notification {
    fn signal(&mut self, bits: u64) {}
    fn wait() {
        // if bits are already set, clear and immediately return
        // otherwise block the domain..
    }
    fn poll() {}
}

impl NucleusObject for Notification {
    const TYPE: ObjectType = ObjectType::Notification;
}

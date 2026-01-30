use core::sync::atomic::Ordering;

// ====================
// == Nucleus object ==
// ====================

struct Domain;

impl Domain {
    // Initialize new domain's cspace
    fn init_cspace(&mut self) {
        // Slot 0: capability to this captbl itself
        self.cspace[CAPTBL_SELF] = Cap::new(ObjectType::KeyTable, self.cspace_id);
        // Now domain can manipulate its own caps
    }
}

#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DomainState {
    /// Just created, never run
    Inactive = 0,
    /// Ready to receive CPU time
    Runnable = 1,
    /// Currently executing (only one domain per CPU)
    Running = 2,
    /// Waiting on notification/event/endpoint
    Blocked = 3,
    /// Explicitly suspended by parent
    Suspended = 4,
    /// Faulted, needs handler
    Faulted = 5,
}

#[repr(u32)]
#[derive(Copy, Clone, Debug)]
pub enum BlockReason {
    None = 0,
    Notification = 1, // NotifyCap::wait()
    EventCount = 2,   // EventCountCap::await_ge()
    Endpoint = 3,     // EndpointCap::call() or recv()
    TimeDonated = 4,  // Donated time, waiting for return
}

// pub enum DeactivateReason {
//     TimeExhausted,
//     BlockedOnEvent(EndpointCap), <-- BlockReason::Endpoint
//     Yielded,
//     Faulted(fault),
// }

/// Domain Control Block
/// Our DCB structure - combining Nemesis ideas with our capability model
///
/// Key insight: split into kernel-private and shared sections
#[repr(C, align(128))] // Cache-line aligned
pub struct DomainControlBlock {
    // ═══════════════════════════════════════════════════════════
    // SHARED SECTION (read-only mapped to userspace)
    // ═══════════════════════════════════════════════════════════
    // ─── Identity ───
    pub id: DomainId,
    pub name: [u8; 24],

    // ─── Execution State (Acquire/Release on state field) ───
    pub state: AtomicU32,        // DomainState
    pub block_reason: AtomicU32, // BlockReason (if blocked)
    pub blocked_on: AtomicU32,   // Cap slot we're blocked on

    // ─── Time Accounting (QoS) ───
    /// Cumulative CPU time consumed (nanoseconds)
    pub time_used_ns: AtomicU64,
    /// Time remaining in current activation
    pub time_remaining_ns: AtomicU64,
    /// Number of times activated
    pub activation_count: AtomicU64,

    // ─── Event State ───
    pub pending_notifications: AtomicU64, // Bitmap of pending notify caps
    /// Number of pending events (sum across all endpoints)
    pub pending_events: AtomicU32, // Number of event counts with data

    /// Endpoint that caused last wakeup
    pub last_event_ep: AtomicU32,

    // ─── Scheduling Parameters ───
    /// Parent scheduler domain
    pub scheduler: DomainId,
    /// Scheduling priority/parameters
    pub priority: u32,
    /// Allocation period (for periodic domains)
    pub period_ns: u64,
    /// CPU allocation per period
    pub budget_ns: u64, // slice_ns

    /// Scheduled deadline (absolute time)
    pub deadline: AtomicU64,

    // ─── Fault Information ───
    /// Last fault type (if any)
    pub fault_type: AtomicU32,

    /// Fault address
    pub fault_addr: AtomicU64,

    pub fault_cap: AtomicU32, // Cap slot that caused fault

    // Padding to cache line
    _pad: [u8; 16],
    // ═══════════════════════════════════════════════════════════
    // PRIVATE SECTION (kernel only, NOT mapped to userspace)
    // ═══════════════════════════════════════════════════════════
    //
    // This would be in a separate structure or after a page boundary
    // - Saved register context
    // - Capability space root
    // - Kernel stack pointer
    // - Etc.
}

// Verify size for cache alignment
const _: () = assert!(core::mem::size_of::<DomainControlBlock>() == 128);

// 32 DCBs per 4KB page
// Multiple pages for more domains

// Userspace sees: const DCB_BASE: *const DomainControlBlock = 0xFFFF_0000_0000_0000; // FIXME: pervasives
// Access DCB n:   &*DCB_BASE.add(n)

/// Userspace view of DCB array
/// Mapped read-only at a well-known address
pub struct DcbView {
    base: *const DomainControlBlock,
}

impl DcbView {
    /// Get from well-known address (set up by kernel at domain creation)
    pub const fn new() -> Self {
        Self {
            base: 0xFFFF_0000_0000_0000 as *const DomainControlBlock,
        }
    }

    /// Read any domain's state
    #[inline(always)]
    pub fn get(&self, id: DomainId) -> &DomainControlBlock {
        unsafe { &*self.base.add(id.0 as usize) }
    }

    /// Get my own DCB
    #[inline(always)]
    pub fn myself(&self) -> &DomainControlBlock {
        // Current domain ID stored in thread-local or well-known register
        self.get(current_domain_id())
    }
}

// Domain scheduling support in kernel:
impl Nucleus {
    /// Called when domain is activated (receives CPU time)
    fn activate_domain(&mut self, id: DomainId, time_budget: u64) {
        let dcb = self.dcb_mut(id);

        dcb.state
            .store(DomainState::Running as u32, Ordering::Release);
        dcb.time_remaining_ns.store(time_budget, Ordering::Release);
        dcb.deadline.store(now() + time_budget, Ordering::Release);
    }

    /// Called on every context switch FROM this domain
    fn deactivate_domain(&mut self, id: DomainId, reason: DeactivateReason) {
        let dcb = self.dcb_mut(id);
        let elapsed = /* calculate from timer */0;

        // Update time accounting
        dcb.time_used_ns.fetch_add(elapsed, Ordering::Relaxed);
        dcb.time_remaining_ns.fetch_sub(elapsed, Ordering::Relaxed);

        // Update state
        match reason {
            DeactivateReason::TimeExhausted => {
                dcb.state
                    .store(DomainState::Runnable as u32, Ordering::Release);
            }
            DeactivateReason::BlockedOnEvent(ep) => {
                dcb.state
                    .store(DomainState::Blocked as u32, Ordering::Release);
                dcb.block_reason
                    .store(BlockReason::Event as u32, Ordering::Release);
                dcb.last_event_ep.store(ep, Ordering::Release);
            }
            DeactivateReason::Yielded => {
                dcb.state
                    .store(DomainState::Runnable as u32, Ordering::Release);
            }
            DeactivateReason::Faulted(fault) => {
                dcb.state
                    .store(DomainState::Faulted as u32, Ordering::Release);
                dcb.fault_type.store(fault.type_code(), Ordering::Release);
                dcb.fault_addr.store(fault.addr(), Ordering::Release);
            }
        }
    }

    /// Called when event arrives for blocked domain
    fn signal_domain(&mut self, id: DomainId) {
        let dcb = self.dcb_mut(id);

        dcb.pending_events.fetch_add(1, Ordering::Release);

        // If blocked on events, make runnable
        if dcb.state.load(Ordering::Acquire) == DomainState::Blocked as u32 {
            dcb.state
                .store(DomainState::Runnable as u32, Ordering::Release);
        }
    }
}

// ## Memory Ordering Considerations

//      KERNEL (writer)                    USERSPACE (reader)
//      ───────────────                    ──────────────────

//      // Update multiple fields
//      dcb.time_used.store(x, Relaxed);
//      dcb.time_remaining.store(y, Relaxed);
//      dcb.state.store(z, Release);  ──────────────────────┐
//                         │                                │
//                         │ Release ensures all            │
//                         │ prior writes visible           │
//                         ▼                                ▼
//                                         let state = dcb.state.load(Acquire);
//                                         // Acquire ensures we see
//                                         // all writes before the Release
//                                         let used = dcb.time_used.load(Relaxed);
//                                         let rem = dcb.time_remaining.load(Relaxed);

//      Protocol:
//      - Kernel does Release store on state LAST
//      - Userspace does Acquire load on state FIRST
//      - Then can safely read other fields with Relaxed

impl NucleusObject for Domain {
    const TYPE: ObjectType = ObjectType::Domain;
}

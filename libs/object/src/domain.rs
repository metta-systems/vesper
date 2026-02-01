use {
    crate::{CapError, Key, KeySlot},
    core::sync::atomic::{AtomicU32, AtomicU64, Ordering},
    libmemory::virt_addr::VirtAddr,
    libsyscall::{protected_call0, protected_call2},
};

// ┌─────────────────────────────────────────────────────────────────────┐
// │                    DCB SHARED PAGES ARCHITECTURE                    │
// ├─────────────────────────────────────────────────────────────────────┤
// │                                                                     │
// │  The DCB pages are the Nemesis-inspired mechanism for zero-syscall  │
// │  domain state queries. The kernel maintains DCBs for all domains,   │
// │  mapped read-only into user space.                                  │
// │                                                                     │
// │  KERNEL VIEW (RW)                      USER VIEW (RO)               │
// │  ────────────────                      ───────────────              │
// │                                                                     │
// │  0xFFFF_0000_xxxx_xxxx                 0x7FFF_00xx_xxxx_xxxx        │
// │  (kernel linear map)                   (user mapping)               │
// │        │                                     │                      │
// │        │    ┌─────────────────────┐          │                      │
// │        └───►│  Physical DCB Page  │◄─────────┘                      │
// │             │  ┌───────────────┐  │                                 │
// │             │  │ DCB[0] 128B   │  │                                 │
// │             │  ├───────────────┤  │                                 │
// │             │  │ DCB[1] 128B   │  │                                 │
// │             │  ├───────────────┤  │                                 │
// │             │  │ DCB[2] 128B   │  │                                 │
// │             │  ├───────────────┤  │                                 │
// │             │  │ ...           │  │                                 │
// │             │  ├───────────────┤  │                                 │
// │             │  │ DCB[31] 128B  │  │                                 │
// │             │  └───────────────┘  │                                 │
// │             └─────────────────────┘                                 │
// │                    4KB page                                         │
// │                    32 DCBs per page                                 │
// │                                                                     │
// └─────────────────────────────────────────────────────────────────────┘

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Domain identifier
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct DomainId(pub u32);

impl DomainId {
    pub const INVALID: Self = Self(u32::MAX);

    /// Get the page index for this domain's DCB
    #[inline]
    pub const fn page_index(&self) -> usize {
        (self.0 / DcbPage::DCBS_PER_PAGE) as usize
    }

    /// Get the slot within the page
    #[inline]
    pub const fn slot_in_page(&self) -> usize {
        (self.0 % DcbPage::DCBS_PER_PAGE) as usize
    }
}

#[repr(u8)]
pub enum DomainOp {
    Activate = 0, // Make domain runnable
    Grant = 1,    // Grant capability to domain
    Suspend = 2,  // Suspend domain
    Resume = 3,   // Resume suspended domain
}

/// Domain capability - handle to a protection domain.
/// State queries use shared DCB (no syscall), mutations use CapInvoke.
pub struct DomainKey {
    key: Key<DomainType>,
    id: DomainId,
}

enum DomainType {}

impl DomainKey {
    /// Create a new domain from untyped memory.
    /// Convenience wrapper around UntypedRetype.
    // pub fn create(untyped: &mut UntypedCap, dest_slot: KeySlot) -> Result<Self, Error> {
    //     // Domains need ~4KB (12 bits) for kernel structures
    //     untyped.retype(
    //         untyped.split(12)?, // Carve off 4KB
    //         ObjectType::Domain,
    //         12,
    //         dest_slot,
    //     )?;
    //
    //     // Domain ID is returned in secondary return value
    //     // (or we query it from the newly created DCB)
    //     Ok(DomainKey {
    //         cap: Cap::new(dest_slot),
    //         id: DomainId(0),
    //     })
    // }

    /// Get domain state from shared DCB
    #[inline]
    pub fn state(&self) -> DomainState {
        let dcb_view = unsafe { DcbView::from_user_mapping() };
        let dcb = dcb_view.get(self.id).expect("oh well");
        DomainState::try_from(dcb.state.load(Ordering::Acquire)).unwrap_or(DomainState::Inactive)
    }

    /// Get time used from shared DCB
    #[inline]
    pub fn time_used_ns(&self) -> u64 {
        let dcb_view = unsafe { DcbView::from_user_mapping() };
        let dcb = dcb_view.get(self.id).expect("oh well");
        dcb.time_consumed_ns.load(Ordering::Relaxed)
    }

    /// Get pending notifications from shared DCB (NO SYSCALL!)
    #[inline]
    pub fn pending_notifications(&self) -> u64 {
        let dcb_view = unsafe { DcbView::from_user_mapping() };
        let dcb = dcb_view.get(self.id).expect("oh well");
        dcb.pending_notifications.load(Ordering::Relaxed)
    }

    /// Activate domain (make runnable) - requires syscall
    pub fn activate(&self) -> Result<(), CapError> {
        let (ok, _, _) = unsafe { protected_call0(self.key.slot(), DomainOp::Activate as u32) };
        match ok {
            0 => Ok(()),
            _ => Err(CapError::Unknown),
        }
    }

    /// Grant a capability to this domain
    pub fn grant<T>(&self, key: &Key<T>, dest_slot: KeySlot) -> Result<(), CapError> {
        let (ok, _, _) = unsafe {
            protected_call2(
                self.key.slot(),
                DomainOp::Grant as u32,
                key.slot() as u64,
                dest_slot.0 as u64,
            )
        };
        match ok {
            0 => Ok(()),
            _ => Err(CapError::Unknown),
        }
    }

    /// Suspend domain - requires syscall
    pub fn suspend(&self) -> Result<(), CapError> {
        let (ok, _, _) = unsafe { protected_call0(self.key.slot(), DomainOp::Suspend as u32) };
        match ok {
            0 => Ok(()),
            _ => Err(CapError::Unknown),
        }
    }

    /// Resume suspended domain - requires syscall
    pub fn resume(&self) -> Result<(), CapError> {
        let (ok, _, _) = unsafe { protected_call0(self.key.slot(), DomainOp::Resume as u32) };
        match ok {
            0 => Ok(()),
            _ => Err(CapError::Unknown),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// DOMAIN CONTROL BLOCK (DCB)
// ═══════════════════════════════════════════════════════════════════

/// Domain execution state
#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DomainState {
    /// Just created, never activated
    Inactive = 0,
    /// Ready to run, waiting for CPU time
    Runnable = 1,
    /// Currently executing on a CPU
    Running = 2,
    /// Blocked waiting on IPC/notification/event
    Blocked = 3,
    /// Explicitly suspended by parent
    Suspended = 4,
    /// Faulted, needs handler
    Faulted = 5,
    /// Being destroyed
    Dying = 6,
}

/// Why a domain is blocked
#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BlockReason {
    None = 0,
    Notification = 1, // Waiting on NotificationKey::wait()
    EventCount = 2,   // Waiting on EventCountKey::await_ge()
    EndpointSend = 3, // Blocked on EndpointKey::call() send phase
    EndpointRecv = 4, // Blocked on EndpointKey::recv()
    Reply = 5,        // Waiting for reply after call()
    TimeDonation = 6, // Donated time, waiting for return
}

// pub enum DeactivateReason {
//     TimeExhausted,
//     BlockedOnEvent(EndpointCap), <-- BlockReason::Endpoint
//     Yielded,
//     Faulted(fault),
// }

// ═══════════════════════════════════════════════════════════
// SHARED SECTION (read-only mapped to userspace)
// ═══════════════════════════════════════════════════════════

/// Domain Control Block - shared between kernel and userspace.
/// This is a user-visible half of domain state structure.
///
/// This is the Nemesis-inspired design for zero-syscall state queries.
/// The kernel writes, userspace reads.
///
/// Size: 128 bytes (cache-line aligned, 32 per 4KB page)
#[repr(C, align(128))]
pub struct DomainControlBlock {
    // ─── Identity ───
    /// Domain ID
    pub id: DomainId,
    /// Domain name (for debugging)
    pub name: [u8; 24],

    // ─── Execution State ───
    /// Current state (use Acquire ordering when reading)
    pub state: AtomicU32, // DomainState
    /// Why blocked (valid when state == Blocked)
    pub block_reason: AtomicU32, // BlockReason (if blocked)
    /// Key slot we're blocked on (e.g., which notification)
    pub blocked_on_slot: AtomicU32, // Cap slot we're blocked on
    /// CPU this domain is running on (or was last running on)
    pub cpu: AtomicU32,

    // ─── Time Accounting (QoS) ───
    /// Total CPU time consumed (nanoseconds)
    pub time_consumed_ns: AtomicU64,
    /// Remaining time in current activation (nanoseconds)
    pub time_remaining_ns: AtomicU64,
    /// Number of times this domain has been activated
    pub activation_count: AtomicU64,
    /// Timestamp of last activation (for profiling)
    pub last_activated_ns: AtomicU64,

    // ─── Scheduling Parameters ───
    /// Parent scheduler domain
    pub scheduler_id: DomainId,
    /// Priority (higher = more important)
    pub priority: u32,
    /// Scheduling flags
    pub sched_flags: u32,
    /// Period for periodic domains (nanoseconds, 0 = aperiodic)
    pub period_ns: u64,
    // CPU allocation per period
    // pub budget_ns: u64, // slice_ns
    // Scheduled deadline (absolute time)
    // pub deadline: AtomicU64,

    // ─── Event State ───
    /// Bitmap of pending notification slots
    pub pending_notifications: AtomicU64,
    /// Count of event counts with pending events
    pub pending_event_counts: AtomicU32,
    /// Count of pending endpoint messages
    pub pending_endpoints: AtomicU32,
    // Endpoint that caused last wakeup
    // pub last_event_ep: AtomicU32,

    // ─── Fault Information ───
    /// Fault type (valid when state == Faulted)
    pub fault_type: AtomicU32,
    /// Fault-specific code
    pub fault_code: AtomicU32,
    /// Fault address (for page faults)
    pub fault_addr: AtomicU64,
    /// Key slot that caused fault (for cap faults)
    pub fault_slot: AtomicU32,
}

// Compile-time size check
// TODO const _: () = assert!(core::mem::size_of::<DomainControlBlock>() == 128);
// TODO const _: () = assert!(core::mem::align_of::<DomainControlBlock>() == 128);

impl DomainControlBlock {
    /// Create a new DCB for a domain
    pub const fn new(id: DomainId, scheduler_id: DomainId) -> Self {
        Self {
            id,
            scheduler_id,
            name: [0; 24],
            state: AtomicU32::new(DomainState::Inactive as u32),
            block_reason: AtomicU32::new(BlockReason::None as u32),
            blocked_on_slot: AtomicU32::new(0),
            cpu: AtomicU32::new(0),
            time_consumed_ns: AtomicU64::new(0),
            time_remaining_ns: AtomicU64::new(0),
            activation_count: AtomicU64::new(0),
            last_activated_ns: AtomicU64::new(0),
            priority: 0,
            sched_flags: 0,
            period_ns: 0,
            pending_notifications: AtomicU64::new(0),
            pending_event_counts: AtomicU32::new(0),
            pending_endpoints: AtomicU32::new(0),
            fault_type: AtomicU32::new(0),
            fault_code: AtomicU32::new(0),
            fault_addr: AtomicU64::new(0),
            fault_slot: AtomicU32::new(0),
        }
    }

    /// Set domain name (truncated to 8 bytes)
    pub fn set_name(&mut self, name: &str) {
        let bytes = name.as_bytes();
        let len = bytes.len().min(8);
        self.name[..len].copy_from_slice(&bytes[..len]);
        self.name[len..].fill(0);
    }

    /// Get domain name as string slice
    pub fn name_str(&self) -> &str {
        let len = self.name.iter().position(|&b| b == 0).unwrap_or(8);
        core::str::from_utf8(&self.name[..len]).unwrap_or("???")
    }
}

// ═══════════════════════════════════════════════════════════════════
// DCB PAGE
// ═══════════════════════════════════════════════════════════════════

/// A single 4KB page containing 32 DCBs.
#[repr(C, align(4096))]
pub struct DcbPage {
    dcbs: [DomainControlBlock; Self::DCBS_PER_PAGE as usize],
}

impl DcbPage {
    /// Number of DCBs per 4KB page (128 bytes each)
    pub const DCBS_PER_PAGE: u32 = 4096 / 128; // = 32

    /// Create a new page with uninitialized DCBs
    pub const fn new() -> Self {
        // This is a bit ugly but works at const time
        const INIT_DCB: DomainControlBlock =
            DomainControlBlock::new(DomainId::INVALID, DomainId::INVALID);
        Self {
            dcbs: [INIT_DCB; Self::DCBS_PER_PAGE as usize],
        }
    }

    /// Get a DCB by slot index
    #[inline]
    pub fn get(&self, slot: usize) -> Option<&DomainControlBlock> {
        self.dcbs.get(slot)
    }

    /// Get a mutable DCB by slot index
    #[inline]
    pub fn get_mut(&mut self, slot: usize) -> Option<&mut DomainControlBlock> {
        self.dcbs.get_mut(slot)
    }
}

// TODO const _: () = assert!(core::mem::size_of::<DcbPage>() == 4096);

// ═══════════════════════════════════════════════════════════════════
// DCB VIEW
// Userspace sees: const DCB_BASE: *const DomainControlBlock = DcbPages::USER_BASE; // FIXME: pervasives
// Access DCB n:   &*DCB_BASE.add(n)
// ═══════════════════════════════════════════════════════════════════

/// User-space view of DCB pages (read-only).
///
/// This is used by schedulers and other user-space code to query
/// domain state without syscalls.
pub struct DcbView {
    base: *const DomainControlBlock,
}

impl DcbView {
    /// Create from the well-known user-space address
    ///
    /// # Safety
    /// Must only be called after kernel has set up the mapping
    pub const unsafe fn from_user_mapping() -> Self {
        // FIXME: Duplicate DcbPages::USER_BASE const from nucleus/objects/domain.rs here, keep in sync!
        const USER_BASE: VirtAddr = VirtAddr::new_unchecked(0x0000_7FFF_FE00_0000);
        Self {
            base: USER_BASE.as_ptr() as *const DomainControlBlock,
        }
    }

    /// Get a DCB by domain ID (read-only)
    ///
    /// Returns None if the domain ID is invalid or not allocated.
    /// Note: We can't actually check allocation status from user-space,
    /// so we just check if the DCB looks valid.
    #[inline]
    pub fn get(&self, id: DomainId) -> Option<&DomainControlBlock> {
        // FIXME: Duplicate DcbPages::MAX_DOMAINS const from nucleus/objects/domain.rs here, keep in sync!
        const MAX_DOMAINS: usize = 8192;
        if id.0 >= MAX_DOMAINS as u32 {
            return None;
        }

        let dcb = unsafe { &*self.base.add(id.0 as usize) };

        // Basic validity check
        if dcb.id != id {
            return None;
        }

        Some(dcb)
    }

    /// Get state of a domain (fast path for schedulers)
    #[inline]
    pub fn state(&self, id: DomainId) -> Option<DomainState> {
        let dcb = self.get(id)?;
        let raw = dcb.state.load(Ordering::Acquire);
        DomainState::try_from(raw).ok()
    }

    /// Check if a domain is runnable
    #[inline]
    pub fn is_runnable(&self, id: DomainId) -> bool {
        self.state(id) == Some(DomainState::Runnable)
    }

    /// Get time consumed by a domain
    #[inline]
    pub fn time_consumed(&self, id: DomainId) -> Option<u64> {
        let dcb = self.get(id)?;
        Some(dcb.time_consumed_ns.load(Ordering::Relaxed))
    }

    /// Get my own DCB
    #[inline(always)]
    pub fn myself(&self) -> &DomainControlBlock {
        // Current domain ID stored in thread-local or well-known register
        self.get(DomainId(1)) //current_domain_id()
            .expect("Self-domain is always present")
    }
}

// Global user-space accessor (set up during domain init) - FIXME: ?
// In user-space code:
// static DCB: DcbView = unsafe { DcbView::from_user_mapping() };

impl TryFrom<u32> for DomainState {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(DomainState::Inactive),
            1 => Ok(DomainState::Runnable),
            2 => Ok(DomainState::Running),
            3 => Ok(DomainState::Blocked),
            4 => Ok(DomainState::Suspended),
            5 => Ok(DomainState::Faulted),
            6 => Ok(DomainState::Dying),
            _ => Err(()),
        }
    }
}

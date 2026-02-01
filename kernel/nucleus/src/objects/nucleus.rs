use {
    crate::objects::{ArchObjects, Domain, ObjectPool, arch::ArchPools, domain::DcbPages},
    core::sync::atomic::Ordering,
    libobject::{
        KeySlot,
        domain::{BlockReason, DomainControlBlock, DomainId, DomainState},
    },
};

// ┌─────────────────────────────────────────────────────────────────────┐
// │                    KERNEL TYPE STRUCTURE                            │
// ├─────────────────────────────────────────────────────────────────────┤
// │                                                                     │
// │  Kernel<A: ArchObjects>                                             │
// │  │                                                                  │
// │  ├── pools: KernelPools<A>                                          │
// │  │   ├── untypeds: ObjectPool<Untyped>                              │
// │  │   ├── domains: ObjectPool<Domain>                                │
// │  │   ├── keytables: ObjectPool<KeyTable>                            │
// │  │   ├── notifications: ObjectPool<Notification>                    │
// │  │   ├── event_counts: ObjectPool<EventCount>                       │
// │  │   ├── endpoints: ObjectPool<Endpoint>                            │
// │  │   ├── time_slices: ObjectPool<TimeSlice>                         │
// │  │   ├── buffers: ObjectPool<Buffer>                                │
// │  │   ├── replies: ObjectPool<Reply>                                 │
// │  │   │                                                              │
// │  │   └── arch: ArchPools<A>                                         │
// │  │       ├── frames: ObjectPool<A::Frame>                           │
// │  │       ├── page_tables: ObjectPool<A::PageTable>                  │
// │  │       ├── vspaces: ObjectPool<A::VSpace>                         │
// │  │       ├── asid_pools: ObjectPool<A::ASIDPool>                    │
// │  │       └── asids: ObjectPool<A::ASID>                             │
// │  │                                                                  │
// │  ├── current_domain: Option<DomainId>                               │
// │  └── dcb_pages: DcbPages                                            │
// │                                                                     │
// └─────────────────────────────────────────────────────────────────────┘

// ═══════════════════════════════════════════════════════════════════
// UNIFIED KERNEL OBJECT MANAGEMENT
// ═══════════════════════════════════════════════════════════════════

/// All kernel object pools - both core and architecture-specific
pub struct NucleusPools<A: ArchObjects> {
    // ─── Core Object Pools ───
    // pub untypeds: ObjectPool<Untyped>,
    pub domains: ObjectPool<Domain>,
    // pub keytables: ObjectPool<KeyTable>,
    // pub notifications: ObjectPool<Notification>,
    // pub event_counts: ObjectPool<EventCount>,
    // pub endpoints: ObjectPool<Endpoint>,
    // pub time_slices: ObjectPool<TimeSlice>,
    // pub buffers: ObjectPool<Buffer>,
    // pub replies: ObjectPool<Reply>,

    // ─── Architecture-Specific Pools ───
    pub arch: ArchPools<A>,
}

/// Complete nucleus state (parameterized by architecture)
pub struct Nucleus<A: ArchObjects> {
    /// All object pools
    pub pools: NucleusPools<A>,
    /// Currently running domain
    pub current_domain: Option<u32 /*DomainId*/>, // FIXME: not option, always something (Idle or other)
    /// DCB shared pages
    pub dcb_pages: DcbPages,
}

// ═══════════════════════════════════════════════════════════════════
// KERNEL INTEGRATION
// ═══════════════════════════════════════════════════════════════════

impl<A: ArchObjects> Nucleus<A> {
    pub fn current_cpu(&self) -> usize {
        0
    }

    pub fn current_time_ns(&self) -> u64 {
        0
    }

    /// Nucleus-private domain data, like keytables
    pub fn current_domain_mut(&mut self) -> Option<&mut Domain> {
        // need objects::Domain here, not DCB! or a tuple
        self.pools
            .domains
            .get_mut(self.current_domain.unwrap_or(0) as usize)
    }

    /// User-visible DCB
    pub fn current_dcb_mut(&mut self) -> Option<&mut DomainControlBlock> {
        // need objects::Domain here, not DCB! or a tuple
        self.dcb_pages
            .get_mut(DomainId(self.current_domain.unwrap_or(0)))
    }

    /// Update DCB when domain is activated
    pub fn activate_domain(&mut self, id: DomainId, time_budget_ns: u64) {
        let cpu = self.current_cpu();
        let time = self.current_time_ns();
        if let Some(dcb) = self.dcb_pages.get_mut(id) {
            // Update time budget
            dcb.time_remaining_ns
                .store(time_budget_ns, Ordering::Relaxed);
            dcb.last_activated_ns.store(time, Ordering::Relaxed);
            dcb.activation_count.fetch_add(1, Ordering::Relaxed);
            dcb.cpu.store(cpu as u32, Ordering::Relaxed);

            // Set state last (Release ensures all above writes are visible)
            dcb.state
                .store(DomainState::Running as u32, Ordering::Release);
        }
    }

    /// Update DCB when domain yields/blocks/faults
    pub fn deactivate_domain(&mut self, id: DomainId, reason: DeactivateReason) {
        let elapsed = 0; //self.time_since_activation(id);

        if let Some(dcb) = self.dcb_pages.get_mut(id) {
            // Update time accounting
            dcb.time_consumed_ns.fetch_add(elapsed, Ordering::Relaxed);
            dcb.time_remaining_ns.fetch_sub(
                elapsed.min(dcb.time_remaining_ns.load(Ordering::Relaxed)),
                Ordering::Relaxed,
            );

            // Update state based on reason
            match reason {
                DeactivateReason::TimeExhausted => {
                    dcb.state
                        .store(DomainState::Runnable as u32, Ordering::Release);
                }

                DeactivateReason::Blocked { reason, slot } => {
                    dcb.block_reason.store(reason as u32, Ordering::Relaxed);
                    dcb.blocked_on_slot.store(slot.0 as u32, Ordering::Relaxed);
                    dcb.state
                        .store(DomainState::Blocked as u32, Ordering::Release);
                }

                DeactivateReason::Faulted {
                    fault_type,
                    code,
                    addr,
                    slot,
                } => {
                    dcb.fault_type.store(fault_type as u32, Ordering::Relaxed);
                    dcb.fault_code.store(code, Ordering::Relaxed);
                    dcb.fault_addr.store(addr, Ordering::Relaxed);
                    dcb.fault_slot.store(slot.0 as u32, Ordering::Relaxed);
                    dcb.state
                        .store(DomainState::Faulted as u32, Ordering::Release);
                }

                DeactivateReason::Suspended => {
                    dcb.state
                        .store(DomainState::Suspended as u32, Ordering::Release);
                }

                DeactivateReason::Yielded => {
                    dcb.state
                        .store(DomainState::Runnable as u32, Ordering::Release);
                }
            }
        }
    }

    /// Update DCB when notification is signaled to a domain
    pub fn signal_notification(&mut self, id: DomainId, slot: KeySlot, bits: u64) {
        if let Some(dcb) = self.dcb_pages.get_mut(id) {
            // OR the notification bits
            dcb.pending_notifications
                .fetch_or(1 << slot.0, Ordering::Release);

            // If domain was blocked on notifications, make it runnable
            let state = dcb.state.load(Ordering::Acquire);
            let block_reason = dcb.block_reason.load(Ordering::Relaxed);

            if state == DomainState::Blocked as u32
                && block_reason == BlockReason::Notification as u32
            {
                dcb.state
                    .store(DomainState::Runnable as u32, Ordering::Release);
            }
        }
    }
}

pub enum DeactivateReason {
    TimeExhausted,
    Blocked {
        reason: BlockReason,
        slot: KeySlot,
    },
    Faulted {
        fault_type: FaultType,
        code: u32,
        addr: u64,
        slot: KeySlot,
    },
    Suspended,
    Yielded,
}

#[repr(u32)]
pub enum FaultType {
    None = 0,
    PageFault = 1,
    CapFault = 2,
    UnknownSyscall = 3,
    UserException = 4,
    VMFault = 5,
}

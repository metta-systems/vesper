use {
    crate::objects::{
        ArchObjects, Domain, KeyTable, ObjectPool, arch::ArchPools, domain::DcbPages,
    },
    core::sync::atomic::Ordering,
    libobject::{
        KeySlot, RawKey,
        domain::{BlockReason, DomainControlBlock, DomainId, DomainState},
    },
};

#[cfg(feature = "debug_kernel")]
use crate::{api::key_entry::KeyEntry, objects::DebugConsole};

// ┌─────────────────────────────────────────────────────────────────────┐
// │                    KERNEL TYPE STRUCTURE                            │
// ├─────────────────────────────────────────────────────────────────────┤
// │                                                                     │
// │  Kernel<A: ArchObjects>                                             │
// │  │                                                                  │
// │  ├── pools: KernelPools<A>                                          │
// │  │   ├── untypeds: ObjectPool<Untyped>                              │
// │  │   ├── domains: ObjectPool<Domain>                                │
// │  │   ├── keytables: carved by Retype (no pool; see api::untyped)     │
// │  │   ├── notifications: ObjectPool<Notification>                    │
// │  │   ├── event_counts: ObjectPool<EventCount>                       │
// │  │   ├── endpoints: ObjectPool<Endpoint>                            │
// │  │   ├── time_slices: ObjectPool<TimeSlice>                         │
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
    // pub notifications: ObjectPool<Notification>,
    // pub event_counts: ObjectPool<EventCount>,
    // pub endpoints: ObjectPool<Endpoint>,
    // pub time_slices: ObjectPool<TimeSlice>,
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
    #[expect(clippy::unused_self)]
    pub fn current_cpu(&self) -> usize {
        0
    }

    #[expect(clippy::unused_self)]
    pub fn current_time_ns(&self) -> u64 {
        0
    }

    /// Nucleus-private domain data, like keytables
    pub fn current_domain_mut(&mut self) -> Option<&mut Domain> {
        // need objects::Domain here, not DCB! or a tuple
        let id = self.current_domain?;
        self.pools.domains.get_live_mut(usize::try_from(id).ok()?)
    }

    /// Shared access to the current domain's capability table.
    pub fn current_domain_table(&self) -> Option<&KeyTable> {
        let addr = self.current_domain_table_addr()?;
        // SAFETY: the domain's table address is kernel-issued (carved by Retype
        // or the boot carve) and the region is never freed under accepted-leak.
        Some(unsafe { &*(addr as *const KeyTable) })
    }

    /// Exclusive access to the current domain's capability table.
    pub fn current_domain_table_mut(&mut self) -> Option<&mut KeyTable> {
        let addr = self.current_domain_table_addr()?;
        // SAFETY: see current_domain_table; &mut self guarantees exclusivity.
        Some(unsafe { &mut *(addr as *mut KeyTable) })
    }

    /// Address of the current domain's capability table (a carved `KeyTable`).
    pub fn current_domain_table_addr(&self) -> Option<u64> {
        let id = self.current_domain?;
        let dom = self.pools.domains.get_live(usize::try_from(id).ok()?)?;
        Some(dom.keytable_addr)
    }

    /// User-visible DCB
    pub fn current_dcb_mut(&mut self) -> Option<&mut DomainControlBlock> {
        // need objects::Domain here, not DCB! or a tuple
        let id = self.current_domain?;
        self.dcb_pages.get_mut(DomainId(id))
    }

    // TODO: Testing fixture
    #[cfg_attr(
        feature = "debug_kernel",
        expect(
            clippy::unnecessary_wraps,
            reason = "feature-off bootstrap has no console key"
        )
    )]
    pub fn create_domain(&mut self, keytable_addr: u64) -> Option<RawKey> {
        // Allocate the Domain itself; its capability table is a carved KeyTable
        // provided by the caller (Retype or the boot carve).
        let (_dom_id, _dom) = self.pools.domains.allocate(Domain {
            keytable_addr,
            translation_root: None,
            asid: None,
        })?;
        #[cfg(feature = "debug_kernel")]
        {
            // The debug console is a stateless singleton, not a pool object;
            // its entry carries a null identity and is validated by type only
            // (see the debug-only exception in doc/nucleus_capabilities.md).
            // SAFETY: the caller supplied a live carved table address; the
            // region is never freed under the accepted-leak model.
            let keytable = unsafe { &mut *(keytable_addr as *mut KeyTable) };
            let key = keytable
                .insert(
                    KeySlot::DEBUG_CONSOLE,
                    KeyEntry::from_id(
                        libobject::ObjectType::DEBUG_CONSOLE,
                        crate::objects::access::ObjectId {
                            pool: crate::objects::access::PoolTag::Region,
                            index: 0,
                            generation: 0,
                        },
                        libobject::Rights::all(),
                        0,
                    ),
                )
                .unwrap_or_else(|failure| {
                    panic!(
                        "bootstrap console installation failed: {:?}",
                        failure.error.code()
                    )
                });
            Some(key)
        }
        #[cfg(not(feature = "debug_kernel"))]
        None
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
            dcb.cpu
                .store(u32::try_from(cpu).unwrap(), Ordering::Relaxed);

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
                DeactivateReason::TimeExhausted | DeactivateReason::Yielded => {
                    dcb.state
                        .store(DomainState::Runnable as u32, Ordering::Release);
                }

                DeactivateReason::Blocked { reason, slot } => {
                    dcb.block_reason.store(reason as u32, Ordering::Relaxed);
                    dcb.blocked_on_slot.store(slot.0, Ordering::Relaxed);
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
                    dcb.fault_slot.store(slot.0, Ordering::Relaxed);
                    dcb.state
                        .store(DomainState::Faulted as u32, Ordering::Release);
                }

                DeactivateReason::Suspended => {
                    dcb.state
                        .store(DomainState::Suspended as u32, Ordering::Release);
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

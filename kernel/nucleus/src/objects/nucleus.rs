use crate::objects::{ArchObjects, arch::ArchPools};

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
    // pub domains: ObjectPool<Domain>,
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

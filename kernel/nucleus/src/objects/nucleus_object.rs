//! Kernel object storage and capability lookup
//!
//! Design goals:
//! 1. Compact `KeyEntry` (fits in cache line)
//! 2. Type-safe access from handlers
//! 3. Objects live in typed pools (good for allocation)
//! 4. Support for derivation/revocation tree

// ┌─────────────────────────────────────────────────────────────────────┐
// │                    ARCHITECTURE-SPECIFIC OBJECTS                    │
// ├─────────────────────────────────────────────────────────────────────┤
// │                                                                     │
// │  Generic Kernel Objects        Architecture-Specific Objects        │
// │  ─────────────────────────     ─────────────────────────────────    │
// │                                                                     │
// │  • Untyped                     AArch64:                             │
// │  • Domain                        • Frame (4KB, 2MB, 1GB pages)      │
// │  • KeyTable                      • PageTable (translation table)    │
// │  • Notification                  • VSpace (TTBR0/TTBR1 root)        │
// │  • EventCount                    • ASIDPool (ASID allocation)       │
// │  • Endpoint                      • ASID (address space ID)          │
// │  • Time                          • IOSpace (SMMU for devices)       │
// │  • Buffer                                                           │
// │  • Reply                       x86_64:                              │
// │                                  • Frame (4KB, 2MB, 1GB pages)      │
// │                                  • PageTable (PML4/PDPT/PD/PT)      │
// │                                  • VSpace (CR3 root)                │
// │                                  • IOPort (x86 I/O ports)           │
// │                                  • IOSpace (VT-d for devices)       │
// │                                                                     │
// │  RISC-V:                                                            │
// │    • Frame (4KB, 2MB, 1GB)                                          │
// │    • PageTable (Sv39/Sv48)                                          │
// │    • VSpace (satp root)                                             │
// │                                                                     │
// └─────────────────────────────────────────────────────────────────────┘

// ┌─────────────────────────────────────────────────────────────────────┐
// │                    OBJECT TYPE HIERARCHY                            │
// ├─────────────────────────────────────────────────────────────────────┤
// │                                                                     │
// │  ObjectType (u8)                                                    │
// │  ├── Core Types (0-15)                                              │
// │  │   ├── 0: Null                                                    │
// │  │   ├── 1: Untyped       ─→ Untyped struct                         │
// │  │   ├── 2: Domain        ─→ Domain struct                          │
// │  │   ├── 3: KeyTable      ─→ KeyTable struct                        │
// │  │   ├── 4: Notification  ─→ Notification struct                    │
// │  │   ├── 5: EventCount    ─→ EventCount struct                      │
// │  │   ├── 6: Endpoint      ─→ Endpoint struct                        │
// │  │   ├── 7: Time          ─→ TimeSlice struct                       │
// │  │   ├── 8: Buffer        ─→ Buffer struct                          │
// │  │   └── 9: Reply         ─→ Reply struct                           │
// │  │                                                                  │
// │  └── Arch Types (16-63) ──────────────────────────────────────────┐ │
// │      │                                                            │ │
// │      │  ┌─────────────────────────────────────────────────────┐   │ │
// │      │  │ impl ArchObjects for AArch64                        │   │ │
// │      │  │   type Frame = AArch64Frame                         │   │ │
// │      │  │   type PageTable = AArch64PageTable                 │   │ │
// │      │  │   type VSpace = AArch64VSpace                       │   │ │
// │      │  │   type ASIDPool = AArch64ASIDPool                   │   │ │
// │      │  │   type ASID = AArch64ASID                           │   │ │
// │      │  └─────────────────────────────────────────────────────┘   │ │
// │      │                                                            │ │
// │      ├── 16: Frame       ─→ A::Frame                              │ │
// │      ├── 17: PageTable   ─→ A::PageTable                          │ │
// │      ├── 18: VSpace      ─→ A::VSpace                             │ │
// │      ├── 19: ASIDPool    ─→ A::ASIDPool                           │ │
// │      ├── 20: ASID        ─→ A::ASID                               │ │
// │      ├── 21: IOSpace     ─→ (SMMU/VT-d specific)                  │ │
// │      ├── 22: IOPort      ─→ (x86 only)                            │ │
// │      ├── 23: IRQHandler  ─→ IRQ binding                           │ │
// │      └── 24: IRQControl  ─→ IRQ management                        │ │
// │                                                                     │
// └─────────────────────────────────────────────────────────────────────┘

// ═══════════════════════════════════════════════════════════════════
// KERNEL OBJECT TRAIT
// ═══════════════════════════════════════════════════════════════════

/// Marker trait for kernel objects - provides type → `ObjectType` mapping
pub trait NucleusObject: Sized + 'static {
    const TYPE: libobject::ObjectType;

    //TODO: add invoke here?
    // fn invoke(obj: &Self::TYPE, op: u32, args: &[u64]) -> SyscallResult;
}

// Should object type live here or in libobject? Seems like here is a better place but CapErrors need to refer to object types.

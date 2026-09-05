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

// Object kind catalogues and wire IDs live in libobject::object_type.
// The cross-layer contract is documented in doc/nucleus_capabilities.md.

// ═══════════════════════════════════════════════════════════════════
// KERNEL OBJECT TRAIT
// ═══════════════════════════════════════════════════════════════════

/// Marker trait for kernel objects - provides type → `ObjectType` mapping
pub trait NucleusObject: Sized + 'static {
    const TYPE: libobject::ObjectType;

    //TODO: add invoke here?
    // fn invoke(obj: &Self::TYPE, op: u32, args: &[u64]) -> SyscallResult;
}

//! Kernel object storage and capability lookup
//!
//! Design goals:
//! 1. Compact KeyEntry (fits in cache line)
//! 2. Type-safe access from handlers
//! 3. Objects live in typed pools (good for allocation)
//! 4. Support for derivation/revocation tree

use core::ptr::NonNull;

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

/// Marker trait for kernel objects - provides type → ObjectType mapping
pub trait NucleusObject: Sized + 'static {
    const TYPE: ObjectType;

    //TODO: add invoke here?
    // fn invoke(obj: &Self::TYPE, op: u32, args: &[u64]) -> SyscallResult;
}

// ═══════════════════════════════════════════════════════════════════
// TYPE-ERASED OBJECT POINTER
// ═══════════════════════════════════════════════════════════════════

/// A type-erased pointer to a kernel object, with its type tag.
///
/// This is the "fat pointer" alternative - we store the type alongside
/// the pointer so we can safely cast it back.
#[derive(Clone, Copy)]
pub struct ObjectRef {
    ptr: NonNull<()>,
    obj_type: ObjectType,
}

impl ObjectRef {
    /// Create a new object reference from a typed pointer
    pub fn new<T: NucleusObject>(obj: &T) -> Self {
        Self {
            ptr: NonNull::from(obj).cast(),
            obj_type: T::TYPE,
        }
    }

    /// Create from a mutable pointer (for objects in pools)
    ///
    /// # Safety
    /// Caller must ensure the pointer is valid and properly aligned
    pub unsafe fn from_raw<T: NucleusObject>(ptr: *mut T) -> Self {
        Self {
            ptr: NonNull::new_unchecked(ptr.cast()),
            obj_type: T::TYPE,
        }
    }

    /// Get the object type
    #[inline]
    pub fn object_type(&self) -> ObjectType {
        self.obj_type
    }

    /// Attempt to cast to a specific type (immutable)
    #[inline]
    pub fn try_as<T: NucleusObject>(&self) -> Option<&T> {
        if self.obj_type == T::TYPE {
            // SAFETY: We verified the type matches
            Some(unsafe { self.ptr.cast::<T>().as_ref() })
        } else {
            None
        }
    }

    /// Attempt to cast to a specific type (mutable)
    #[inline]
    pub fn try_as_mut<T: NucleusObject>(&mut self) -> Option<&mut T> {
        if self.obj_type == T::TYPE {
            // SAFETY: We verified the type matches
            Some(unsafe { self.ptr.cast::<T>().as_mut() })
        } else {
            None
        }
    }

    /// Cast with error on type mismatch
    #[inline]
    pub fn as_type<T: NucleusObject>(&self) -> Result<&T, CapError> {
        self.try_as().ok_or(CapError::TypeMismatch {
            expected: T::TYPE,
            found: self.obj_type,
        })
    }

    /// Cast with error on type mismatch (mutable)
    #[inline]
    pub fn as_type_mut<T: NucleusObject>(&mut self) -> Result<&mut T, CapError> {
        self.try_as_mut().ok_or(CapError::TypeMismatch {
            expected: T::TYPE,
            found: self.obj_type,
        })
    }
}

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
// OBJECT TYPE DISCRIMINANT - EXTENDED FOR ARCH TYPES
// ═══════════════════════════════════════════════════════════════════

/// Core object types (architecture-independent)
///
/// These are the same across all architectures.
/// Values 0-63 are reserved for core types.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ObjectType {
    // ─── Core Types (0-15) ───
    Null = 0,
    Untyped = 1,
    Domain = 2,
    KeyTable = 3,
    Notification = 4,
    EventCount = 5,
    Endpoint = 6,
    Time = 7,
    Buffer = 8,
    Reply = 9,

    // Reserved for future core types: 10-15

    // ─── Architecture-Specific Types (16-63) ───
    // These are defined per-architecture but share the enum space
    // so we can have a single ObjectType for dispatch.
    /// Physical memory frame (page)
    Frame = 16,
    /// Page table (any level)
    PageTable = 17,
    /// Virtual address space root
    VSpace = 18,
    /// ASID pool (AArch64, RISC-V)
    ASIDPool = 19,
    /// ASID control (AArch64, RISC-V)
    ASID = 20,
    /// I/O memory space (SMMU/VT-d)
    IOSpace = 21,
    /// I/O port range (x86 only)
    IOPort = 22,
    /// IRQ handler object
    IRQHandler = 23,
    /// IRQ control (for binding IRQs)
    IRQControl = 24,
    // Reserved for future arch types: 25-63
}

impl ObjectType {
    /// Is this a core (architecture-independent) type?
    #[inline]
    pub const fn is_core(&self) -> bool {
        (*self as u8) < 16
    }

    /// Is this an architecture-specific type?
    #[inline]
    pub const fn is_arch(&self) -> bool {
        (*self as u8) >= 16 && (*self as u8) < 64
    }
}

// ═══════════════════════════════════════════════════════════════════
// KERNEL OBJECT TRAIT
// ═══════════════════════════════════════════════════════════════════

/// Marker trait for kernel objects - provides type → ObjectType mapping
pub trait KernelObject: Sized + 'static {
    const TYPE: ObjectType;

    //TODO: add invoke here?
    // fn invoke(obj: &Self::TYPE, op: u32, args: &[u64]) -> SyscallResult;
}

// Implement for each kernel object type
impl KernelObject for Untyped {
    const TYPE: ObjectType = ObjectType::Untyped;
}
impl KernelObject for Domain {
    const TYPE: ObjectType = ObjectType::Domain;
}
impl KernelObject for KeyTable {
    const TYPE: ObjectType = ObjectType::KeyTable;
}
impl KernelObject for Notification {
    const TYPE: ObjectType = ObjectType::Notification;
}
impl KernelObject for EventCount {
    const TYPE: ObjectType = ObjectType::EventCount;
}
impl KernelObject for Endpoint {
    const TYPE: ObjectType = ObjectType::Endpoint;
}
impl KernelObject for TimeSlice {
    const TYPE: ObjectType = ObjectType::Time;
}
impl KernelObject for Buffer {
    const TYPE: ObjectType = ObjectType::Buffer;
}
impl KernelObject for Reply {
    const TYPE: ObjectType = ObjectType::Reply;
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
    pub fn new<T: KernelObject>(obj: &T) -> Self {
        Self {
            ptr: NonNull::from(obj).cast(),
            obj_type: T::TYPE,
        }
    }

    /// Create from a mutable pointer (for objects in pools)
    ///
    /// # Safety
    /// Caller must ensure the pointer is valid and properly aligned
    pub unsafe fn from_raw<T: KernelObject>(ptr: *mut T) -> Self {
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
    pub fn try_as<T: KernelObject>(&self) -> Option<&T> {
        if self.obj_type == T::TYPE {
            // SAFETY: We verified the type matches
            Some(unsafe { self.ptr.cast::<T>().as_ref() })
        } else {
            None
        }
    }

    /// Attempt to cast to a specific type (mutable)
    #[inline]
    pub fn try_as_mut<T: KernelObject>(&mut self) -> Option<&mut T> {
        if self.obj_type == T::TYPE {
            // SAFETY: We verified the type matches
            Some(unsafe { self.ptr.cast::<T>().as_mut() })
        } else {
            None
        }
    }

    /// Cast with error on type mismatch
    #[inline]
    pub fn as_type<T: KernelObject>(&self) -> Result<&T, CapError> {
        self.try_as().ok_or(CapError::TypeMismatch {
            expected: T::TYPE,
            found: self.obj_type,
        })
    }

    /// Cast with error on type mismatch (mutable)
    #[inline]
    pub fn as_type_mut<T: KernelObject>(&mut self) -> Result<&mut T, CapError> {
        self.try_as_mut().ok_or(CapError::TypeMismatch {
            expected: T::TYPE,
            found: self.obj_type,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════
// KEY ENTRY (CAPABILITY TABLE ENTRY)
// ═══════════════════════════════════════════════════════════════════

/// A single entry in a domain's capability table (KeyTable).
///
/// Size: 32 bytes (fits nicely in cache)
///
/// ```text
/// ┌────────────────────────────────────────┐
/// │ object_ref: ObjectRef (16 bytes)       │
/// │   - ptr: NonNull<()>    (8 bytes)      │
/// │   - obj_type: ObjectType (1 byte)      │
/// │   - padding             (7 bytes)      │
/// ├────────────────────────────────────────┤
/// │ rights: Rights          (2 bytes)      │
/// │ parent_slot: u16        (2 bytes)      │
/// │ badge: u32              (4 bytes)      │
/// │ gen: u32                (4 bytes)      │
/// │ padding                 (4 bytes)      │
/// └────────────────────────────────────────┘
/// ```
#[repr(C)]
pub struct KeyEntry {
    /// Reference to the kernel object
    object_ref: ObjectRef,
    /// Access rights for this capability
    rights: Rights,
    /// Slot of parent capability (for revocation tree)
    /// 0xFFFF = no parent (root capability)
    parent_slot: u16,
    /// Badge value (for endpoint discrimination, buffer offset, etc.)
    badge: u32,
    /// Generation counter (detect stale capabilities)
    generation: u32,
    _pad: u32,
}

impl KeyEntry {
    /// Create a null/empty entry
    pub const fn null() -> Self {
        Self {
            object_ref: ObjectRef {
                ptr: NonNull::dangling(),
                obj_type: ObjectType::Null,
            },
            rights: Rights::empty(),
            parent_slot: 0xFFFF,
            badge: 0,
            generation: 0,
            _pad: 0,
        }
    }

    /// Create a new capability entry
    pub fn new<T: KernelObject>(
        object: &T,
        rights: Rights,
        badge: u32,
        parent: Option<KeySlot>,
    ) -> Self {
        Self {
            object_ref: ObjectRef::new(object),
            rights,
            parent_slot: parent.map(|s| s.0).unwrap_or(0xFFFF),
            badge,
            generation: 0,
            _pad: 0,
        }
    }

    /// Check if this entry is valid (not null)
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.object_ref.obj_type != ObjectType::Null
    }

    /// Get the object type
    #[inline]
    pub fn object_type(&self) -> ObjectType {
        self.object_ref.obj_type
    }

    /// Get access rights
    #[inline]
    pub fn rights(&self) -> Rights {
        self.rights
    }

    /// Get badge value
    #[inline]
    pub fn badge(&self) -> u32 {
        self.badge
    }

    /// Access the underlying object with type checking
    #[inline]
    pub fn as_object<T: KernelObject>(&self) -> Result<&T, CapError> {
        self.object_ref.as_type()
    }

    /// Access the underlying object mutably with type checking
    #[inline]
    pub fn as_object_mut<T: KernelObject>(&mut self) -> Result<&mut T, CapError> {
        self.object_ref.as_type_mut()
    }
}

// Verify size at compile time
const _: () = assert!(core::mem::size_of::<KeyEntry>() == 32);

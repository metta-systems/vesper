// ═══════════════════════════════════════════════════════════════════
// KEY ENTRY (CAPABILITY TABLE ENTRY)
// ═══════════════════════════════════════════════════════════════════

use crate::{
    libobject::{KeySlot, ObjectType},
    objects::{NucleusObject, ObjectRef},
};

/// A single entry in a domain's capability table (KeyTable).
///
/// Size: 32 bytes (fits nicely in cache)
///
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
///
#[repr(align(32))]
pub struct KeyEntry {
    /// Reference to the kernel object
    object_ref: ObjectRef,
    // FIXME: OR,
    /// Physical address of the kernel object
    // object_paddr: PhysAddr,
    /// Access rights for this capability
    rights: Rights,
    /// Slot of parent capability (for revocation tree)
    /// 0xFFFF = no parent (root capability)
    parent_slot: u16,
    /// Badge value (for endpoint discrimination, buffer offset, etc.)
    badge: u32,
    /// Generation counter (detect stale capabilities)
    generation: u32,
}
// FIXME: ^ for Untyped this should be the object itself...

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
        }
    }

    /// Create a new capability entry
    pub fn new<T: NucleusObject>(
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
    pub fn as_object<T: NucleusObject>(&self) -> Result<&T, CapError> {
        self.object_ref.as_type()
    }

    /// Access the underlying object mutably with type checking
    #[inline]
    pub fn as_object_mut<T: NucleusObject>(&mut self) -> Result<&mut T, CapError> {
        self.object_ref.as_type_mut()
    }
}

// Verify size at compile time
const _: () = assert!(core::mem::size_of::<KeyEntry>() == 32);

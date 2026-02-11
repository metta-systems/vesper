// ═══════════════════════════════════════════════════════════════════
// KEY ENTRY (CAPABILITY TABLE ENTRY)
// ═══════════════════════════════════════════════════════════════════
//
// Tagged union: most types store a pointer to a pool-allocated object,
// but Untyped stores its metadata inline (the capability IS the object).
// Revocation tree is external (userspace CapManager, Composite-style).
//
// ┌──────────────────────────────────────────────┐
// │  Common header — 4 bytes                     │
// │    obj_type: ObjectType       (1 byte)       │
// │    rights: Rights             (1 byte)       │
// │    badge: u16                 (2 bytes)      │
// ├──────────────────────────────────────────────┤
// │  Payload — 16 bytes (union on obj_type)      │
// │                                              │
// │  VARIANT A: Pointer-based (most types)       │
// │    ptr: NonNull<()>           (8 bytes)      │
// │    generation: u32            (4 bytes)      │
// │    _pad: u32                  (4 bytes)      │
// │                                              │
// │  VARIANT B: Inline Untyped                   │
// │    paddr: u64                 (8 bytes)      │
// │    watermark: u32             (4 bytes)      │
// │    size_bits: u8              (1 byte)       │
// │    is_device: bool            (1 byte)       │
// │    _pad: u16                  (2 bytes)      │
// │                                              │
// │  VARIANT C: Null                             │
// │    (all zeros)                               │
// └──────────────────────────────────────────────┘
// Total: 20 bytes used, 32-byte aligned slot

use {
    crate::objects::{NucleusObject, object_ref::ObjectRef},
    core::ptr::NonNull,
    libobject::{CapError, ObjectType, Rights},
};

/// Payload for pointer-based capabilities (most object types).
#[repr(C)]
#[derive(Clone, Copy)]
struct ObjectPayload {
    ptr: NonNull<()>,
    generation: u32,
    _pad: u32,
}

/// Payload for inline Untyped capabilities (no indirection).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct UntypedPayload {
    /// Physical base address of the untyped region
    pub paddr: u64,
    /// Watermark: next free byte offset, shifted right by MIN_ALIGN_BITS (4).
    /// Covers up to 64 GB with 16-byte alignment granularity.
    pub watermark: u32,
    /// Total size as log2 (actual size = 1 << size_bits)
    pub size_bits: u8,
    /// Is this device memory (not normal RAM)?
    pub is_device: bool,
    _pad: u16,
}

/// 16-byte payload union, discriminated by `obj_type` in the header.
#[repr(C)]
union KeyPayload {
    obj: ObjectPayload,
    untyped: UntypedPayload,
    null: [u8; 16],
}

/// A single entry in a domain's capability table (KeyTable).
///
/// 20 bytes used in a 32-byte aligned slot.
/// Discriminated union: `obj_type` selects the payload variant.
#[repr(C, align(32))]
pub struct KeyEntry {
    obj_type: ObjectType,
    rights: Rights,
    badge: u16,
    payload: KeyPayload,
}

// Verify size at compile time
const _: () = assert!(core::mem::size_of::<KeyEntry>() == 32);
const _: () = assert!(core::mem::size_of::<KeyPayload>() == 16);

/// Minimum alignment bits for watermark shift (16-byte alignment).
const MIN_ALIGN_BITS: u32 = 4;

impl KeyEntry {
    /// Create a null/empty entry.
    pub const fn null() -> Self {
        Self {
            obj_type: ObjectType::NULL,
            rights: Rights::empty(),
            badge: 0,
            payload: KeyPayload { null: [0u8; 16] },
        }
    }

    /// Create a pointer-based capability entry (most object types).
    pub fn new<T: NucleusObject>(object: &T, rights: Rights, badge: u16) -> Self {
        Self {
            obj_type: T::TYPE,
            rights,
            badge,
            payload: KeyPayload {
                obj: ObjectPayload {
                    ptr: NonNull::from(object).cast(),
                    generation: 0,
                    _pad: 0,
                },
            },
        }
    }

    /// Create a capability entry from a pre-built ObjectRef (for arch objects).
    pub fn from_ref(obj_ref: ObjectRef, rights: Rights, badge: u16) -> Self {
        Self {
            obj_type: obj_ref.object_type(),
            rights,
            badge,
            payload: KeyPayload {
                obj: ObjectPayload {
                    ptr: obj_ref.as_raw_ptr(),
                    generation: 0,
                    _pad: 0,
                },
            },
        }
    }

    /// Create an inline Untyped capability (no pool allocation).
    pub fn new_untyped(paddr: u64, size_bits: u8, is_device: bool, rights: Rights) -> Self {
        Self {
            obj_type: ObjectType::Untyped,
            rights,
            badge: 0,
            payload: KeyPayload {
                untyped: UntypedPayload {
                    paddr,
                    watermark: 0,
                    size_bits,
                    is_device,
                    _pad: 0,
                },
            },
        }
    }

    /// Check if this entry is valid (not null).
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.obj_type != ObjectType::NULL
    }

    /// Get the object type.
    #[inline]
    pub fn object_type(&self) -> ObjectType {
        self.obj_type
    }

    /// Get access rights.
    #[inline]
    pub fn rights(&self) -> Rights {
        self.rights
    }

    /// Get badge value.
    #[inline]
    pub fn badge(&self) -> u16 {
        self.badge
    }

    /// Get generation counter (pointer-based caps only).
    #[inline]
    pub fn generation(&self) -> u32 {
        debug_assert!(self.obj_type != ObjectType::Untyped && self.obj_type != ObjectType::NULL);
        unsafe { self.payload.obj.generation }
    }

    /// Access the underlying object with type checking.
    /// Panics if called on an Untyped entry — use `as_untyped()` instead.
    #[inline]
    pub fn as_object<T: NucleusObject>(&self) -> Result<&T, CapError> {
        if T::TYPE == ObjectType::Untyped {
            return Err(CapError::TypeMismatch {
                expected: T::TYPE,
                found: self.obj_type,
            });
        }
        if self.obj_type != T::TYPE {
            return Err(CapError::TypeMismatch {
                expected: T::TYPE,
                found: self.obj_type,
            });
        }
        // SAFETY: type verified, pointer-based variant guaranteed
        Ok(unsafe { self.payload.obj.ptr.cast::<T>().as_ref() })
    }

    /// Access the underlying object mutably with type checking.
    /// Panics if called on an Untyped entry — use `as_untyped_mut()` instead.
    #[inline]
    pub fn as_object_mut<T: NucleusObject>(&mut self) -> Result<&mut T, CapError> {
        if T::TYPE == ObjectType::Untyped {
            return Err(CapError::TypeMismatch {
                expected: T::TYPE,
                found: self.obj_type,
            });
        }
        if self.obj_type != T::TYPE {
            return Err(CapError::TypeMismatch {
                expected: T::TYPE,
                found: self.obj_type,
            });
        }
        // SAFETY: type verified, pointer-based variant guaranteed
        Ok(unsafe { self.payload.obj.ptr.cast::<T>().as_mut() })
    }

    /// Access the inline Untyped payload (read-only).
    #[inline]
    pub fn as_untyped(&self) -> Result<&UntypedPayload, CapError> {
        if self.obj_type != ObjectType::Untyped {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::Untyped,
                found: self.obj_type,
            });
        }
        Ok(unsafe { &self.payload.untyped })
    }

    /// Access the inline Untyped payload (mutable).
    #[inline]
    pub fn as_untyped_mut(&mut self) -> Result<&mut UntypedPayload, CapError> {
        if self.obj_type != ObjectType::Untyped {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::Untyped,
                found: self.obj_type,
            });
        }
        Ok(unsafe { &mut self.payload.untyped })
    }
}

// ═══════════════════════════════════════════════════════════════════
// UNTYPED PAYLOAD OPERATIONS
// ═══════════════════════════════════════════════════════════════════

impl UntypedPayload {
    /// Get the total size of the untyped region in bytes.
    #[inline]
    pub fn size(&self) -> usize {
        1usize << self.size_bits
    }

    /// Get the watermark (next free byte offset) in bytes.
    #[inline]
    pub fn watermark_bytes(&self) -> usize {
        (self.watermark as usize) << MIN_ALIGN_BITS
    }

    /// Set the watermark from a byte offset.
    /// The offset must be aligned to MIN_ALIGN_BITS.
    #[inline]
    pub fn set_watermark_bytes(&mut self, offset: usize) {
        debug_assert!(offset & ((1 << MIN_ALIGN_BITS) - 1) == 0);
        self.watermark = (offset >> MIN_ALIGN_BITS) as u32;
    }

    /// Get the remaining free bytes in this region.
    #[inline]
    pub fn free_bytes(&self) -> usize {
        self.size() - self.watermark_bytes()
    }

    /// Check if this untyped is completely free (no allocations).
    #[inline]
    pub fn is_free(&self) -> bool {
        self.watermark == 0
    }

    /// Reset the watermark to zero (after revocation).
    #[inline]
    pub fn reset(&mut self) {
        self.watermark = 0;
    }
}

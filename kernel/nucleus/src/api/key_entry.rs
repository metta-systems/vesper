// ═══════════════════════════════════════════════════════════════════
// KEY ENTRY (CAPABILITY TABLE ENTRY)
// ═══════════════════════════════════════════════════════════════════
//
// Tagged union: most types store a pointer to a pool-allocated object,
// but region types (Untyped, Frame) store metadata inline — the
// capability IS the object, no indirection needed.
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
// │  VARIANT B: Inline Region (Untyped, Frame)   │
// │    paddr: u64                 (8 bytes)      │
// │    state: u32                 (4 bytes)      │
// │      Untyped → watermark (>> MIN_ALIGN_BITS) │
// │      Frame   → map_count (low 16 bits)       │
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

/// Payload for inline region capabilities (Untyped, Frame).
/// No indirection — the capability IS the object.
///
/// The `state` field is dual-use:
/// - **Untyped**: watermark (next free byte offset, shifted right by MIN_ALIGN_BITS)
/// - **Frame**: mapped virtual address >> 12 (0 = unmapped).
///   Each frame cap copy tracks its own single mapping (seL4-style).
///   To map the same physical frame twice, duplicate the cap first.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RegionPayload {
    /// Physical base address of the region
    pub paddr: u64,
    /// Dual-use state field (see type docs)
    pub state: u32,
    /// Size as log2 (region = 2^size_bits)
    pub size_bits: u8,
    /// Is this device memory (not normal RAM)?
    pub is_device: bool,
    _pad: u16,
}

/// 16-byte payload union, discriminated by `obj_type` in the header.
#[repr(C)]
union KeyPayload {
    obj: ObjectPayload,
    region: RegionPayload,
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

// Verify sizes at compile time
const _: () = assert!(core::mem::size_of::<KeyEntry>() == 32); // same as seL4
const _: () = assert!(core::mem::size_of::<KeyPayload>() == 16);
const _: () = assert!(core::mem::size_of::<RegionPayload>() == 16);

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
            obj_type: ObjectType::UNTYPED,
            rights,
            badge: 0,
            payload: KeyPayload {
                region: RegionPayload {
                    paddr,
                    state: 0, // watermark starts at 0
                    size_bits,
                    is_device,
                    _pad: 0,
                },
            },
        }
    }

    /// Create an inline Frame capability (no pool allocation).
    pub fn new_frame(paddr: u64, size_bits: u8, is_device: bool, rights: Rights) -> Self {
        Self {
            obj_type: ObjectType::FRAME,
            rights,
            badge: 0,
            payload: KeyPayload {
                region: RegionPayload {
                    paddr,
                    state: 0, // map_count starts at 0
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

    /// Check if this is an inline region type (Untyped or Frame).
    #[inline]
    pub fn is_region(&self) -> bool {
        self.obj_type == ObjectType::UNTYPED || self.obj_type == ObjectType::FRAME
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
        debug_assert!(!self.is_region() && self.obj_type != ObjectType::NULL);
        unsafe { self.payload.obj.generation }
    }

    /// Access the underlying object with type checking.
    /// Returns error if called on a region type — use `as_region()` instead.
    #[inline]
    pub fn as_object<T: NucleusObject>(&self) -> Result<&T, CapError> {
        if self.obj_type != T::TYPE || self.is_region() {
            return Err(CapError::TypeMismatch {
                expected: T::TYPE,
                found: self.obj_type,
            });
        }
        // SAFETY: type verified, pointer-based variant guaranteed
        Ok(unsafe { self.payload.obj.ptr.cast::<T>().as_ref() })
    }

    /// Access the underlying object mutably with type checking.
    /// Returns error if called on a region type — use `as_region_mut()` instead.
    #[inline]
    pub fn as_object_mut<T: NucleusObject>(&mut self) -> Result<&mut T, CapError> {
        if self.obj_type != T::TYPE || self.is_region() {
            return Err(CapError::TypeMismatch {
                expected: T::TYPE,
                found: self.obj_type,
            });
        }
        // SAFETY: type verified, pointer-based variant guaranteed
        Ok(unsafe { self.payload.obj.ptr.cast::<T>().as_mut() })
    }

    /// Access the inline region payload (Untyped or Frame, read-only).
    #[inline]
    pub fn as_region(&self) -> Result<&RegionPayload, CapError> {
        if !self.is_region() {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::UNTYPED,
                found: self.obj_type,
            });
        }
        Ok(unsafe { &self.payload.region })
    }

    /// Access the inline region payload (Untyped or Frame, mutable).
    #[inline]
    pub fn as_region_mut(&mut self) -> Result<&mut RegionPayload, CapError> {
        if !self.is_region() {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::UNTYPED,
                found: self.obj_type,
            });
        }
        Ok(unsafe { &mut self.payload.region })
    }

    /// Access the inline region payload, but only if this is an Untyped.
    #[inline]
    pub fn as_untyped(&self) -> Result<&RegionPayload, CapError> {
        if self.obj_type != ObjectType::UNTYPED {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::UNTYPED,
                found: self.obj_type,
            });
        }
        Ok(unsafe { &self.payload.region })
    }

    /// Access the inline region payload mutably, but only if this is an Untyped.
    #[inline]
    pub fn as_untyped_mut(&mut self) -> Result<&mut RegionPayload, CapError> {
        if self.obj_type != ObjectType::UNTYPED {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::UNTYPED,
                found: self.obj_type,
            });
        }
        Ok(unsafe { &mut self.payload.region })
    }

    /// Access the inline region payload, but only if this is a Frame.
    #[inline]
    pub fn as_frame(&self) -> Result<&RegionPayload, CapError> {
        if self.obj_type != ObjectType::FRAME {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::FRAME,
                found: self.obj_type,
            });
        }
        Ok(unsafe { &self.payload.region })
    }

    /// Access the inline region payload mutably, but only if this is a Frame.
    #[inline]
    pub fn as_frame_mut(&mut self) -> Result<&mut RegionPayload, CapError> {
        if self.obj_type != ObjectType::FRAME {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::FRAME,
                found: self.obj_type,
            });
        }
        Ok(unsafe { &mut self.payload.region })
    }
}

// ═══════════════════════════════════════════════════════════════════
// REGION PAYLOAD OPERATIONS
// ═══════════════════════════════════════════════════════════════════

impl RegionPayload {
    // ── Common ──

    /// Get the total size of the region in bytes.
    #[inline]
    pub fn size(&self) -> usize {
        1usize << self.size_bits
    }

    /// Check if the state field is zero (no allocations / no mappings).
    #[inline]
    pub fn is_free(&self) -> bool {
        self.state == 0
    }

    /// Reset the state field to zero.
    #[inline]
    pub fn reset(&mut self) {
        self.state = 0;
    }

    // ── Untyped-specific ──

    /// Get the watermark (next free byte offset) in bytes.
    /// Only meaningful when this is an Untyped region.
    #[inline]
    pub fn watermark_bytes(&self) -> usize {
        (self.state as usize) << MIN_ALIGN_BITS
    }

    /// Set the watermark from a byte offset.
    /// The offset must be aligned to MIN_ALIGN_BITS.
    /// Only meaningful when this is an Untyped region.
    #[inline]
    pub fn set_watermark_bytes(&mut self, offset: usize) {
        debug_assert!(offset & ((1 << MIN_ALIGN_BITS) - 1) == 0);
        self.state = (offset >> MIN_ALIGN_BITS) as u32;
    }

    /// Get the remaining free bytes in this untyped region.
    #[inline]
    pub fn free_bytes(&self) -> usize {
        self.size() - self.watermark_bytes()
    }

    // ── Frame-specific ──
    // Each frame cap tracks its own single mapping (seL4-style).
    // state = mapped vaddr >> 12 (0 = unmapped).

    /// Check if this frame cap is currently mapped.
    #[inline]
    pub fn is_mapped(&self) -> bool {
        self.state != 0
    }

    /// Get the virtual address this frame is mapped at (if any).
    #[inline]
    pub fn mapped_vaddr(&self) -> Option<u64> {
        if self.state != 0 {
            Some((self.state as u64) << 12)
        } else {
            None
        }
    }

    /// Record that this frame cap was mapped at `vaddr`.
    /// The vaddr must be page-aligned.
    #[inline]
    pub fn set_mapped(&mut self, vaddr: u64) {
        debug_assert!(vaddr & 0xFFF == 0);
        debug_assert!(vaddr != 0, "cannot map at vaddr 0");
        self.state = (vaddr >> 12) as u32;
    }

    /// Clear the mapping (frame was unmapped).
    #[inline]
    pub fn clear_mapped(&mut self) {
        self.state = 0;
    }
}

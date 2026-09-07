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
// │  VARIANT A: Object identity (most types)     │
// │    pool: PoolTag              (1 byte)       │
// │    _pad: u8                   (1 byte)       │
// │    index: u16                 (2 bytes)      │
// │    generation: u32            (4 bytes)      │
// │    _pad2: u64                 (8 bytes)      │
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
//
// Variant A stores a checked object identity (pool tag, index, generation),
// never a raw pointer: the owning access context computes object addresses
// from pool bases after validating authoritative pool metadata, so stale
// pointers cannot be dereferenced (D3 concrete guarded access, 2026-09-07).

use {
    crate::objects::{
        NucleusObject,
        access::{ObjectId, PoolTag},
    },
    libobject::{CapError, ObjectType, Rights},
};

/// Payload for identity-based capabilities (most object types).
#[repr(C)]
#[derive(Clone, Copy)]
struct ObjectPayload {
    pool: u8,
    _pad: u8,
    index: u16,
    generation: u32,
    _pad2: u64,
}

/// Payload for inline region capabilities (Untyped, Frame).
/// No indirection — the capability IS the object.
///
/// The `state` field is dual-use:
/// - **Untyped**: watermark (next free byte offset, shifted right by `MIN_ALIGN_BITS`)
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
    /// Size as log2 (region = `2^size_bits`)
    pub size_bits: u8,
    /// Is this device memory (not normal RAM)?
    pub is_device: bool,
    pub _pad: u16,
}

/// 16-byte payload union, discriminated by `obj_type` in the header.
#[repr(C)]
#[derive(Clone, Copy)]
union KeyPayload {
    obj: ObjectPayload,
    region: RegionPayload,
    null: [u8; 16],
}

/// A single entry in a domain's capability table (`KeyTable`).
///
/// 20 bytes used in a 32-byte aligned slot.
/// Discriminated union: `obj_type` selects the payload variant.
#[repr(C, align(32))]
#[derive(Clone, Copy)]
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
            payload: KeyPayload { null: [0_u8; 16] },
        }
    }

    /// Create an identity-based capability entry (most object types).
    ///
    /// The entry stores only the checked `ObjectId`; dereferencing requires
    /// the owning access context (see `doc/lifetime-and-authority.md` §3).
    pub fn new<T: NucleusObject>(id: ObjectId, rights: Rights, badge: u16) -> Self {
        debug_assert_eq!(id.pool, T::POOL);
        Self::from_id(T::TYPE, id, rights, badge)
    }

    /// Create an identity-based capability entry from a checked identity
    /// (for arch objects created via `ArchObjects::create_arch_object`).
    pub fn from_id(obj_type: ObjectType, id: ObjectId, rights: Rights, badge: u16) -> Self {
        Self {
            obj_type,
            rights,
            badge,
            payload: KeyPayload {
                obj: ObjectPayload {
                    pool: id.pool as u8,
                    _pad: 0,
                    index: id.index,
                    generation: id.generation,
                    _pad2: 0,
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

    /// Create a derived copy with attenuated rights, preserving the badge and
    /// payload verbatim. Callers must have already established that
    /// `rights` is a subset of this entry's rights; this is a pure
    /// representation transform, not an authority check.
    #[inline]
    pub fn derive(&self, rights: Rights) -> Self {
        let mut derived = *self;
        derived.rights = rights;
        derived
    }

    /// Get badge value.
    #[inline]
    pub fn badge(&self) -> u16 {
        self.badge
    }

    /// Get the checked object identity (identity-based caps only).
    ///
    /// This is a handle, not a reference: dereferencing requires the owning
    /// access context, which validates the identity against authoritative
    /// pool metadata first. Returns error on region types — use `as_region()`.
    #[inline]
    pub fn object_id(&self) -> Result<ObjectId, CapError> {
        if self.is_region() || self.obj_type == ObjectType::NULL {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::UNTYPED, // any region type; see as_region
                found: self.obj_type,
            });
        }
        // SAFETY: identity-based variant guaranteed by the checks above.
        let obj = unsafe { self.payload.obj };
        Ok(ObjectId {
            pool: PoolTag::from_raw(obj.pool),
            index: obj.index,
            generation: obj.generation,
        })
    }

    /// Access the inline region payload (Untyped or Frame, read-only).
    #[inline]
    pub fn as_region(&self) -> Result<&RegionPayload, CapError> {
        if !self.is_region() {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::UNTYPED, // FIXME or Frame?
                found: self.obj_type,
            });
        }
        // SAFETY: We checked the object is valid and is of the right type.
        Ok(unsafe { &self.payload.region })
    }

    /// Access the inline region payload (Untyped or Frame, mutable).
    #[inline]
    pub fn as_region_mut(&mut self) -> Result<&mut RegionPayload, CapError> {
        if !self.is_region() {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::UNTYPED, // FIXME or Frame?
                found: self.obj_type,
            });
        }
        // SAFETY: We checked the object is valid and is of the right type.
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
        // SAFETY: We checked the object is valid and is of the right type.
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
        // SAFETY: We checked the object is valid and is of the right type.
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
        // SAFETY: We checked the object is valid and is of the right type.
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
        // SAFETY: We checked the object is valid and is of the right type.
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
        1_usize << self.size_bits
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
    // TODO: UntypedPayload trait?

    /// Get the watermark (next free byte offset) in bytes.
    /// Only meaningful when this is an Untyped region.
    #[inline]
    pub fn watermark_bytes(&self) -> usize {
        (self.state as usize) << MIN_ALIGN_BITS
    }

    /// Set the watermark from a byte offset.
    /// The offset must be aligned to `MIN_ALIGN_BITS`.
    /// Only meaningful when this is an Untyped region.
    #[inline]
    pub fn set_watermark_bytes(&mut self, offset: usize) {
        debug_assert!(offset & ((1 << MIN_ALIGN_BITS) - 1) == 0);
        self.state = u32::try_from(offset >> MIN_ALIGN_BITS).unwrap();
    }

    /// Get the remaining free bytes in this untyped region.
    #[inline]
    pub fn free_bytes(&self) -> usize {
        self.size() - self.watermark_bytes()
    }

    // ── Frame-specific ──
    // TODO: FramePayload trait?
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
        (self.state != 0).then_some(u64::from(self.state) << 12)
    }

    /// Record that this frame cap was mapped at `vaddr`.
    /// The vaddr must be page-aligned.
    #[inline]
    pub fn set_mapped(&mut self, vaddr: u64) {
        debug_assert!(vaddr.trailing_zeros() >= 12);
        debug_assert!(vaddr != 0, "cannot map at vaddr 0");
        self.state = u32::try_from(vaddr >> 12).unwrap();
    }

    /// Clear the mapping (frame was unmapped).
    #[inline]
    pub fn clear_mapped(&mut self) {
        self.state = 0;
    }
}

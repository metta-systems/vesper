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
// │  Payload — 24 bytes (union on obj_type)      │
// │                                              │
// │  VARIANT A: Object identity (most types)     │
// │    pool: PoolTag              (1 byte)       │
// │    _pad: u8                   (1 byte)       │
// │    index: u16                 (2 bytes)       │
// │    generation: u32            (4 bytes)       │
// │    _pad2: u64                 (8 bytes)       │
// │                                              │
// │  VARIANT B: Inline Untyped                    │
// │    paddr: u64                 (8 bytes)       │
// │    state: u32  → watermark (>> MIN_ALIGN_BITS)│
// │    size_bits: u8              (1 byte)       │
// │    is_device: bool            (1 byte)       │
// │    _pad: u16                  (2 bytes)       │
// │                                              │
// │  VARIANT C: Inline Frame (mapping record)    │
// │    paddr: u64                 (8 bytes)       │
// │    vaddr: u64  → mapped virtual address      │
// │    as_index: u16              (2 bytes)       │
// │    as_generation: u32         (4 bytes)       │
// │    size_bits: u8              (1 byte)       │
// │    flags: u8   → is_device | mapped          │
// │                                              │
// │  VARIANT D: Carved KeyTable address          │
// │    address: u64               (8 bytes)       │
// │                                              │
// │  VARIANT E: Null                             │
// │    (all zeros)                               │
// └──────────────────────────────────────────────┘
// Total: 28 bytes used, 32-byte aligned slot
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

/// Payload for inline Untyped capabilities.
/// No indirection — the capability IS the object.
///
/// The `state` field stores the allocation watermark (next free byte offset,
/// shifted right by `MIN_ALIGN_BITS`).
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

/// Payload for inline Frame capabilities: a physical region plus the frame's
/// single-mapping record.
///
/// Mapping identity must contain enough information to locate and retire the
/// real mapping (translation context and full virtual address), so a frame
/// records its mapping as a dedicated record — the owning `AddressSpace`'s
/// checked identity and the complete virtual address — rather than a compressed
/// address squeezed into a shared state field. Each frame capability tracks
/// its own single mapping (seL4-style); a derived copy starts unmapped.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FramePayload {
    /// Physical base address of the frame.
    pub paddr: u64,
    /// Mapped virtual address; meaningful only while the mapped flag is set.
    pub vaddr: u64,
    /// Owning `AddressSpace` allocation generation; meaningful only while
    /// mapped.
    pub as_generation: u32,
    /// Owning `AddressSpace` pool index; meaningful only while the mapped
    /// flag is set.
    pub as_index: u16,
    /// Size as log2 (frame = `2^size_bits`).
    pub size_bits: u8,
    /// Flag bits: bit 0 `is_device`, bit 1 `mapped`.
    pub flags: u8,
}

/// Flag bit: the backing is device memory, not ordinary RAM.
const FRAME_IS_DEVICE: u8 = 1 << 0;
/// Flag bit: the frame is currently mapped.
const FRAME_MAPPED: u8 = 1 << 1;

/// A frame's recorded mapping: enough identity to locate and retire the real
/// hardware descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameMapping {
    /// The mapping context: the owning `AddressSpace`'s checked identity.
    pub address_space: ObjectId,
    /// The complete mapped virtual address.
    pub vaddr: u64,
}

/// Payload for a `KeyTable` capability: a reference to the carved `KeyTable`
/// kernel object (seL4-style object pointer). Retype-created objects are
/// addressed directly; the carved region is never freed under the accepted-leak
/// model, so the address never goes stale. The kind lives in the entry header;
/// this payload is interpreted per actual object type, not as a storage
/// mechanism.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KeyTablePayload {
    /// Kernel virtual address of the carved `KeyTable` object.
    pub address: u64,
    pub _pad: u64,
}

/// 24-byte payload union, discriminated by `obj_type` in the header.
#[repr(C)]
#[derive(Clone, Copy)]
union KeyPayload {
    obj: ObjectPayload,
    region: RegionPayload,
    frame: FramePayload,
    keytable: KeyTablePayload,
    null: [u8; 24],
}

/// A single entry in a thread's capability table (`KeyTable`).
///
/// 28 bytes used in a 32-byte aligned slot.
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
const _: () = assert!(core::mem::size_of::<KeyPayload>() == 24);
const _: () = assert!(core::mem::size_of::<RegionPayload>() == 16);
const _: () = assert!(core::mem::size_of::<FramePayload>() == 24);
const _: () = assert!(core::mem::size_of::<KeyTablePayload>() == 16);

/// Minimum alignment bits for watermark shift (16-byte alignment).
const MIN_ALIGN_BITS: u32 = 4;

/// Watermark encoding granularity in bytes: stored watermarks are always
/// multiples of this (see `RegionPayload::set_watermark_bytes`), so every
/// allocation end committed to an Untyped's watermark must be aligned up to
/// it or the encoding would silently discard the remainder and let the next
/// allocation overlap.
pub(crate) const MIN_ALIGN: usize = 1 << MIN_ALIGN_BITS;

impl KeyEntry {
    /// Create a null/empty entry.
    pub const fn null() -> Self {
        Self {
            obj_type: ObjectType::NULL,
            rights: Rights::empty(),
            badge: 0,
            payload: KeyPayload { null: [0_u8; 24] },
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
                frame: FramePayload {
                    paddr,
                    vaddr: 0,
                    as_index: 0,
                    as_generation: 0,
                    size_bits,
                    flags: if is_device { FRAME_IS_DEVICE } else { 0 },
                },
            },
        }
    }

    /// Create a capability for a carved `KeyTable` object (Retype-created).
    ///
    /// The payload is a reference to the carved `KeyTable` kernel object; the
    /// kind is the entry header. The address is kernel-issued (from Retype or
    /// the boot carve) and the region is never freed under the accepted-leak
    /// model, so it never goes stale.
    pub fn new_keytable(address: u64, rights: Rights, badge: u16) -> Self {
        Self {
            obj_type: ObjectType::KEY_TABLE,
            rights,
            badge,
            payload: KeyPayload {
                keytable: KeyTablePayload { address, _pad: 0 },
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

    /// Check if this is a Retype-created (carved) object kind.
    ///
    /// Carved objects are addressed directly through a per-type payload rather
    /// than a pooled identity; see `doc/lifetime-and-authority.md` §3.
    #[inline]
    pub fn is_carved(&self) -> bool {
        self.obj_type == ObjectType::KEY_TABLE
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
    /// payload verbatim — except that a derived Frame starts unmapped: the
    /// mapping belongs to the original capability, and Copy is capability-only
    /// derivation with no active mapping association. Callers must have
    /// already established that `rights` is a subset of this entry's rights;
    /// this is a pure representation transform, not an authority check.
    #[inline]
    pub fn derive(&self, rights: Rights) -> Self {
        let mut derived = *self;
        derived.rights = rights;
        if derived.obj_type == ObjectType::FRAME {
            // SAFETY: the FRAME type check selects the frame payload variant.
            unsafe {
                derived.payload.frame.clear_mapped();
            }
        }
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
    /// pool metadata first. Returns error on region types and carved-object
    /// kinds — use `as_region()` / `keytable_address()` instead.
    #[inline]
    pub fn object_id(&self) -> Result<ObjectId, CapError> {
        if self.is_region() || self.is_carved() || self.obj_type == ObjectType::NULL {
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

    /// Get the carved `KeyTable` object address (`KeyTable` caps only).
    ///
    /// The address is kernel-issued and the carved region is never freed under
    /// the accepted-leak model; dereferencing requires the owning access
    /// context, which ties the resulting guard to the locked invocation.
    #[inline]
    pub fn keytable_address(&self) -> Result<u64, CapError> {
        if self.obj_type != ObjectType::KEY_TABLE {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::KEY_TABLE,
                found: self.obj_type,
            });
        }
        // SAFETY: keytable variant guaranteed by the type check above.
        Ok(unsafe { self.payload.keytable.address })
    }

    /// Access the inline Untyped payload (read-only).
    #[inline]
    pub fn as_region(&self) -> Result<&RegionPayload, CapError> {
        if self.obj_type != ObjectType::UNTYPED {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::UNTYPED,
                found: self.obj_type,
            });
        }
        // SAFETY: We checked the object is valid and is of the right type.
        Ok(unsafe { &self.payload.region })
    }

    /// Access the inline Untyped payload (mutable).
    #[inline]
    pub fn as_region_mut(&mut self) -> Result<&mut RegionPayload, CapError> {
        if self.obj_type != ObjectType::UNTYPED {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::UNTYPED,
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

    /// Access the inline Frame payload, but only if this is a Frame.
    #[inline]
    pub fn as_frame(&self) -> Result<&FramePayload, CapError> {
        if self.obj_type != ObjectType::FRAME {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::FRAME,
                found: self.obj_type,
            });
        }
        // SAFETY: We checked the object is valid and is of the right type.
        Ok(unsafe { &self.payload.frame })
    }

    /// Access the inline Frame payload mutably, but only if this is a Frame.
    #[inline]
    pub fn as_frame_mut(&mut self) -> Result<&mut FramePayload, CapError> {
        if self.obj_type != ObjectType::FRAME {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::FRAME,
                found: self.obj_type,
            });
        }
        // SAFETY: We checked the object is valid and is of the right type.
        Ok(unsafe { &mut self.payload.frame })
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
}

// ═══════════════════════════════════════════════════════════════════
// FRAME PAYLOAD OPERATIONS
// ═══════════════════════════════════════════════════════════════════

impl FramePayload {
    /// Get the total size of the frame in bytes.
    #[inline]
    pub fn size(&self) -> usize {
        1_usize << self.size_bits
    }

    /// Whether the backing is device memory, not ordinary RAM.
    #[inline]
    pub fn is_device(&self) -> bool {
        self.flags & FRAME_IS_DEVICE != 0
    }

    /// Check if this frame cap is currently mapped.
    #[inline]
    pub fn is_mapped(&self) -> bool {
        self.flags & FRAME_MAPPED != 0
    }

    /// The recorded mapping identity, if the frame is mapped.
    #[inline]
    pub fn mapping(&self) -> Option<FrameMapping> {
        self.is_mapped().then_some(FrameMapping {
            address_space: ObjectId {
                pool: PoolTag::AddressSpace,
                index: self.as_index,
                generation: self.as_generation,
            },
            vaddr: self.vaddr,
        })
    }

    /// Record that this frame cap was mapped into `address_space` at `vaddr`.
    /// The vaddr must be aligned to the frame size.
    #[inline]
    pub fn set_mapped(&mut self, address_space: ObjectId, vaddr: u64) {
        debug_assert_eq!(address_space.pool, PoolTag::AddressSpace);
        debug_assert_eq!(vaddr & (self.size() as u64 - 1), 0);
        self.as_index = address_space.index;
        self.as_generation = address_space.generation;
        self.vaddr = vaddr;
        self.flags |= FRAME_MAPPED;
    }

    /// Clear the mapping record (frame was unmapped).
    #[inline]
    pub fn clear_mapped(&mut self) {
        self.flags &= !FRAME_MAPPED;
        self.vaddr = 0;
        self.as_index = 0;
        self.as_generation = 0;
    }
}

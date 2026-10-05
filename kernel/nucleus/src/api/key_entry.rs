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
// │                                              │
// │  VARIANT F: Invocation                       │
// │    function_address: NonZero<u64>            │
// │    AddressSpace pool/index/generation        │
// │                                              │
// │  VARIANT G: Thread selector                  │
// │    Named(ObjectId) | CurrentReturnOnly        │
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
    core::num::NonZero,
    libaddress::align,
    libobject::{ArchType, CapError, CoreType, ObjectType, Rights},
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

/// A Thread capability either names an object for management or permits only
/// the invoking Thread's own PPC Return. The current-relative form has no
/// object identity and cannot be resolved through a pool.
#[repr(C, u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadSelector {
    Named(ObjectId),
    CurrentReturnOnly,
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

/// Payload for an `Invocation` capability: the entry address and the checked
/// identity of its target `AddressSpace`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct InvocationPayload {
    /// Interface entry address as supplied to `AddressSpace.CreateInvocation`.
    /// Invocation is Call-only; `CreateInvocation` rejects a zero address.
    /// Current-relative Return authority is a separate Thread selector.
    pub function_address: NonZero<u64>,
    /// Pool tag for the target `AddressSpace`.
    pub address_space_pool: u8,
    pub _pad: u8,
    /// Target `AddressSpace` pool index.
    pub address_space_index: u16,
    /// Target `AddressSpace` allocation generation.
    pub address_space_generation: u32,
    pub _pad2: u64,
}

/// Payload for a `KeyTable` capability: a reference to the carved `KeyTable`
/// kernel object (Retype-created). Retype-created objects are
/// addressed directly; the carved region is never freed under the accepted-leak
/// model, so the address never goes stale. The kind lives in the entry header;
/// this payload is interpreted per actual object type, not as a storage
/// mechanism.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KeyTablePayload {
    /// Kernel virtual address of the carved `KeyTable` object.
    pub address: u64,
    /// The table's guard (guarded key-space package, selected 2026-09-23):
    /// a userspace-chosen value fixed at Retype, carried by every key minted
    /// into the table above the slot index. Copied verbatim on derivation.
    pub guard: u32,
    /// The table's capacity exponent: `2^size_bits` entries.
    pub size_bits: u8,
    pub _pad: [u8; 3],
}

/// 24-byte payload union, discriminated by `obj_type` in the header.
#[repr(C)]
#[derive(Clone, Copy)]
union KeyPayload {
    obj: ObjectPayload,
    thread: ThreadSelector,
    region: RegionPayload,
    frame: FramePayload,
    keytable: KeyTablePayload,
    invocation: InvocationPayload,
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
const _: () = assert!(core::mem::size_of::<InvocationPayload>() == 24);
const _: () = assert!(core::mem::size_of::<ThreadSelector>() == 12);
const _: () = assert!(core::mem::align_of::<ThreadSelector>() == 4);
const _: () = assert!(core::mem::align_of::<KeyEntry>() == 32);

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
        Self::from_id(T::TYPE, id, rights, badge).unwrap_or_else(|error| {
            panic!("not an identity-backed object kind: {:?}", error.code())
        })
    }

    /// Create an identity-based capability entry from a checked identity
    /// (for arch objects created via `ArchObjects::create_arch_object`).
    /// Dedicated-payload kinds are rejected before constructing an entry;
    /// the kind must never select an uninitialized or incompatible union member.
    pub fn from_id(
        obj_type: ObjectType,
        id: ObjectId,
        rights: Rights,
        badge: u16,
    ) -> Result<Self, CapError> {
        if matches!(
            obj_type,
            ObjectType::NULL
                | ObjectType::UNTYPED
                | ObjectType::FRAME
                | ObjectType::KEY_TABLE
                | ObjectType::INVOCATION
        ) {
            return Err(CapError::InvalidObjectType(obj_type));
        }
        if obj_type.is_arch() {
            ArchType::try_from(obj_type)?;
        } else {
            CoreType::try_from(obj_type)?;
        }
        if obj_type == ObjectType::THREAD {
            return Ok(Self {
                obj_type,
                rights,
                badge,
                payload: KeyPayload {
                    thread: ThreadSelector::Named(id),
                },
            });
        }
        Ok(Self {
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
        })
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
    /// The payload is a reference to the carved `KeyTable` kernel object plus
    /// its guard and capacity exponent; the kind is the entry header. The
    /// address is kernel-issued (from Retype or the boot carve) and the region
    /// is never freed under the accepted-leak model, so it never goes stale.
    /// The guard and `size_bits` are fixed at table creation and copied
    /// verbatim by derivation.
    pub fn new_keytable(
        address: u64,
        guard: u32,
        size_bits: u8,
        rights: Rights,
        badge: u16,
    ) -> Self {
        Self {
            obj_type: ObjectType::KEY_TABLE,
            rights,
            badge,
            payload: KeyPayload {
                keytable: KeyTablePayload {
                    address,
                    guard,
                    size_bits,
                    _pad: [0; 3],
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

    /// Create an `Invocation` capability targeting an entry point in an
    /// `AddressSpace`. The capability starts with `CALL` authority and no badge.
    /// The nonzero entry address is mandatory by construction.
    pub fn new_invocation(address_space: ObjectId, function_address: NonZero<u64>) -> Self {
        debug_assert_eq!(address_space.pool, PoolTag::AddressSpace);
        Self {
            obj_type: ObjectType::INVOCATION,
            rights: Rights(Rights::CALL),
            badge: 0,
            payload: KeyPayload {
                invocation: InvocationPayload {
                    function_address,
                    address_space_pool: address_space.pool as u8,
                    _pad: 0,
                    address_space_index: address_space.index,
                    address_space_generation: address_space.generation,
                    _pad2: 0,
                },
            },
        }
    }

    /// Create kernel-installed current-relative PPC Return authority.
    /// It names no Thread, function or `AddressSpace` and grants no management
    /// rights. Ordinary `KeyTable` lookup still applies before invocation.
    pub fn new_thread_return() -> Self {
        Self {
            obj_type: ObjectType::THREAD,
            rights: Rights::empty(),
            badge: 0,
            payload: KeyPayload {
                thread: ThreadSelector::CurrentReturnOnly,
            },
        }
    }

    /// Read a Thread entry's selector without resolving a named object.
    pub fn thread_selector(&self) -> Result<ThreadSelector, CapError> {
        if self.obj_type != ObjectType::THREAD {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::THREAD,
                found: self.obj_type,
            });
        }
        // SAFETY: Thread constructors initialize this variant, and the kind
        // check selects it. Derivation preserves the selector verbatim.
        Ok(unsafe { self.payload.thread })
    }

    /// Whether this entry permits only current-relative Thread.Return.
    pub fn is_thread_return_key(&self) -> bool {
        matches!(
            self.thread_selector(),
            Ok(ThreadSelector::CurrentReturnOnly)
        )
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
        if self.obj_type == ObjectType::THREAD {
            return match self.thread_selector()? {
                ThreadSelector::Named(id) => Ok(id),
                ThreadSelector::CurrentReturnOnly => Err(CapError::InvalidOperation),
            };
        }
        if self.is_region()
            || self.is_carved()
            || self.obj_type == ObjectType::INVOCATION
            || self.obj_type == ObjectType::NULL
        {
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

    /// Get the carved table's guard and capacity exponent (`KeyTable` caps
    /// only). Fixed at table creation; derivation copies them verbatim, so
    /// every capability naming a table carries its guard.
    #[inline]
    pub fn keytable_guard_and_size(&self) -> Result<(u32, u8), CapError> {
        if self.obj_type != ObjectType::KEY_TABLE {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::KEY_TABLE,
                found: self.obj_type,
            });
        }
        // SAFETY: keytable variant guaranteed by the type check above.
        Ok(unsafe { (self.payload.keytable.guard, self.payload.keytable.size_bits) })
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

    /// The `AddressSpace` identity and mandatory entry address of a Call-only
    /// `Invocation`.
    pub fn invocation_target(&self) -> Result<(ObjectId, NonZero<u64>), CapError> {
        if self.obj_type != ObjectType::INVOCATION {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::INVOCATION,
                found: self.obj_type,
            });
        }
        // SAFETY: the Invocation type check selects the invocation payload.
        let payload = unsafe { self.payload.invocation };
        Ok((
            ObjectId {
                pool: PoolTag::from_raw(payload.address_space_pool),
                index: payload.address_space_index,
                generation: payload.address_space_generation,
            },
            payload.function_address,
        ))
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

    /// Validate and reserve `total` bytes aligned to `align` from this
    /// region's unused watermark range — the single internal allocation
    /// primitive shared by `Untyped.Retype`, kernel-private bootstrap
    /// carves, and `ObjectPool::carve`. Pure calculation: the caller
    /// commits by advancing the watermark to `end` (or installing it on the
    /// capability entry), so a rejection leaves the region unchanged.
    ///
    /// The absolute carve address (`paddr + start`) is aligned, not just
    /// the watermark: a misaligned region base is folded into the
    /// computation, so both the carve base and the committed end stay
    /// watermark-encodable and the encoding can never discard
    /// sub-granularity bytes (which would let the next carve overlap this
    /// allocation). The end's padding bytes are consumed, not lost.
    pub fn reserve(&self, align: u64, total: u64) -> Result<CarveReservation, CapError> {
        debug_assert!(align.is_power_of_two());
        // The extent must be representable before any shift or addition:
        // `size_bits` below the address width and `paddr + size` within
        // `u64`. Malformed extents are rejected with the region's own size
        // instead of panicking in the size shift.
        let region_size = 1_u64
            .checked_shl(u32::from(self.size_bits))
            .ok_or(CapError::InvalidSize(usize::from(self.size_bits)))?;
        self.paddr
            .checked_add(region_size)
            .ok_or(CapError::InvalidSize(usize::from(self.size_bits)))?;
        // The watermark state field stores `offset >> MIN_ALIGN_BITS` in a
        // `u32`, so the usable range ends at `u32::MAX << MIN_ALIGN_BITS`
        // (that is, `u32::MAX × MIN_ALIGN`) even in larger regions.
        let usable_end = region_size.min(u64::from(u32::MAX) * u64::try_from(MIN_ALIGN).unwrap());
        let base_misalign = self.paddr & (align - 1);
        let start = align::align_up(
            base_misalign + u64::try_from(self.watermark_bytes()).unwrap(),
            align,
        ) - base_misalign;
        let end = align::align_up(
            start.checked_add(total).ok_or(CapError::InvalidSize(0))?,
            align,
        );
        if end > usable_end {
            return Err(CapError::InsufficientMemory);
        }
        Ok(CarveReservation {
            start: usize::try_from(start).unwrap(),
            end: usize::try_from(end).unwrap(),
        })
    }
}

/// A validated, uncommitted carve from an Untyped's unused watermark range;
/// the result of [`RegionPayload::reserve`].
#[derive(Clone, Copy, Debug)]
pub struct CarveReservation {
    /// Byte offset of the aligned carve start within the region.
    pub start: usize,
    /// Byte offset of the committed end (the new watermark) within the
    /// region.
    pub end: usize,
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

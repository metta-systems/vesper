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
// │  Payload — 56 bytes (union on obj_type)      │
// │                                              │
// │  VARIANT A: Pool identity (ASIDPool)         │
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
// │    as_address: u64            (8 bytes)       │
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
// │    AddressSpace address/generation           │
// │    Stack base/end/minimum headroom (24 bytes)│
// │    Placement window first/count (2 x u32)    │
// │                                              │
// │  VARIANT G: Thread selector                  │
// │    Named(ThreadId) | CurrentReturnOnly        │
// │                                              │
// │  VARIANT H: Untyped-carved object            │
// │    (Notification, EventCount, Time,          │
// │     Scheduler, PageTable, AddressSpace)      │
// │    address: u64               (8 bytes)       │
// │    generation: u32            (4 bytes)       │
// │                                              │
// │  VARIANT I: IRQHandler                       │
// │    line: u32                  (4 bytes)       │
// │    generation: u32            (4 bytes)       │
// │                                              │
// │  IRQControl carries no payload (all zeros)   │
// └──────────────────────────────────────────────┘
// Total: 64 bytes (4-byte header, flags byte, padding to the 8-aligned
// payload), slot aligned to 32 bytes
//
// Variants A and H store a checked object identity, never a bare pointer:
// the owning access context validates a pool slot's metadata, or a carved
// object's header, before any dereference, so stale identities cannot be
// dereferenced.

use {
    crate::objects::{
        NucleusObject,
        access::{ObjectId, PoolTag},
        arch_objects::{AddressSpaceId, PageTableId},
        carved::CarvedId,
        invocation::InvocationStackExtent,
        irq::IrqHandlerId,
        thread::ThreadId,
    },
    core::num::NonZero,
    libaddress::align,
    libobject::{
        ArchType, CapError, CoreType, FrameAttribute, ObjectType, PlacementWindow, Rights,
    },
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
    Named(ThreadId),
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
    /// Wire code of the memory-attribute ceiling of a device region
    /// ([`FrameAttribute`]; zero for RAM). Frames carved from the region and
    /// its split children start at, or below, this ceiling.
    pub ceiling: u8,
    pub _pad: u8,
}

impl RegionPayload {
    /// The memory-attribute ceiling of a device region: the loosest attribute
    /// its Frames may be mapped with. [`FrameAttribute::Default`] for RAM,
    /// which maps `Cacheable` only.
    pub fn ceiling(&self) -> FrameAttribute {
        FrameAttribute::from_code(u64::from(self.ceiling)).unwrap_or(FrameAttribute::Default)
    }
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
    /// Kernel address of the owning `AddressSpace`'s carved record;
    /// meaningful only while the mapped flag is set.
    pub as_address: u64,
    /// Owning `AddressSpace` generation; meaningful only while mapped.
    pub as_generation: u32,
    /// Size as log2 (frame = `2^size_bits`).
    pub size_bits: u8,
    /// Flag bits: bit 0 `is_device`, bit 1 `mapped`, bits 4:2 the
    /// memory-attribute ceiling code of a device Frame.
    pub flags: u8,
    pub _pad: u16,
}

/// Flag bit: the backing is device memory, not ordinary RAM.
const FRAME_IS_DEVICE: u8 = 1 << 0;
/// Flag bit: the frame is currently mapped.
const FRAME_MAPPED: u8 = 1 << 1;
/// Position and mask of the ceiling code of a device Frame in `flags`.
const FRAME_CEILING_SHIFT: u8 = 2;
const FRAME_CEILING_MASK: u8 = 0b111 << FRAME_CEILING_SHIFT;

/// A frame's recorded mapping: enough identity to locate and retire the real
/// hardware descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameMapping {
    /// The mapping context: the owning `AddressSpace`'s checked identity.
    pub address_space: AddressSpaceId,
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
    /// Kernel address of the target `AddressSpace`'s carved record.
    pub address_space_address: u64,
    /// Numeric target stack contract, validated before construction: the
    /// extent's base and end, and its minimum headroom in 16-byte units (the
    /// headroom is a multiple of 16, so the `u32` count covers 2^36 bytes).
    pub stack_base: u64,
    pub stack_end: u64,
    pub minimum_headroom_units: u32,
    /// Target `AddressSpace` generation.
    pub address_space_generation: u32,
    /// First slot of the key-placement window `CallWithKey` places a lent key
    /// in, within the target `AddressSpace`'s `KeyTable`. Meaningful only when
    /// `placement_count` is nonzero.
    pub placement_first: u32,
    /// Number of slots in the key-placement window; zero means the Invocation
    /// accepts no lent key.
    pub placement_count: u32,
}

/// Payload for a `KeyTable` capability: the carved `KeyTable`'s identity
/// (its [`CarvedId`] without the kind, which lives in the entry header) plus
/// the table's guard and capacity exponent. Retiring the table bumps its
/// generation, so every capability still naming it fails.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KeyTablePayload {
    /// Kernel virtual address of the carved `KeyTable` record.
    pub address: u64,
    /// The table's generation when this capability was minted.
    pub generation: u32,
    /// The table's guard:
    /// a userspace-chosen value fixed at Retype, carried by every key minted
    /// into the table above the slot index. Copied verbatim on derivation.
    pub guard: u32,
    /// The table's capacity exponent: `2^size_bits` entries.
    pub size_bits: u8,
    pub _pad: [u8; 7],
}

/// Payload for an Untyped-carved object (Notification, `EventCount`, Time):
/// its [`CarvedId`] without the kind, which lives in the entry header.
#[repr(C)]
#[derive(Clone, Copy)]
struct CarvedPayload {
    address: u64,
    generation: u32,
    _pad: u32,
}

/// Payload of an `IRQHandler`: its [`IrqHandlerId`].
#[repr(C)]
#[derive(Clone, Copy)]
struct IrqHandlerPayload {
    line: u32,
    generation: u32,
}

/// 48-byte payload union, discriminated by `obj_type` in the header.
#[repr(C)]
#[derive(Clone, Copy)]
union KeyPayload {
    obj: ObjectPayload,
    thread: ThreadSelector,
    region: RegionPayload,
    frame: FramePayload,
    keytable: KeyTablePayload,
    carved: CarvedPayload,
    invocation: InvocationPayload,
    irq_handler: IrqHandlerPayload,
    null: [u8; PAYLOAD_BYTES],
}

/// A single entry in a thread's capability table (`KeyTable`).
///
/// 64 bytes: a 16-byte header (type, rights, flags, a 64-bit badge) and a
/// 48-byte payload, aligned to 32 bytes. Discriminated union: `obj_type`
/// selects the payload variant.
#[repr(C, align(32))]
#[derive(Clone, Copy)]
pub struct KeyEntry {
    obj_type: ObjectType,
    rights: Rights,
    /// Kernel-set marks that are never part of a capability's authority (see
    /// [`KeyEntry::FLAG_LENT`]). Zero for every entry installed by Retype,
    /// bootstrap or `CopyDerive`.
    flags: u8,
    /// The badge: zero means unbadged. Set once by a `CopyDerive` of an
    /// unbadged key, then kept by every copy.
    badge: u64,
    payload: KeyPayload,
}

/// Bytes in the payload union: the 64-byte entry less its 16-byte header.
const PAYLOAD_BYTES: usize = 48;

// Verify sizes at compile time
const _: () = assert!(core::mem::size_of::<KeyEntry>() == 64);
const _: () = assert!(core::mem::size_of::<KeyPayload>() == PAYLOAD_BYTES);
const _: () = assert!(core::mem::size_of::<RegionPayload>() == 16);
const _: () = assert!(core::mem::size_of::<FramePayload>() == 32);
const _: () = assert!(core::mem::size_of::<KeyTablePayload>() == 24);
const _: () = assert!(core::mem::size_of::<CarvedPayload>() == 16);
const _: () = assert!(core::mem::size_of::<IrqHandlerPayload>() == 8);
const _: () = assert!(core::mem::size_of::<InvocationPayload>() == PAYLOAD_BYTES);
const _: () = assert!(core::mem::offset_of!(KeyEntry, badge) == 8);
const _: () = assert!(core::mem::offset_of!(KeyEntry, payload) == 16);
const _: () = assert!(core::mem::size_of::<ThreadSelector>() == 24);
const _: () = assert!(core::mem::align_of::<ThreadSelector>() == 8);
const _: () = assert!(core::mem::align_of::<KeyEntry>() == 32);

/// Kinds whose capabilities carry a [`CarvedId`] (`CarvedPayload`).
fn is_carved_object_kind(kind: ObjectType) -> bool {
    matches!(
        kind,
        ObjectType::NOTIFICATION
            | ObjectType::EVENT_COUNT
            | ObjectType::WAIT_SET
            | ObjectType::TIME
            | ObjectType::SCHEDULER
            | ObjectType::PAGE_TABLE
            | ObjectType::ADDRESS_SPACE
    )
}

/// Minimum alignment bits for watermark shift (16-byte alignment).
const MIN_ALIGN_BITS: u32 = 4;

/// Watermark encoding granularity in bytes: stored watermarks are always
/// multiples of this (see `RegionPayload::set_watermark_bytes`), so every
/// allocation end committed to an Untyped's watermark must be aligned up to
/// it or the encoding would silently discard the remainder and let the next
/// allocation overlap.
pub(crate) const MIN_ALIGN: usize = 1 << MIN_ALIGN_BITS;

impl KeyEntry {
    /// Entry mark set only on the borrowed entry `Invocation.CallWithKey`
    /// places in the callee's table. A `LENT` entry may be invoked, used as a
    /// destination-table key, deleted, and lent onward through
    /// `CallWithKey`, but it is never the source of `CopyDerive` or `Move`, so the
    /// callee cannot keep it beyond the call. The kernel removes it when the
    /// call returns.
    pub const FLAG_LENT: u8 = 1;

    /// Whether this is a borrowed entry placed by `CallWithKey`.
    #[inline]
    pub const fn is_lent(&self) -> bool {
        self.flags & Self::FLAG_LENT != 0
    }

    /// The kinds `CopyDerive`, `Move` and `CallWithKey` may carry: `KeyTable`,
    /// `Frame` and the debug-gated `DebugConsole`. Other kinds remain
    /// unsupported until their lifecycle contracts exist.
    pub fn is_derivable_kind(obj_type: ObjectType) -> bool {
        obj_type == ObjectType::KEY_TABLE || obj_type == ObjectType::FRAME || {
            #[cfg(feature = "debug_kernel")]
            {
                obj_type == ObjectType::DEBUG_CONSOLE
            }
            #[cfg(not(feature = "debug_kernel"))]
            {
                false
            }
        }
    }

    /// The borrowed entry `CallWithKey` places for this lent entry: the same
    /// object and payload with exactly this entry's rights (including `LEND`,
    /// so it can be lent onward), marked [`FLAG_LENT`](Self::FLAG_LENT). A
    /// Frame's mapping record is not carried over, as for any derived copy.
    pub fn borrowed_copy(&self) -> Self {
        let mut borrowed = self.derive(self.rights);
        borrowed.flags = Self::FLAG_LENT;
        borrowed
    }

    /// The key-placement window of an `Invocation`: where `CallWithKey`
    /// places a lent key in the target's table. Empty if it accepts none.
    pub fn invocation_placement_window(&self) -> Result<PlacementWindow, CapError> {
        if self.obj_type != ObjectType::INVOCATION {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::INVOCATION,
                found: self.obj_type,
            });
        }
        // SAFETY: the Invocation type check selects the invocation payload,
        // whose constructor initializes the window.
        let payload = unsafe { self.payload.invocation };
        Ok(PlacementWindow::new(
            payload.placement_first,
            payload.placement_count,
        ))
    }
}

impl KeyEntry {
    /// Create a null/empty entry.
    pub const fn null() -> Self {
        Self {
            obj_type: ObjectType::NULL,
            rights: Rights::empty(),
            badge: 0,
            flags: 0,
            payload: KeyPayload {
                null: [0_u8; PAYLOAD_BYTES],
            },
        }
    }

    /// Create an identity-based capability entry (most object types).
    ///
    /// The entry stores only the checked `ObjectId`; dereferencing requires
    /// the owning access context (see `doc/capabilities-design.md` §3).
    pub fn new<T: NucleusObject>(id: ObjectId, rights: Rights, badge: u64) -> Self {
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
        badge: u64,
    ) -> Result<Self, CapError> {
        if matches!(
            obj_type,
            ObjectType::NULL
                | ObjectType::UNTYPED
                | ObjectType::FRAME
                | ObjectType::KEY_TABLE
                | ObjectType::INVOCATION
                | ObjectType::THREAD
                | ObjectType::IRQ_CONTROL
                | ObjectType::IRQ_HANDLER
        ) || is_carved_object_kind(obj_type)
        {
            return Err(CapError::InvalidObjectType(obj_type));
        }
        if obj_type.is_arch() {
            ArchType::try_from(obj_type)?;
        } else {
            CoreType::try_from(obj_type)?;
        }
        Ok(Self {
            obj_type,
            rights,
            badge,
            flags: 0,
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

    /// Create an inline Untyped capability (no pool allocation). A device
    /// Untyped gets the strictest ceiling, `DeviceStrict`.
    pub fn new_untyped(paddr: u64, size_bits: u8, is_device: bool, rights: Rights) -> Self {
        let ceiling = if is_device {
            FrameAttribute::DeviceStrict
        } else {
            FrameAttribute::Default
        };
        Self::new_untyped_with_ceiling(paddr, size_bits, is_device, ceiling, rights)
    }

    /// Create an inline device Untyped whose Frames may be mapped up to
    /// `ceiling` (`DeviceStrict`, `DevicePosted` or `WriteCombining`).
    pub fn new_device_untyped(
        paddr: u64,
        size_bits: u8,
        ceiling: FrameAttribute,
        rights: Rights,
    ) -> Self {
        Self::new_untyped_with_ceiling(paddr, size_bits, true, ceiling, rights)
    }

    fn new_untyped_with_ceiling(
        paddr: u64,
        size_bits: u8,
        is_device: bool,
        ceiling: FrameAttribute,
        rights: Rights,
    ) -> Self {
        Self {
            obj_type: ObjectType::UNTYPED,
            rights,
            badge: 0,
            flags: 0,
            payload: KeyPayload {
                region: RegionPayload {
                    paddr,
                    state: 0, // watermark starts at 0
                    size_bits,
                    is_device,
                    ceiling: ceiling.code(),
                    _pad: 0,
                },
            },
        }
    }

    /// Create an inline Frame capability (no pool allocation). A device Frame
    /// gets the strictest ceiling, `DeviceStrict`.
    pub fn new_frame(paddr: u64, size_bits: u8, is_device: bool, rights: Rights) -> Self {
        let ceiling = if is_device {
            FrameAttribute::DeviceStrict
        } else {
            FrameAttribute::Default
        };
        Self::new_frame_with_ceiling(paddr, size_bits, is_device, ceiling, rights)
    }

    /// Create an inline device Frame whose mappings may use any attribute up
    /// to `ceiling`.
    pub fn new_device_frame(
        paddr: u64,
        size_bits: u8,
        ceiling: FrameAttribute,
        rights: Rights,
    ) -> Self {
        Self::new_frame_with_ceiling(paddr, size_bits, true, ceiling, rights)
    }

    fn new_frame_with_ceiling(
        paddr: u64,
        size_bits: u8,
        is_device: bool,
        ceiling: FrameAttribute,
        rights: Rights,
    ) -> Self {
        let flags = if is_device {
            FRAME_IS_DEVICE | (ceiling.code() << FRAME_CEILING_SHIFT)
        } else {
            0
        };
        Self {
            obj_type: ObjectType::FRAME,
            rights,
            badge: 0,
            flags: 0,
            payload: KeyPayload {
                frame: FramePayload {
                    paddr,
                    vaddr: 0,
                    as_address: 0,
                    as_generation: 0,
                    size_bits,
                    flags,
                    _pad: 0,
                },
            },
        }
    }

    /// Create a capability for a carved `KeyTable` object (Retype-created).
    ///
    /// The payload is the table's carved identity (from Retype or the boot
    /// carve) plus its guard and capacity exponent; the kind is the entry
    /// header. The guard and `size_bits` are fixed at table creation and
    /// copied verbatim by derivation.
    pub fn new_keytable(
        table: CarvedId,
        guard: u32,
        size_bits: u8,
        rights: Rights,
        badge: u64,
    ) -> Self {
        debug_assert_eq!(table.kind, ObjectType::KEY_TABLE);
        Self {
            obj_type: ObjectType::KEY_TABLE,
            rights,
            badge,
            flags: 0,
            payload: KeyPayload {
                keytable: KeyTablePayload {
                    address: table.address,
                    generation: table.generation,
                    guard,
                    size_bits,
                    _pad: [0; 7],
                },
            },
        }
    }

    /// Check if this entry is valid (not null).
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.obj_type != ObjectType::NULL
    }

    /// Create a capability for an Untyped-carved object; the kind comes from
    /// its identity.
    pub fn new_carved(id: CarvedId, rights: Rights, badge: u64) -> Self {
        debug_assert!(is_carved_object_kind(id.kind));
        Self {
            obj_type: id.kind,
            rights,
            badge,
            flags: 0,
            payload: KeyPayload {
                carved: CarvedPayload {
                    address: id.address,
                    generation: id.generation,
                    _pad: 0,
                },
            },
        }
    }

    /// The identity of an Untyped-carved object (Notification, `EventCount`,
    /// Time, Scheduler, `PageTable`, `AddressSpace`); `TypeMismatch` for any
    /// other kind. Resolving it goes through
    /// the access context, which checks the object's header.
    pub fn carved_id(&self) -> Result<CarvedId, CapError> {
        if !is_carved_object_kind(self.obj_type) {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::NOTIFICATION,
                found: self.obj_type,
            });
        }
        // SAFETY: carved-object kinds are only constructed by `new_carved`.
        let carved = unsafe { self.payload.carved };
        Ok(CarvedId {
            kind: self.obj_type,
            address: carved.address,
            generation: carved.generation,
        })
    }

    /// The carved identity an `AddressSpace` capability names;
    /// `TypeMismatch` for any other kind.
    pub fn address_space_id(&self) -> Result<AddressSpaceId, CapError> {
        self.carved_id_of(ObjectType::ADDRESS_SPACE)
    }

    /// The carved identity of the metadata record a `PageTable` capability
    /// names; `TypeMismatch` for any other kind.
    pub fn page_table_id(&self) -> Result<PageTableId, CapError> {
        self.carved_id_of(ObjectType::PAGE_TABLE)
    }

    /// The carved identity of an entry of kind `kind`.
    fn carved_id_of(&self, kind: ObjectType) -> Result<CarvedId, CapError> {
        if self.obj_type != kind {
            return Err(CapError::TypeMismatch {
                expected: kind,
                found: self.obj_type,
            });
        }
        self.carved_id()
    }

    /// Check if this is an inline region type (Untyped or Frame).
    #[inline]
    pub fn is_region(&self) -> bool {
        self.obj_type == ObjectType::UNTYPED || self.obj_type == ObjectType::FRAME
    }

    /// Create an `Invocation` capability targeting an entry point in an
    /// `AddressSpace`. The capability starts with `CALL` and `BADGE` authority and no
    /// badge: its holder can call it and mint badged copies; a copy that drops
    /// `BADGE` can only call.
    /// The nonzero entry and validated numeric stack contract are mandatory;
    /// `placement_window` is where `CallWithKey` places a lent key (empty for
    /// an Invocation that accepts none).
    pub fn new_invocation(
        address_space: AddressSpaceId,
        function_address: NonZero<u64>,
        stack_extent: InvocationStackExtent,
        placement_window: PlacementWindow,
    ) -> Self {
        debug_assert_eq!(address_space.kind, ObjectType::ADDRESS_SPACE);
        Self {
            obj_type: ObjectType::INVOCATION,
            rights: Rights(Rights::CALL | Rights::BADGE),
            badge: 0,
            flags: 0,
            payload: KeyPayload {
                invocation: InvocationPayload {
                    function_address,
                    address_space_address: address_space.address,
                    stack_base: stack_extent.base(),
                    stack_end: stack_extent.end(),
                    minimum_headroom_units: stack_extent.minimum_headroom_units(),
                    address_space_generation: address_space.generation,
                    placement_first: placement_window.first(),
                    placement_count: placement_window.count(),
                },
            },
        }
    }

    /// Create a named Thread capability.
    pub fn new_thread(id: ThreadId, rights: Rights, badge: u64) -> Self {
        Self {
            obj_type: ObjectType::THREAD,
            rights,
            badge,
            flags: 0,
            payload: KeyPayload {
                thread: ThreadSelector::Named(id),
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
            flags: 0,
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

    /// The Thread a named Thread capability names; `InvalidOperation` for the
    /// current-relative Return key, `TypeMismatch` for another kind.
    pub fn thread_id(&self) -> Result<ThreadId, CapError> {
        match self.thread_selector()? {
            ThreadSelector::Named(id) => Ok(id),
            ThreadSelector::CurrentReturnOnly => Err(CapError::InvalidOperation),
        }
    }

    /// Create the `IRQControl` capability. There is one interrupt-line
    /// namespace, so the entry names nothing beyond its kind.
    pub fn new_irq_control(rights: Rights) -> Self {
        Self {
            obj_type: ObjectType::IRQ_CONTROL,
            rights,
            badge: 0,
            flags: 0,
            payload: KeyPayload {
                null: [0_u8; PAYLOAD_BYTES],
            },
        }
    }

    /// Create an `IRQHandler` capability for the handler `id`.
    pub fn new_irq_handler(id: IrqHandlerId, rights: Rights) -> Self {
        Self {
            obj_type: ObjectType::IRQ_HANDLER,
            rights,
            badge: 0,
            flags: 0,
            payload: KeyPayload {
                irq_handler: IrqHandlerPayload {
                    line: id.line,
                    generation: id.generation,
                },
            },
        }
    }

    /// The handler an `IRQHandler` capability names; `TypeMismatch` for
    /// another kind.
    pub fn irq_handler_id(&self) -> Result<IrqHandlerId, CapError> {
        if self.obj_type != ObjectType::IRQ_HANDLER {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::IRQ_HANDLER,
                found: self.obj_type,
            });
        }
        // SAFETY: only `new_irq_handler` constructs IRQHandler entries, and
        // the kind check selects its variant.
        let payload = unsafe { self.payload.irq_handler };
        Ok(IrqHandlerId {
            line: payload.line,
            generation: payload.generation,
        })
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
    /// A carved `KeyTable` is named through its own payload (identity plus
    /// guard and capacity) rather than a pooled identity; see
    /// `doc/capabilities-design.md` §3.
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
        derived.flags = 0;
        if derived.obj_type == ObjectType::FRAME {
            // SAFETY: the FRAME type check selects the frame payload variant.
            unsafe {
                derived.payload.frame.clear_mapped();
            }
        }
        derived
    }

    /// This entry with `badge` set. The caller has established that the key
    /// may be badged (see `CopyDerive`): this is a pure representation
    /// transform, not an authority check.
    #[inline]
    #[must_use]
    pub const fn with_badge(mut self, badge: u64) -> Self {
        self.badge = badge;
        self
    }

    /// Whether this kind can be badged on derive: `Invocation` (the badge
    /// reaches the exporter) and `Notification` (the badge is the signalled
    /// bits).
    #[inline]
    pub fn is_badgeable_kind(obj_type: ObjectType) -> bool {
        obj_type == ObjectType::INVOCATION || obj_type == ObjectType::NOTIFICATION
    }

    /// Get badge value.
    #[inline]
    pub fn badge(&self) -> u64 {
        self.badge
    }

    /// Get the checked object identity (identity-based caps only).
    ///
    /// This is a handle, not a reference: dereferencing requires the owning
    /// access context, which validates the identity against authoritative
    /// pool metadata first. Returns error on region types and carved-object
    /// kinds — use `as_region()` / `keytable_id()` instead.
    #[inline]
    pub fn object_id(&self) -> Result<ObjectId, CapError> {
        if self.is_region()
            || self.obj_type == ObjectType::THREAD
            || self.is_carved()
            || is_carved_object_kind(self.obj_type)
            || self.obj_type == ObjectType::INVOCATION
            || self.obj_type == ObjectType::IRQ_CONTROL
            || self.obj_type == ObjectType::IRQ_HANDLER
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

    /// Get the carved `KeyTable`'s identity (`KeyTable` caps only).
    ///
    /// Dereferencing goes through the owning access context, which validates
    /// the identity against the table's header and ties the resulting guard
    /// to the locked invocation.
    #[inline]
    pub fn keytable_id(&self) -> Result<CarvedId, CapError> {
        if self.obj_type != ObjectType::KEY_TABLE {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::KEY_TABLE,
                found: self.obj_type,
            });
        }
        // SAFETY: keytable variant guaranteed by the type check above.
        let payload = unsafe { self.payload.keytable };
        Ok(CarvedId {
            kind: ObjectType::KEY_TABLE,
            address: payload.address,
            generation: payload.generation,
        })
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
    pub fn invocation_target(&self) -> Result<(AddressSpaceId, NonZero<u64>), CapError> {
        if self.obj_type != ObjectType::INVOCATION {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::INVOCATION,
                found: self.obj_type,
            });
        }
        // SAFETY: the Invocation type check selects the invocation payload.
        let payload = unsafe { self.payload.invocation };
        Ok((
            CarvedId {
                kind: ObjectType::ADDRESS_SPACE,
                address: payload.address_space_address,
                generation: payload.address_space_generation,
            },
            payload.function_address,
        ))
    }

    /// Read the immutable numeric target stack contract of an `Invocation`.
    pub fn invocation_stack_extent(&self) -> Result<InvocationStackExtent, CapError> {
        if self.obj_type != ObjectType::INVOCATION {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::INVOCATION,
                found: self.obj_type,
            });
        }
        // SAFETY: Invocation constructors initialize the validated extent;
        // the checked tag selects that payload, and no mutable accessor exposes it.
        let payload = unsafe { self.payload.invocation };
        Ok(InvocationStackExtent::from_validated(
            payload.stack_base,
            payload.stack_end,
            payload.minimum_headroom_units,
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

    /// The loosest attribute a device Frame may be mapped with, as a ceiling;
    /// [`FrameAttribute::Default`] for a RAM Frame (which maps `Cacheable`
    /// only).
    #[inline]
    pub fn ceiling(&self) -> FrameAttribute {
        if !self.is_device() {
            return FrameAttribute::Default;
        }
        FrameAttribute::from_code(u64::from(
            (self.flags & FRAME_CEILING_MASK) >> FRAME_CEILING_SHIFT,
        ))
        .unwrap_or(FrameAttribute::DeviceStrict)
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
            address_space: CarvedId {
                kind: ObjectType::ADDRESS_SPACE,
                address: self.as_address,
                generation: self.as_generation,
            },
            vaddr: self.vaddr,
        })
    }

    /// Record that this frame cap was mapped into `address_space` at `vaddr`.
    /// The vaddr must be aligned to the frame size.
    #[inline]
    pub fn set_mapped(&mut self, address_space: AddressSpaceId, vaddr: u64) {
        debug_assert_eq!(address_space.kind, ObjectType::ADDRESS_SPACE);
        debug_assert_eq!(vaddr & (self.size() as u64 - 1), 0);
        self.as_address = address_space.address;
        self.as_generation = address_space.generation;
        self.vaddr = vaddr;
        self.flags |= FRAME_MAPPED;
    }

    /// Clear the mapping record (frame was unmapped).
    #[inline]
    pub fn clear_mapped(&mut self) {
        self.flags &= !FRAME_MAPPED;
        self.vaddr = 0;
        self.as_address = 0;
        self.as_generation = 0;
    }
}

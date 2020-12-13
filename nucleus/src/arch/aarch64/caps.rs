/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

//! Implementation of seL4-like capabilities.

// DerivationTree nodes record the tree of inheritance for caps:
// See the picture on derivation from seL4 manual for how this works: each cap contains a ref to
// DerivationTree node, which records the previous cap and the following cap(s).

// ☐ Rust implementation of capabilities - ?
//   ☐ Need to implement in kernel entries storage and lookup
//   ☐ cte = cap table entry (a cap_t plus mdb_node_t)
//   ☐ mdb = ? (mdb_node_new)
//   ☐ sameObjectAs()

//     cap_get_capType();//generated
//     lookupCapAndSlot();

// cap_domain_cap_new() etc //generated
// create_mapped_it_frame_cap(); //vspace.c

// pptr_of_cap(); -- extracts cap.pptr from cnode_cap
// deriveCap();

use {
    core::{convert::TryFrom, fmt},
    paste::paste,
    register::{register_bitfields, LocalRegisterCopy},
    snafu::Snafu,
};

//==================
// Caps definitions
//==================

register_bitfields! {
    u128,
    NullCap [
        Type OFFSET(64) NUMBITS(5) [
            value = 0
        ]
    ],
    // The combination of freeIndex and blockSize must match up with the
    // definitions of MIN_SIZE_BITS and MAX_SIZE_BITS
    // -- https://github.com/seL4/seL4/blob/master/include/object/structures_32.bf#L18
    //
    // /* It is assumed that every untyped is within seL4_MinUntypedBits and seL4_MaxUntypedBits
    //  * (inclusive). This means that every untyped stored as seL4_MinUntypedBits
    //  * subtracted from its size before it is stored in capBlockSize, and
    //  * capFreeIndex counts in chunks of size 2^seL4_MinUntypedBits. The seL4_MaxUntypedBits
    //  * is the minimal untyped that can be stored when considering both how
    //  * many bits of capBlockSize there are, and the largest offset that can
    //  * be stored in capFreeIndex */
    // +#define MAX_FREE_INDEX(sizeBits) (BIT( (sizeBits) - seL4_MinUntypedBits ))
    // +#define FREE_INDEX_TO_OFFSET(freeIndex) ((freeIndex)<<seL4_MinUntypedBits)
    // #define GET_FREE_REF(base,freeIndex) ((word_t)(((word_t)(base)) + FREE_INDEX_TO_OFFSET(freeIndex)))
    // #define GET_FREE_INDEX(base,free) (((word_t)(free) - (word_t)(base))>>seL4_MinUntypedBits)
    // #define GET_OFFSET_FREE_PTR(base, offset) ((void *)(((word_t)(base)) + (offset)))
    // +#define OFFSET_TO_FREE_INDEX(offset) ((offset)>>seL4_MinUntypedBits)
    //
    // exception_t decodeUntypedInvocation(word_t invLabel, word_t length,
    //                                     cte_t *slot, cap_t cap,
    //                                     extra_caps_t excaps, bool_t call,
    //                                     word_t *buffer);
    // exception_t invokeUntyped_Retype(cte_t *srcSlot, bool_t reset,
    //                                  void *retypeBase, object_t newType,
    //                                  word_t userSize, slot_range_t destSlots,
    //                                  bool_t deviceMemory);
    // // -- https://github.com/seL4/seL4/blob/master/src/object/untyped.c#L276
    // -- https://github.com/seL4/seL4/blob/master/include/object/untyped.h
    //
    // /* Untyped size limits */
    // #define seL4_MinUntypedBits 4
    // #define seL4_MaxUntypedBits 47
    // -- https://github.com/seL4/seL4/blob/master/libsel4/sel4_arch_include/aarch64/sel4/sel4_arch/constants.h#L234
    //
    // /*
    //  * Determine where in the Untyped region we should start allocating new
    //  * objects.
    //  *
    //  * If we have no children, we can start allocating from the beginning of
    //  * our untyped, regardless of what the "free" value in the cap states.
    //  * (This may happen if all of the objects beneath us got deleted).
    //  *
    //  * If we have children, we just keep allocating from the "free" value
    //  * recorded in the cap.
    //  */
    // -- https://github.com/seL4/seL4/blob/master/src/object/untyped.c#L175
    // /*
    //  * Determine the maximum number of objects we can create, and return an
    //  * error if we don't have enough space.
    //  *
    //  * We don't need to worry about alignment in this case, because if anything
    //  * fits, it will also fit aligned up (by packing it on the right hand side
    //  * of the untyped).
    //  */
    // -- https://github.com/seL4/seL4/blob/master/src/object/untyped.c#L196

    UntypedCap [
        /// Index of the first unoccupied byte within this Untyped.
        /// This index is limited between MIN_UNTYPED_BITS and max bits number in BlockSizePower.
        /// To occupy less bits, the free index is shifted right by MIN_UNTYPED_BITS.
        ///
        /// Free index is used only if this untyped has children, which may be occupying only
        /// part of its space.
        /// This means an Untyped can be retyped multiple times as long as there is
        /// free space left in it.
        FreeIndexShifted OFFSET(0) NUMBITS(48) [],
        /// Device mapped untypeds cannot be touched by the kernel.
        IsDevice OFFSET(57) NUMBITS(1) [],
        /// Untyped is 2**BlockSizePower bytes in size
        BlockSizePower OFFSET(58) NUMBITS(6) [],
        Type OFFSET(64) NUMBITS(5) [
            value = 2
        ],
        /// Physical address of untyped.
        Ptr OFFSET(80) NUMBITS(48) [],
    ],
    EndpointCap [
        Badge OFFSET(0) NUMBITS(64) [],
        Type OFFSET(64) NUMBITS(5) [
            value = 4
        ],
        CanGrantReply OFFSET(69) NUMBITS(1) [],
        CanGrant OFFSET(70) NUMBITS(1) [],
        CanReceive OFFSET(71) NUMBITS(1) [],
        CanSend OFFSET(72) NUMBITS(1) [],
        Ptr OFFSET(80) NUMBITS(48) [],
    ],
    NotificationCap [ // @todo replace with Event
        Badge OFFSET(0) NUMBITS(64) [],
        Type OFFSET(64) NUMBITS(5) [
            value = 6
        ],
        CanReceive OFFSET(69) NUMBITS(1) [],
        CanSend OFFSET(70) NUMBITS(1) [],
        Ptr OFFSET(80) NUMBITS(48) [],
    ],
    ReplyCap [
        TCBPtr OFFSET(0) NUMBITS(64) [],
        Type OFFSET(64) NUMBITS(5) [
            value = 8
        ],
        ReplyCanGrant OFFSET(126) NUMBITS(1) [],
        ReplyMaster OFFSET(127) NUMBITS(1) [],
    ],
    CapNodeCap [
        Guard OFFSET(0) NUMBITS(64) [],
        Type OFFSET(64) NUMBITS(5) [
            value = 10
        ],
        GuardSize OFFSET(69) NUMBITS(6) [],
        Radix OFFSET(75) NUMBITS(6) [],
        Ptr OFFSET(81) NUMBITS(47) [],
    ],
    ThreadCap [
        Type OFFSET(64) NUMBITS(5) [
            value = 12
        ],
        TCBPtr OFFSET(80) NUMBITS(48) [],
    ],
    IrqControlCap [
        Type OFFSET(64) NUMBITS(5) [
            value = 14
        ]
    ],
    IrqHandlerCap [
        Irq OFFSET(52) NUMBITS(12) [],
        Type OFFSET(64) NUMBITS(5) [
            value = 16
        ]
    ],
    ZombieCap [
        ZombieID OFFSET(0) NUMBITS(64) [],
        Type OFFSET(64) NUMBITS(5) [
            value = 18
        ],
        ZombieType OFFSET(121) NUMBITS(7) []
    ],
    DomainCap [
        Type OFFSET(64) NUMBITS(5) [
            value = 20
        ]
    ],
    // https://ts.data61.csiro.au/publications/csiro_full_text/Lyons_MAH_18.pdf
    // Resume objects, modelled after KeyKOS [Bomberger et al.1992], are a new object type
    // that generalise the “reply capabilities” of baseline seL4. These were capabilities
    // to virtual objects created by the kernel on-the-fly in seL4’s RPC-style call() operation,
    // which sends a message to an endpoint and blocks on a reply. The receiver of the message
    // (i.e. the server) receives the reply capability in a magic “reply slot” in its
    // capability space. The server replies by invoking that capability. Resume objects
    // remove the magic by explicitly representing the reply channel (and the SC-donation chain).
    // They also provide more efficient support for stateful servers that handle concurrent client
    // sessions.
    // The introduction of Resume objects requires some changes to the IPC system-call API.
    // The client-style call() operation is unchanged, but server-side equivalent, ReplyRecv
    // (previously ReplyWait) replies to a previous request and then blocks on the next one.
    // It now must provide an explicit Resume capability; on the send phase, that capability
    // identifies the client and returns the SC if appropriate, on the receive phase it is
    // populated with new values. The new API makes stateful server implementation more efficient.
    // In baseline seL4, the server would have to use at least two extra system calls to save the
    // reply cap and later move it back into its magic slot, removing the magic also removes
    // the need for the extra system calls.

    ResumeCap [
        Type OFFSET(64) NUMBITS(5) [
            value = 22
        ]
    ]
}

// mod aarch64 {

// ARM-specific caps
register_bitfields! {
    u128,
    FrameCap [
        MappedASID OFFSET(0) NUMBITS(16) [],
        BasePtr OFFSET(16) NUMBITS(48) [], // PhysAddr
        Type OFFSET(64) NUMBITS(5) [
            value = 1
        ],
        Size OFFSET(69) NUMBITS(2) [],
        VMRights OFFSET(71) NUMBITS(2) [],
        IsDevice OFFSET(73) NUMBITS(1) [],
        MappedAddress OFFSET(80) NUMBITS(48) [] // VirtAddr
    ],
    PageTableCap [
        MappedASID OFFSET(0) NUMBITS(16) [],
        BasePtr OFFSET(16) NUMBITS(48) [], // PhysAddr
        Type OFFSET(64) NUMBITS(5) [
            value = 3
        ],
        IsMapped OFFSET(79) NUMBITS(1) [],
        MappedAddress OFFSET(80) NUMBITS(28) [] // VirtAddr
    ],
    PageDirectoryCap [
        MappedASID OFFSET(0) NUMBITS(16) [],
        BasePtr OFFSET(16) NUMBITS(48) [], // PhysAddr
        Type OFFSET(64) NUMBITS(5) [
            value = 5
        ],
        IsMapped OFFSET(79) NUMBITS(1) [],
        MappedAddress OFFSET(80) NUMBITS(19) [] // VirtAddr
    ],
    PageUpperDirectoryCap [
        MappedASID OFFSET(0) NUMBITS(16) [],
        BasePtr OFFSET(16) NUMBITS(48) [], // PhysAddr
        Type OFFSET(64) NUMBITS(5) [
            value = 7
        ],
        IsMapped OFFSET(79) NUMBITS(1) [],
        MappedAddress OFFSET(80) NUMBITS(10) [] // VirtAddr
    ],
    PageGlobalDirectoryCap [
        MappedASID OFFSET(0) NUMBITS(16) [],
        BasePtr OFFSET(16) NUMBITS(48) [], // PhysAddr
        Type OFFSET(64) NUMBITS(5) [
            value = 9
        ],
        IsMapped OFFSET(79) NUMBITS(1) []
    ],
    AsidControlCap [
        Type OFFSET(64) NUMBITS(5) [
            value = 11
        ]
    ],
    AsidPoolCap [
        Type OFFSET(64) NUMBITS(5) [
            value = 13
        ],
        ASIDBase OFFSET(69) NUMBITS(16) [],
        ASIDPool OFFSET(91) NUMBITS(37) []
    ]
    // For HYP mode:
    // VCpuCap [
    //     Type OFFSET(64) NUMBITS(5) [], // 15
    //     VCPUPtr OFFSET(80) NUMBITS(48) [],
    // ],
}

//================
// Kernel objects
//================

//-- Mapping database (MDB) node: size = 16 bytes
//block mdb_node {
//padding 16 -- highest in word[1]
//field_high mdbNext 46  <-- field_high means "will need sign-extension", also value has 2 lower bits just dropped when setting
//field mdbRevocable 1 -- second bit in word[1]
//field mdbFirstBadged 1 -- lowest in word[1]
//
//field mdbPrev 64 -- enter lowest word (word[0]) in sel4
//}

register_bitfields! {
    u128,
    CapDerivationNode [
        FirstBadged OFFSET(0) NUMBITS(1) [
            Disable = 0,
            Enable = 1
        ],
        Revocable OFFSET(1) NUMBITS(1) [
            Disable = 0,
            Enable = 1
        ],
        // -- 2 bits still free here --
        // Next CTE node address -- per cteInsert this is address of the entire CTE slot
        // cap derivation slots are supposedly aligned in u128 boundary (16 bytes) this means we can
        // drop bottom 4 bits from it in these fields.
        Next OFFSET(4) NUMBITS(44) [], // 16-bytes-aligned, size of canonical phys address is 48 bits
        // -- 16 bits still free here --
        // -- New word doundary --
        // -- 4 bits still free here --
        // Prev CTE node address -- per cteInsert this is address of the entire CTE slot
        Prev OFFSET(68) NUMBITS(44) []
        // -- 16 bits still free here --
    ]
}

/// Opaque capability object, manipulated by the kernel.
pub trait Capability {
    ///
    /// Is this capability arch-specific?
    ///
    fn is_arch(&self) -> bool;

    ///
    /// Retrieve this capability as scalar value.
    ///
    fn as_u128(&self) -> u128;
}

macro_rules! capdefs {
    ($($name:ident),*) => {
        paste! {
            $(
            #[doc = "Wrapper representing `" $name "Capability`."]
            pub struct [<$name Capability>](LocalRegisterCopy<u128, [<$name Cap>]::Register>);
            impl Capability for [<$name Capability>] {
                #[inline]
                fn as_u128(&self) -> u128 {
                    self.0.into()
                }
                #[inline]
                fn is_arch(&self) -> bool {
                    ([<$name Cap>]::Type::Value::value as u8) % 2 != 0
                }
            }
            impl TryFrom<u128> for [<$name Capability>] {
                type Error = CapError;
                fn try_from(v: u128) -> Result<[<$name Capability>], Self::Error> {
                    let reg = LocalRegisterCopy::<_, [<$name Cap>]::Register>::new(v);
                    if reg.read([<$name Cap>]::Type) == u128::from([<$name Cap>]::Type::value) {
                        Ok([<$name Capability>](LocalRegisterCopy::new(v)))
                    } else {
                        Err(Self::Error::InvalidCapabilityType)
                    }
                }
            }
            impl From<[<$name Capability>]> for u128 {
                #[inline]
                fn from(v: [<$name Capability>]) -> u128 {
                    v.as_u128()
                }
            }
            )*
        }
    }
}

// Generic caps - @todo move to src/caps/
capdefs! {
    Null, Untyped, Endpoint,
    Notification, Reply, CapNode,
    Thread, IrqControl, IrqHandler,
    Zombie, Domain, Resume
}

// ARM-specific caps
capdefs! {
    Frame, PageTable, PageDirectory,
    PageUpperDirectory, PageGlobalDirectory,
    AsidControl, AsidPool
}

impl NullCapability {
    /// Create a Null capability.
    ///
    /// Such capabilities are invalid and can not be used for anything.
    pub fn new() -> NullCapability {
        NullCapability(LocalRegisterCopy::new(u128::from(NullCap::Type::value)))
    }
}

// @todo retyping a device capability requires specifying memory base exactly, can't just pick next frame?

/// Capability to a block of untyped memory.
/// Can be retyped into more usable types.
impl UntypedCapability {
    const MIN_BITS: usize = 4;
    const MAX_BITS: usize = 47;

    pub fn is_device(&self) -> bool {
        self.0.read(UntypedCap::IsDevice) == 1
    }

    pub fn block_size(&self) -> usize {
        1 << self.0.read(UntypedCap::BlockSizePower)
    }
    // FreeIndex OFFSET(0) NUMBITS(48) [],
    pub fn free_area_offset(&self) -> usize {
        use core::convert::TryInto;
        Self::free_index_to_offset(
            self.0
                .read(UntypedCap::FreeIndexShifted)
                .try_into()
                .unwrap(),
        )
    }

    pub fn base(&self) -> PhysAddr {
        self.0.read(UntypedCap::Ptr)
    }

    // #define MAX_FREE_INDEX(sizeBits) (BIT( (sizeBits) - seL4_MinUntypedBits ))
    pub fn max_free_index_from_bits(size_bits: usize) -> usize {
        assert!(size_bits >= Self::MIN_BITS);
        assert!(size_bits <= Self::MAX_BITS);
        1 << (size_bits - Self::MIN_BITS)
    }

    // #define FREE_INDEX_TO_OFFSET(freeIndex) ((freeIndex)<<seL4_MinUntypedBits)
    fn free_index_to_offset(index: usize) -> usize {
        index << Self::MIN_BITS
    }

    // #define OFFSET_TO_FREE_INDEX(offset) ((offset)>>seL4_MinUntypedBits)
    fn offset_to_free_index(offset: usize) -> usize {
        offset >> Self::MIN_BITS
    }
}

// Endpoints support all 10 IPC variants (see COMP9242 slides by Gernot)
impl EndpointCapability {}
// Notifications support NBSend (Signal), Wait and NBWait (Poll) (see COMP9242 slides by Gernot)
// Other objects support only Call() (see COMP9242 slides by Gernot)
// Appear as (kernel-implemented) servers
//     • Each has a kernel-defined protocol
//         • operations encoded in message tag
//         • parameters passed in message words
//     • Mostly hidden behind “syscall” wrappers

// * Capability slots: 16 bytes of memory per slot (exactly one capability). --?
// CapNode describes `a given number of capability slots` with `a given guard`
// of `a given guard size` bits.

// @todo const generic on number of capabilities contained in the node? currently only contains a Cap
// capnode_cap has a pptr, guard_size, guard and radix
// this is enough to address a cap in the capnode contents
// by having a root capnode cap we can traverse the whole tree.

impl CapNodeCapability {
    /// Create new root node.
    pub fn new_root(pptr: u64) -> CapNodeCapability {
        const CONFIG_ROOT_CAPNODE_SIZE_BITS: u32 = 12;
        const WORD_BITS: u32 = 64;

        CapNodeCapability::new(
            pptr,
            CONFIG_ROOT_CAPNODE_SIZE_BITS,
            WORD_BITS - CONFIG_ROOT_CAPNODE_SIZE_BITS,
            0,
        )
    }

    //    pub const fn from_capability(cap: dyn Capability) -> CapNodeCapability {
    //        let reg = LocalRegisterCopy::<_, CapNodeCap::Register>::new(cap.as_u128());
    //        //assert_eq!(
    //        //    reg.read(CapNodeCap::Type),
    //        //    u128::from(CapNodeCap::Type::value)
    //        //);
    //        CapNodeCapability(reg)
    //    }

    /// @internal
    pub fn write_slot(&mut self, slot: usize, cap: &dyn Capability) {
        let ptr = self.0.read(CapNodeCap::Ptr);
        let size =
            (1usize << self.0.read(CapNodeCap::Radix)) * core::mem::size_of::<CapTableEntry>();
        let slice = unsafe { core::slice::from_raw_parts_mut(ptr as *mut CapTableEntry, size) };
        slice[slot].capability = cap.as_u128();
        slice[slot].derivation = DerivationTreeNode::empty()
            .set_revocable(true)
            .set_first_badged(true);
    }
}

impl CapNodeCapability {
    /// Create a capability to CapNode.
    ///
    /// CapNode capabilities allow to address a capability node tree entry.
    pub fn new(pptr: u64, radix: u32, guard_size: u32, guard: u64) -> CapNodeCapability {
        CapNodeCapability(LocalRegisterCopy::new(u128::from(
            CapNodeCap::Type::value
                + CapNodeCap::Radix.val(radix.into())
                + CapNodeCap::GuardSize.val(guard_size.into())
                + CapNodeCap::Guard.val(guard.into())
                + CapNodeCap::Ptr.val(pptr.into()),
        )))
    }
}

/// Wrapper for CapDerivationNode
#[derive(Clone, Debug)]
pub struct DerivationTreeNode(LocalRegisterCopy<u128, CapDerivationNode::Register>);

/// Errors that may happen in capability derivation tree operations.
#[derive(Debug, PartialEq, Snafu)]
pub enum DerivationTreeError {
    /// Previous link is invalid.
    InvalidPrev,
    /// Next link is invalid.
    InvalidNext,
}

// In seL4, the MDB is stored as a doubly-linked list, representing the **preorder-DFS** through
// the hierarchy of capabilities. This data structure allows easy insertion of a capability
// given its immediate ancestor or a copy, and easy checking for existence of copies and descendants.
// But when no relations are known beforehand, finding the position to place a new capability
// requires a O(n) linear scan through the list, as does finding ancestors and descendants
// of a capability given just the capability’s value. This operation is performed in
// the non-preemptable kernel, creating a scheduling hole that is problematic for real-time applications.
// To reduce the complexity of operations described above, we replace the MDB’s linked list with
// a more suitable search data structure.
// -- nevill-master-thesis Using Capabilities for OS Resource Management
// sel4: mdb_node_t
impl DerivationTreeNode {
    const ADDR_BIT_SHIFT: usize = 4;

    fn empty() -> Self {
        Self(LocalRegisterCopy::new(0))
    }

    // Unlike mdb_node_new we do not pass revocable and firstBadged flags here, they are enabled
    // using builder interface set_first_badged() and set_revocable().
    fn new(prevPtr: PhysAddr, nextPtr: PhysAddr) -> Self {
        Self::empty().set_prev(prevPtr).set_next(nextPtr)
    }

    /// Get previous link in derivation tree.
    /// Previous link exists if this is a derived capability.
    ///
    /// SAFETY: it is UB to get prev reference from a null Prev pointer.
    pub unsafe fn get_prev(&self) -> CapTableEntry {
        let ptr = (self.0.read(CapDerivationNode::Prev) << ADDR_BIT_SHIFT) as *const CapTableEntry;
        (*ptr).clone()
    }

    /// Try to get previous link in derivation tree.
    /// Previous link exists if this is a derived capability.
    pub fn try_get_prev(&self) -> Result<CapTableEntry, DerivationTreeError> {
        if self.0.read(CapDerivationNode::Prev) == 0 {
            Err(DerivationTreeError::InvalidPrev)
        } else {
            Ok(unsafe { self.get_prev() })
        }
    }

    pub fn set_prev(&mut self, prevPtr: PhysAddr) -> Self {
        self.0
            .write(CapDerivationNode::Prev(prevPtr >> ADDR_BIT_SHIFT));
        self
    }

    /// Get next link in derivation tree.
    /// Next link exists if this capability has derived capabilities or siblings.
    ///
    /// SAFETY: it is UB to get next reference from a null Next pointer.
    pub unsafe fn get_next(&self) -> CapTableEntry {
        let ptr = (self.0.read(CapDerivationNode::Next) << ADDR_BIT_SHIFT) as *const CapTableEntry;
        (*ptr).clone()
    }

    /// Try to get next link in derivation tree.
    /// Next link exists if this capability has derived capabilities or siblings.
    pub fn try_get_next(&self) -> Result<CapTableEntry, DerivationTreeError> {
        if self.0.read(CapDerivationNode::Next) == 0 {
            Err(DerivationTreeError::InvalidNext)
        } else {
            Ok(unsafe { self.get_next() })
        }
    }

    pub fn set_next(&mut self, nextPtr: PhysAddr) -> Self {
        self.0
            .write(CapDerivationNode::Next(nextPtr >> ADDR_BIT_SHIFT));
        self
    }

    /// Builder interface to modify firstBadged flag
    /// @todo Describe the firstBadged flag and what it does.
    fn set_first_badged(mut self, enable: bool) -> Self {
        self.0.modify(if enable {
            CapDerivationNode::FirstBadged::Enable
        } else {
            CapDerivationNode::FirstBadged::Disable
        });
        self
    }

    /// Builder interface to modify revocable flag
    /// @todo Describe the revocable flag and what it does.
    fn set_revocable(mut self, enable: bool) -> Self {
        self.0.modify(if enable {
            CapDerivationNode::Revocable::Enable
        } else {
            CapDerivationNode::Revocable::Disable
        });
        self
    }
}

/// Errors in capability operations.
#[derive(Debug, Snafu)]
pub enum CapError {
    /// Unable to create capability, exact reason TBD.
    CannotCreate,
    /// Capability has a type incompatible with the requested operation.
    InvalidCapabilityType,
}

// -- cte_t from seL4
// structures.h:140
// /* Capability table entry (CTE) */
// struct cte {
//     cap_t cap; // two words
//     mdb_node_t cteMDBNode; // two words
// }; // -- four words: u256, 32 bytes.
// typedef struct cte cte_t;
/// Each entry in capability tree contains capability value and its position in the derivation tree.
#[derive(Clone)]
pub struct CapTableEntry {
    capability: u128,
    derivation: DerivationTreeNode,
}

impl fmt::Debug for CapTableEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:x}", self.capability) // @todo
    }
}

impl Default for CapTableEntry {
    fn default() -> Self {
        Self::empty()
    }
}

impl CapTableEntry {
    /// Temporary for testing:
    fn empty() -> CapTableEntry {
        CapTableEntry {
            capability: 0,
            derivation: DerivationTreeNode::empty(),
        }
    }
    // We need to pass reference to the parent entry so that we can set up derivation pointers.
    // @todo should be &mut since we need to set up Next pointer in parent also.
    // @fixme this cannot work well unless we modify already allocated cap table entry in the table.
    // (otherwise Next pointer will be invalid)
    // sel4: cteInsert()
    fn derived_from(&mut self, parent: &mut CapTableEntry) {
        self.derivation.set_prev(&parent as *const CapTableEntry);
        parent.set_next(&self as *const CapTableEntry);
    }
}

/*
struct CapNodePath {
    /// Index contains `depth` lowermost bits of the path.
    index: u64,
    /// Depth specifies the remaining amount of bits left to traverse in the path.
    /// Once depth reaches zero, the selected CapNode slot is the final target.
    depth: usize,
}

struct CapNodeRootedPath {
    root: CapNode,
    path: CapNodePath,
}

// @todo just use CapNodeCap
//struct CapNodeConfig { <-- for each CapTable we would need these..
//    guard: u64,
//    guard_bits: usize,
//}

// @note src and dest are swapped here, compared to seL4 api
impl CapNode {
    // Derives a capability into a new, less powerful one, with potentially added Badge.
    fn mint(
        src: CapNodeRootedPath, // can be just CapNodePath since it's relative (is it?) to this CapNode.
        dest: CapNodePath,
        rights: CapRights,
        badge: Badge,
    ) -> Result<(), CapError> {
        unimplemented!();
    }
    // [wip] is copy a derivation too?
    fn copy(src: CapNodeRootedPath, dest: CapNodePath, rights: CapRights) -> Result<(), CapError> {
        unimplemented!();
    }
    fn r#move(src: CapNodeRootedPath, dest: CapNodePath) -> Result<(), CapError> {
        unimplemented!();
    }
    fn mutate(src: CapNodeRootedPath, dest: CapNodePath, badge: Badge) -> Result<(), CapError> {
        unimplemented!();
    }
    fn rotate(
        src: CapNodeRootedPath,
        dest: CapNodePath,
        dest_badge: Badge,
        pivot: CapNodeRootedPath,
        pivot_badge: Badge,
    ) -> Result<(), CapError> {
        unimplemented!();
    }
    fn delete(path: CapNodePath) -> Result<(), CapError> {
        unimplemented!();
    }
    fn revoke(path: CapNodePath) -> Result<(), CapError> {
        unimplemented!();
    }
    fn save_caller(r#where: CapNodePath) -> Result<(), CapError> { // save_reply_cap() in sel4
        unimplemented!();
    }
    fn cancel_badged_sends(path: CapNodePath) -> Result<(), CapError> {
        unimplemented!();
    }
}*/

/// Structure holding a number of capabilities.
// In seL4 the capnode is capability to an object called CapTable btw:
// case seL4_CapTableObject:
// return cap_cnode_cap_new(userSize, 0, 0, CTE_REF(regionBase));
struct CapTable<const SIZE_BITS: usize>
where
    [CapTableEntry; 1 << SIZE_BITS]: Sized,
{
    items: [CapTableEntry; 1 << SIZE_BITS],
}

/// Conceptually a thread’s CapSpace is the portion of the directed graph that is reachable
/// starting with the CapNode capability that is its CapSpace root.
struct CapSpace {
    // cap_space_root: CapNodePath, -- probably not a path but direct CapNode pointer??
}
//impl CapNode for CapSpace {} -- ?

impl ThreadCapability {}
impl IrqControlCapability {}
impl IrqHandlerCapability {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test_case]
    fn create_empty_cap_table() {
        let table = CapTable::<5> {
            items: Default::default(),
        };
        assert_eq!(table.items[0].capability, NullCapability::new().into());
        assert_eq!(table.items[31].capability, NullCapability::new().into());
        // Doesn't even compile:
        // assert_eq!(table.items[32].capability, NullCapability::new().into());
    }

    #[test_case]
    fn first_capability_derivation_has_no_prev_link() {
        let entry = CapTableEntry::empty();
        assert!(entry
            .derivation
            .try_get_prev()
            .contains_err(&DerivationTreeError::InvalidPrev));
    }

    // Impl strategy
    // 1. Make capabilities list
    // 2. Fill it with capabilities
    // 3. Test capability manipulation functions - mint/clone/revoke
    // 4. Validate capability path, capability contents and capability derivation chain at each step
    // 5. Start with Untyped capabilities and implement Retype()
    // typedef enum api_object { -- basic list of API types of objects:
    // seL4_UntypedObject,
    // seL4_TCBObject,
    // seL4_EndpointObject,
    // seL4_NotificationObject,
    // seL4_CapTableObject,
    // 6. Retype to TCB and implement Thread capability to run threads (in priv mode first?)
}

// @todo Use bitmatch over cap Type field?
// Could be interesting if usable. See https://github.com/porglezomp/bitmatch
// Maybe look at https://lib.rs/crates/enumflags2 too

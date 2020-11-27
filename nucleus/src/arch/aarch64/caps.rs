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
    UntypedCap [
        FreeIndex OFFSET(0) NUMBITS(48) [],
        IsDevice OFFSET(57) NUMBITS(1) [],
        BlockSize OFFSET(58) NUMBITS(6) [],
        Type OFFSET(64) NUMBITS(5) [
            value = 2
        ],
        Ptr OFFSET(80) NUMBITS(48) []
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
        Ptr OFFSET(80) NUMBITS(48) []
    ],
    NotificationCap [ // @todo replace with Event
        Badge OFFSET(0) NUMBITS(64) [],
        Type OFFSET(64) NUMBITS(5) [
            value = 6
        ],
        CanReceive OFFSET(69) NUMBITS(1) [],
        CanSend OFFSET(70) NUMBITS(1) [],
        Ptr OFFSET(80) NUMBITS(48) []
    ],
    ReplyCap [
        TCBPtr OFFSET(0) NUMBITS(64) [],
        Type OFFSET(64) NUMBITS(5) [
            value = 8
        ],
        ReplyCanGrant OFFSET(126) NUMBITS(1) [],
        ReplyMaster OFFSET(127) NUMBITS(1) []
    ],
    CapNodeCap [
        Guard OFFSET(0) NUMBITS(64) [],
        Type OFFSET(64) NUMBITS(5) [
            value = 10
        ],
        GuardSize OFFSET(69) NUMBITS(6) [],
        Radix OFFSET(75) NUMBITS(6) [],
        Ptr OFFSET(81) NUMBITS(47) []
    ],
    ThreadCap [
        Type OFFSET(64) NUMBITS(5) [
            value = 12
        ],
        TCBPtr OFFSET(80) NUMBITS(48) []
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
//padding 16
//field_high mdbNext 46
//field mdbRevocable 1
//field mdbFirstBadged 1
//
//field mdbPrev 64
//}

register_bitfields! {
    u128,
    CapDerivationNode [
        // Next CTE node address -- per cteInsert this is address of the entire CTE slot
        Next OFFSET(16) NUMBITS(46) [], // 4-bytes-aligned, size of canonical phys address is 48 bits
        Revocable OFFSET(62) NUMBITS(1) [
            Disable = 0,
            Enable = 1
        ],
        FirstBadged OFFSET(63) NUMBITS(1) [
            Disable = 0,
            Enable = 1
        ],
        // Prev CTE node address -- per cteInsert this is address of the entire CTE slot
        Prev OFFSET(64) NUMBITS(64) []
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

capdefs! {
    Null, Untyped, Endpoint,
    Notification, Reply, CapNode,
    Thread, IrqControl, IrqHandler,
    Zombie, Domain, Resume,
    Frame, PageTable, PageDirectory,
    PageUpperDirectory, PageGlobalDirectory,
    AsidControl, AsidPool
}

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
        slice[slot].derivation_node = DerivationTreeNode::empty()
            .set_revocable(true)
            .set_first_badged(true);
    }
}

impl NullCapability {
    /// Create a Null capability.
    ///
    /// Such capabilities are invalid and can not be used for anything.
    pub fn new() -> NullCapability {
        NullCapability(LocalRegisterCopy::new(u128::from(NullCap::Type::value)))
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
#[derive(Clone)]
pub struct DerivationTreeNode(LocalRegisterCopy<u128, CapDerivationNode::Register>);

/// Errors that may happen in capability derivation tree operations.
#[derive(Debug, Snafu)]
pub enum DerivationTreeError {
    /// Previous link is invalid.
    InvalidPrev,
}

impl DerivationTreeNode {
    fn empty() -> Self {
        Self(LocalRegisterCopy::new(0))
    }

    /// SAFETY: it is UB to get prev reference from a null Prev pointer.
    pub unsafe fn get_prev(&self) -> CapTableEntry {
        let ptr = self.0.read(CapDerivationNode::Prev) as *const CapTableEntry;
        (*ptr).clone()
    }

    /// Get previous link in derivation tree - this is a derived-from capability.
    pub fn try_get_prev(&self) -> Result<CapTableEntry, DerivationTreeError> {
        if self.0.read(CapDerivationNode::Prev) == 0 {
            Err(DerivationTreeError::InvalidPrev)
        } else {
            Ok(unsafe { self.get_prev() })
        }
    }

    fn set_first_badged(mut self, enable: bool) -> Self {
        self.0.modify(if enable {
            CapDerivationNode::FirstBadged::Enable
        } else {
            CapDerivationNode::FirstBadged::Disable
        });
        self
    }

    fn set_revocable(mut self, enable: bool) -> Self {
        self.0.modify(if enable {
            CapDerivationNode::Revocable::Enable
        } else {
            CapDerivationNode::Revocable::Disable
        });
        self
    }
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
    derivation_node: DerivationTreeNode,
}

impl fmt::Debug for CapTableEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:x}", self.capability) // @todo
    }
}

impl CapTableEntry {
    /// Temporary for testing:
    fn empty() -> CapTableEntry {
        CapTableEntry {
            capability: 0,
            derivation_node: DerivationTreeNode::empty(),
        }
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

// @note src and dest are swapped here, compared to seL4 api
/*
struct CapNodePath {
    index: u32,
    depth: u32,
}

struct CapNodeRootedPath {
    root: CapNode,
    path: CapNodePath,
}

// @todo just use CapNodeCap
//struct CapNodeConfig {
//    guard: u32,
//    guard_size: u32,
//}

impl CapNode {
    fn mint(
        src: CapNodeRootedPath,
        dest: CapNodePath,
        rights: CapRights,
        badge: Badge,
    ) -> Result<(), CapError> {
        unimplemented!();
    }
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
    fn save_caller(r#where: CapNodePath) -> Result<(), CapError> {
        unimplemented!();
    }
    fn cancel_badged_sends(path: CapNodePath) -> Result<(), CapError> {
        unimplemented!();
    }
}*/

//struct CapSpace {} -- capspace is collection of capnodes in a single address space?
//impl CapNode for CapSpace {}

#[cfg(test)]
mod tests {
    // use super::*;

    #[test_case]
    fn first_capability_derivation_has_no_prev_link() {
        let entry = CapTableEntry::empty();
        assert_eq!(entry.derivation_node.try_get_prev(), Err(DerivationTreeError::InvalidPrev));
    }
}

// @todo Use bitmatch over cap Type field?
// Could be interesting if usable. See https://github.com/porglezomp/bitmatch
// Maybe look at https://lib.rs/crates/enumflags2 too

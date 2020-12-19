/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

//! DerivationTree nodes record the tree of inheritance for caps:
//! See the picture on derivation from seL4 manual for how this works: each cap contains a ref to
//! DerivationTree node, which records the previous cap and the following cap(s).

use {
    super::captable::CapTableEntry,
    crate::memory::PhysAddr,
    register::{register_bitfields, LocalRegisterCopy},
    snafu::Snafu,
};

//-- Mapping database (MDB) node: size = 16 bytes
//block mdb_node {
//padding 16 -- highest in word[1]
//field_high mdbNext 46  <-- field_high means "will need sign-extension", also value has 2 lower bits just dropped when setting
//field mdbRevocable 1 -- second bit in word[1]
//field mdbFirstBadged 1 -- lowest in word[1]
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

/// Wrapper for CapDerivationNode
#[derive(Clone, Debug, Copy)]
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

    pub(crate) fn empty() -> Self {
        Self(LocalRegisterCopy::new(0))
    }

    // Unlike mdb_node_new we do not pass revocable and firstBadged flags here, they are enabled
    // using builder interface set_first_badged() and set_revocable().
    pub(crate) fn new(prev_ptr: PhysAddr, next_ptr: PhysAddr) -> Self {
        Self::empty().set_prev(prev_ptr).set_next(next_ptr)
    }

    /// Get previous link in derivation tree.
    /// Previous link exists if this is a derived capability.
    ///
    /// SAFETY: it is UB to get prev reference from a null Prev pointer.
    pub(crate) unsafe fn get_prev(&self) -> CapTableEntry {
        let ptr =
            (self.0.read(CapDerivationNode::Prev) << Self::ADDR_BIT_SHIFT) as *const CapTableEntry;
        (*ptr).clone()
    }

    /// Try to get previous link in derivation tree.
    /// Previous link exists if this is a derived capability.
    pub(crate) fn try_get_prev(&self) -> Result<CapTableEntry, DerivationTreeError> {
        if self.0.read(CapDerivationNode::Prev) == 0 {
            Err(DerivationTreeError::InvalidPrev)
        } else {
            Ok(unsafe { self.get_prev() })
        }
    }

    pub(crate) fn set_prev(&mut self, prev_ptr: PhysAddr) -> Self {
        self.0
            .write(CapDerivationNode::Prev.val((prev_ptr >> Self::ADDR_BIT_SHIFT).into()));
        *self
    }

    /// Get next link in derivation tree.
    /// Next link exists if this capability has derived capabilities or siblings.
    ///
    /// SAFETY: it is UB to get next reference from a null Next pointer.
    pub(crate) unsafe fn get_next(&self) -> CapTableEntry {
        let ptr =
            (self.0.read(CapDerivationNode::Next) << Self::ADDR_BIT_SHIFT) as *const CapTableEntry;
        (*ptr).clone()
    }

    /// Try to get next link in derivation tree.
    /// Next link exists if this capability has derived capabilities or siblings.
    pub(crate) fn try_get_next(&self) -> Result<CapTableEntry, DerivationTreeError> {
        if self.0.read(CapDerivationNode::Next) == 0 {
            Err(DerivationTreeError::InvalidNext)
        } else {
            Ok(unsafe { self.get_next() })
        }
    }

    pub(crate) fn set_next(&mut self, next_ptr: PhysAddr) -> Self {
        self.0
            .write(CapDerivationNode::Next.val((next_ptr >> Self::ADDR_BIT_SHIFT).into()));
        *self
    }

    /// Builder interface to modify firstBadged flag
    /// @todo Describe the firstBadged flag and what it does.
    pub(crate) fn set_first_badged(mut self, enable: bool) -> Self {
        self.0.modify(if enable {
            CapDerivationNode::FirstBadged::Enable
        } else {
            CapDerivationNode::FirstBadged::Disable
        });
        self
    }

    /// Builder interface to modify revocable flag
    /// @todo Describe the revocable flag and what it does.
    pub(crate) fn set_revocable(mut self, enable: bool) -> Self {
        self.0.modify(if enable {
            CapDerivationNode::Revocable::Enable
        } else {
            CapDerivationNode::Revocable::Disable
        });
        self
    }
}

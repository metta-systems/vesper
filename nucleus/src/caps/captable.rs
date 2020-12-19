/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

use {super::derivation_tree::DerivationTreeNode, /*crate::memory::PhysAddr,*/ core::fmt};

// * Capability slots: 16 bytes of memory per slot (exactly one capability). --?
// CapNode describes `a given number of capability slots` with `a given guard`
// of `a given guard size` bits.

// @todo const generic on number of capabilities contained in the node? currently only contains a Cap
// capnode_cap has a pptr, guard_size, guard and radix
// this is enough to address a cap in the capnode contents
// by having a root capnode cap we can traverse the whole tree.

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
pub(crate) struct CapTableEntry {
    pub(crate) capability: u128,
    pub(crate) derivation: DerivationTreeNode,
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
    fn derived_from(&mut self, _parent: &mut CapTableEntry) {
        // self.derivation
        //     .set_prev(parent as *mut CapTableEntry as PhysAddr);
        // parent
        //     .derivation
        //     .set_next(self as *mut CapTableEntry as PhysAddr);
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

// sel4: cnode_capdata_t
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
    // [wip] is copy a derivation too? - yes it is - kernel_final.c:15769
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

#[cfg(test)]
mod tests {
    use super::{
        super::{derivation_tree::DerivationTreeError, null_cap::NullCapability},
        *,
    };

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

/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

use {
    super::{
        captable::CapTableEntry, derivation_tree::DerivationTreeNode, CapError, Capability, TryFrom,
    },
    crate::capdef,
    paste::paste,
    register::{register_bitfields, LocalRegisterCopy},
};

//=====================
// Cap definition
//=====================

register_bitfields! {
    u128,
    CapNodeCap [
        Guard OFFSET(0) NUMBITS(64) [],
        Type OFFSET(64) NUMBITS(5) [
            value = 10
        ],
        GuardSize OFFSET(69) NUMBITS(6) [],
        Radix OFFSET(75) NUMBITS(6) [],
        Ptr OFFSET(81) NUMBITS(47) [],
    ]
}

capdef! { CapNode }

//=====================
// Cap implementation
//=====================

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

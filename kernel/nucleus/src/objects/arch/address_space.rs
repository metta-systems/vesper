use {
    crate::objects::{NucleusObject, access::PoolTag},
    libobject::ObjectType,
};

/// Kernel state of one `AddressSpace` — the protection/mapping-context
/// boundary (Vesper's equivalent of seL4's `VSpace`).
///
/// Holds the translation root and the bound ASID: everything that makes a
/// hardware translation context. The executing `Thread` (a core object)
/// references its address space through a checked pool identity.
pub struct AArch64AddressSpace {
    /// Physical address of this address space's translation-root page table,
    /// if a root has been installed (mapping context, selected 2026-09-15).
    ///
    /// The root is a Retype-carved `PageTable` installed through
    /// `PageTable.Map` with this `AddressSpace`'s capability as the parent. The
    /// field records the table's physical address so the hardware walk can be
    /// reached through the direct map.
    pub translation_root: Option<u64>,
    /// The hardware ASID bound to this address space's translation root, if
    /// any (selected 2026-09-15: capability-protected `ASIDPool` resources
    /// with authorized `ASIDPool.Assign` binding).
    ///
    /// Unmap paths use the bound ASID to withdraw cached translations for
    /// exactly this context. `None` means no hardware context was ever
    /// established for the root, so no TLB invalidation is required.
    /// `AddressSpace.Retire` releases a
    /// bound ASID back to its originating pool.
    pub asid: Option<u16>,
}

impl AArch64AddressSpace {
    pub const fn new() -> Self {
        Self {
            translation_root: None,
            asid: None,
        }
    }
}

impl NucleusObject for AArch64AddressSpace {
    const TYPE: ObjectType = ObjectType::ADDRESS_SPACE;
    const POOL: PoolTag = PoolTag::AddressSpace;
}

impl crate::objects::arch_objects::AddressSpaceObject for AArch64AddressSpace {
    fn translation_root(&self) -> Option<u64> {
        self.translation_root
    }

    fn set_translation_root(&mut self, root: Option<u64>) {
        self.translation_root = root;
    }

    fn asid(&self) -> Option<u16> {
        self.asid
    }

    fn set_asid(&mut self, asid: Option<u16>) {
        self.asid = asid;
    }
}

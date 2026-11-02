use {
    crate::objects::{
        NucleusObject, carved::CarvedObject, key_table::KeyTableBinding, stack_pool::StackPools,
    },
    libobject::{CapError, ObjectType},
};

/// Kernel state of one `AddressSpace` — the protection/mapping-context
/// boundary (Vesper's equivalent of seL4's `VSpace`).
///
/// Holds the translation root and the bound ASID: everything that makes a
/// hardware translation context. The executing `Thread` (a core object)
/// references its address space through its carved identity.
#[expect(
    clippy::partial_pub_fields,
    reason = "the table binding is set once; translation root and ASID remain kernel-managed mapping state"
)]
pub struct AArch64AddressSpace {
    /// Table association, set once by `AddressSpace.Bind` (or by bootstrap
    /// for the boot `AddressSpace`) and fixed from then on. Every Thread
    /// executing in this `AddressSpace` uses this same table; `None` until
    /// bound.
    keytable: Option<KeyTableBinding>,
    /// Physical address of this address space's translation-root page table,
    /// if a root has been installed.
    ///
    /// The root is a Retype-carved `PageTable` installed through
    /// `PageTable.Map` with this `AddressSpace`'s capability as the parent. The
    /// field records the table's physical address so the hardware walk can be
    /// reached through the direct map.
    pub translation_root: Option<u64>,
    /// The hardware ASID bound to this address space's translation root, if
    /// any
    /// with authorized `ASIDPool.Assign` binding).
    ///
    /// Unmap paths use the bound ASID to withdraw cached translations for
    /// exactly this context. `None` means no hardware context was ever
    /// established for the root, so no TLB invalidation is required.
    /// `AddressSpace.Retire` releases a
    /// bound ASID back to its originating pool.
    pub asid: Option<u16>,
    /// The fault handler is running a delivered fault.
    fault_handler_busy: bool,
    /// Faults here that no handler took.
    unhandled_faults: u64,
    /// The stacks calls into this `AddressSpace` run on.
    stack_pools: StackPools,
}

impl AArch64AddressSpace {
    /// An inert `AddressSpace`: no keytable, translation root or ASID.
    pub const fn new() -> Self {
        Self {
            keytable: None,
            translation_root: None,
            asid: None,
            fault_handler_busy: false,
            unhandled_faults: 0,
            stack_pools: StackPools::new(),
        }
    }
}

impl CarvedObject for AArch64AddressSpace {}

impl NucleusObject for AArch64AddressSpace {
    const TYPE: ObjectType = ObjectType::ADDRESS_SPACE;
}

impl crate::objects::arch_objects::AddressSpaceObject for AArch64AddressSpace {
    fn keytable(&self) -> Option<KeyTableBinding> {
        self.keytable
    }

    fn bind(&mut self, keytable: KeyTableBinding) -> Result<(), CapError> {
        if self.keytable.is_some() {
            return Err(CapError::AlreadyMapped);
        }
        self.keytable = Some(keytable);
        Ok(())
    }

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

    fn fault_handler_busy(&self) -> bool {
        self.fault_handler_busy
    }

    fn set_fault_handler_busy(&mut self, busy: bool) {
        self.fault_handler_busy = busy;
    }

    fn unhandled_faults(&self) -> u64 {
        self.unhandled_faults
    }

    fn count_unhandled_fault(&mut self) {
        self.unhandled_faults = self.unhandled_faults.saturating_add(1);
    }

    fn stack_pools(&self) -> &StackPools {
        &self.stack_pools
    }

    fn stack_pools_mut(&mut self) -> &mut StackPools {
        &mut self.stack_pools
    }
}

//! A complete component `AddressSpace`: its own carved table, root chain and
//! ASID, built by the bootstrap builder.

use {
    crate::{
        builder::Builder,
        keys::{BOOT_TABLE_GUARD, BOOT_TABLE_SIZE_BITS, SlotCursor, table_key},
    },
    libobject::{KeySlot, KeyTableKey, ObjectType, RawKey, Rights},
    nucleus::{
        api::key_entry::KeyEntry,
        objects::{
            ArchObjects, ArchObjectsImpl, KeyTable, Nucleus,
            arch_objects::{AddressSpaceId, AddressSpaceObject},
        },
    },
};

/// A provisioned component `AddressSpace`.
pub struct Component {
    /// Carved identity of the component's `AddressSpace`.
    pub address_space: AddressSpaceId,
    /// Boot-table capability to the `AddressSpace` (full rights).
    pub address_space_key: RawKey,
    /// Boot-table capability to the component's table (full rights): the
    /// builder installs into it through ordinary invocations.
    pub table_key: RawKey,
    /// The component table's guard; every component-local key carries it.
    pub guard: u32,
    /// Kernel-window address of the component table, for bootstrap-origin
    /// grants only.
    pub table_addr: u64,
    /// The L2 table covering low VAs (image span and private regions).
    pub l2: RawKey,
    pub asid: u16,
}

impl Component {
    /// A key into this component's own table.
    pub fn local_key(&self, index: u32, incarnation: u32) -> RawKey {
        table_key(self.guard, index, incarnation)
    }

    /// The component table as an invocable destination for the builder.
    pub fn table(&self) -> KeyTableKey {
        KeyTableKey::from_key(self.table_key)
    }
}

impl Builder<'_> {
    /// Provision a component: carve its table with `guard`, seed the
    /// well-known self-table/self-`AddressSpace` slots, bind the table at
    /// `AddressSpace` provisioning (which installs the `Thread.Return`
    /// sentinel at Slot(1)), then give it a root chain and an ASID.
    ///
    /// `AddressSpace` creation has no public ABI yet: the builder carves it
    /// kernel-privately from the boot Untyped and grants itself the
    /// capability, as Kickstart does for the boot `AddressSpace`.
    pub fn component(&self, guard: u32, slots: &mut SlotCursor) -> Component {
        let table_key = self
            .untyped
            .retype(
                ObjectType::KEY_TABLE,
                BOOT_TABLE_SIZE_BITS,
                guard,
                1,
                self.self_table,
                slots.take(1),
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("component KeyTable Retype failed: {:?}", error.code()));
        let table_addr = self.table_address(table_key);
        // SAFETY: Retype initialized the full private carve; its accounted
        // backing is never relocated, reclaimed or reinitialized while the
        // binding is live. This exclusive borrow overlaps no invocation.
        let table = unsafe { &mut *(table_addr as *mut KeyTable) };
        // The self-table capability anchors the component's invocations:
        // the syscall entry sources the caller's own-table guard from it.
        table
            .insert(
                KeySlot::SELF_KEYTABLE,
                KeyEntry::new_keytable(table_addr, guard, BOOT_TABLE_SIZE_BITS, Rights::all(), 0),
                guard,
            )
            .unwrap_or_else(|failure| {
                panic!(
                    "component self-table grant failed: {:?}",
                    failure.error.code()
                )
            });
        // SAFETY: as above.
        let binding = unsafe { table.bind_address_space() }.unwrap_or_else(|error| {
            panic!("component table provisioning failed: {:?}", error.code())
        });
        let address_space = self.carve_object(ArchObjectsImpl::new_address_space(binding));
        let address_space_entry = || KeyEntry::new_carved(address_space, Rights::all(), 0);
        let address_space_key = {
            // SAFETY: the live carved boot KeyTable, exclusively borrowed for
            // this bootstrap grant.
            unsafe { &mut *(self.boot_table_addr as *mut KeyTable) }
                .insert(
                    KeySlot(slots.take(1)),
                    address_space_entry(),
                    BOOT_TABLE_GUARD,
                )
                .unwrap_or_else(|failure| {
                    panic!(
                        "component AddressSpace grant failed: {:?}",
                        failure.error.code()
                    )
                })
        };
        // SAFETY: the component table again; no other borrow is live.
        unsafe { &mut *(table_addr as *mut KeyTable) }
            .insert(KeySlot::SELF_ADDRESS_SPACE, address_space_entry(), guard)
            .unwrap_or_else(|failure| {
                panic!(
                    "component self-AddressSpace grant failed: {:?}",
                    failure.error.code()
                )
            });
        let l2 = self.root_chain(address_space_key, slots);
        let asid = Self::assign_asid(address_space_key);
        Component {
            address_space,
            address_space_key,
            table_key,
            guard,
            table_addr,
            l2,
            asid,
        }
    }
}

/// The TTBR0 value an `AddressSpace` activates with: root | ASID << 48.
pub fn ttbr(nucleus: &Nucleus<ArchObjectsImpl>, address_space: AddressSpaceId) -> u64 {
    let space = nucleus
        .address_space(address_space)
        .expect("AddressSpace missing");
    let root = space.translation_root().expect("AddressSpace has no root");
    let asid = space.asid().expect("AddressSpace has no ASID");
    root | (u64::from(asid) << 48)
}

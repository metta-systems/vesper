//! The boot Thread's builder `AddressSpace` and the start of the bundled
//! drivers: what the kickstart kernel and every `kernel/tests` kernel do
//! before their own work.
//!
//! [`BootObjects::bootstrap`] builds the initial nucleus and holds the boot
//! keys; [`BootObjects::builder_address_space`] gives the boot Thread an
//! `AddressSpace` with the retained image mapped, activates it and returns a
//! [`BootImage`]: the [`Builder`] and slot cursor a kernel carves its objects
//! with, and [`BootImage::start_drivers`], which loads the drivers and
//! programs the device tree asks for.

use {
    crate::{
        bootstrap::{BootState, bootstrap_nucleus, device_untypeds, retained_init_memory},
        drivers::{BootKeys, LoadedDriver, instance_count, load_drivers},
    },
    libcomponent_loader::{
        builder::{Builder, ImageArchive, ImageSummary, ImageTarget, verify_retained_image},
        component::ttbr,
        keys::{SlotCursor, boot_key},
        paging::image_table_count,
        retained::RetainedInitMemory,
    },
    libimage::DriverImage,
    libobject::{
        KeySlot, KeyTableKey, ObjectType, RawKey, Rights, UntypedKey,
        address_space::AddressSpaceKey,
    },
    nucleus::objects::{ArchObjectsImpl, Nucleus, arch_objects::AddressSpaceId},
};

/// First boot `KeyTable` slot the builder carves into.
pub const FIRST_BUILDER_SLOT: u32 = 64;

/// The usual guard of the `KeyTable` archiving the builder's image Frames.
pub const BUILDER_ARCHIVE_GUARD: u32 = 0x0A_4C01;

/// The initial nucleus and the boot Thread's keys, built once at EL1.
pub struct BootObjects {
    /// The init memory the boot image keeps (image and boot stack)
    pub retained: RetainedInitMemory,
    /// The boot state [`bootstrap_nucleus`] returns
    pub state: BootState,
    /// The boot keys drivers are given copies of
    pub keys: BootKeys,
    /// The boot Untyped
    pub untyped: UntypedKey,
    /// The boot Thread's own table
    pub self_table: KeyTableKey,
}

impl BootObjects {
    /// Take the retained init memory and build the initial nucleus.
    pub fn bootstrap() -> Self {
        let retained = retained_init_memory();
        let state = bootstrap_nucleus();
        let keys = BootKeys::of(&state);
        Self {
            retained,
            untyped: UntypedKey::from_key(keys.untyped),
            self_table: KeyTableKey::from_key(keys.self_table),
            state,
            keys,
        }
    }

    /// Give the boot Thread its builder `AddressSpace`: the retained image
    /// mapped in it and checked, its image Frames archived in a `KeyTable`
    /// guarded by `archive_guard` (usually [`BUILDER_ARCHIVE_GUARD`]), ASID 1,
    /// and active from here on.
    pub fn builder_address_space(&mut self, archive_guard: u32) -> BootImage<'_> {
        let Self {
            retained,
            state,
            keys,
            untyped,
            self_table,
        } = self;
        let builder = Builder {
            untyped,
            self_table,
            boot_table_addr: state.keytable_addr,
            retained,
        };
        let devices = device_untypeds();
        let mut slots =
            SlotCursor::starting_at(FIRST_BUILDER_SLOT).avoiding(devices.first_slot, devices.count);

        let address_space = boot_key(KeySlot::SELF_ADDRESS_SPACE.0, 1);
        let l2 = builder.root_chain(address_space, &mut slots);
        assert_eq!(Builder::assign_asid(address_space), 1, "the builder's ASID");
        let summary = builder.map_retained_image(
            &[ImageTarget {
                address_space,
                l2,
                first_table_slot: slots
                    .take(u32::try_from(image_table_count(retained)).unwrap_or(u32::MAX)),
            }],
            &ImageArchive {
                slot: slots.take(1),
                guard: archive_guard,
                grant_scratch: slots.take(1),
                copy_scratch: slots.take(1),
            },
        );
        verify_retained_image(retained, &[ttbr(state.nucleus, state.boot_as_id)]);
        AddressSpaceKey::from_key(address_space)
            .activate()
            .unwrap_or_else(|error| panic!("builder Activate failed: {:?}", error.code()));

        BootImage {
            nucleus: &mut *state.nucleus,
            keytable_addr: state.keytable_addr,
            boot_as_id: state.boot_as_id,
            keys: *keys,
            untyped,
            self_table,
            builder,
            slots,
            address_space,
            l2,
            summary,
        }
    }
}

/// The boot Thread in its builder `AddressSpace`, ready to carve and load.
pub struct BootImage<'a> {
    pub nucleus: &'a mut Nucleus<ArchObjectsImpl>,
    /// Kernel-window address of the boot `KeyTable`
    pub keytable_addr: u64,
    /// The boot `AddressSpace`'s identity
    pub boot_as_id: AddressSpaceId,
    pub keys: BootKeys,
    pub untyped: &'a UntypedKey,
    pub self_table: &'a KeyTableKey,
    pub builder: Builder<'a>,
    /// The boot slots still free, past the device Untypeds
    pub slots: SlotCursor,
    /// The builder `AddressSpace`'s key (`SELF_ADDRESS_SPACE`)
    pub address_space: RawKey,
    /// The L2 table covering the retained image in it
    pub l2: RawKey,
    /// What mapping the retained image took
    pub summary: ImageSummary,
}

impl BootImage<'_> {
    /// A new Notification in the next free boot slot, all rights: a report
    /// a component signals, or one the boot Thread parks on.
    pub fn notification(&mut self) -> RawKey {
        self.untyped
            .retype(
                ObjectType::NOTIFICATION,
                0,
                0,
                1,
                self.self_table,
                self.slots.take(1),
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("Notification Retype failed: {:?}", error.code()))
    }

    /// Load and start `drivers` (usually the image's `components::DRIVERS`),
    /// one instance per enabled device tree node a driver serves and one per
    /// program, calling `before_start` on each but the root dispatcher before
    /// it starts. Every planned instance starts; returns how many did.
    pub fn start_drivers(
        &mut self,
        drivers: &[&'static DriverImage],
        mut before_start: impl FnMut(&LoadedDriver<'_>, &Builder<'_>),
    ) -> usize {
        let planned = instance_count(drivers);
        let builder = &self.builder;
        let started = load_drivers(
            self.nucleus,
            builder,
            self.keys,
            drivers,
            &mut self.slots,
            |loaded| before_start(loaded, builder),
        );
        assert_eq!(started, planned, "every planned driver instance starts");
        started
    }
}

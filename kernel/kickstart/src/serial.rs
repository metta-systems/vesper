//! Attaching a client to a serial driver's channel (see `libserial-channel`).
//!
//! [`SerialChannelParts`] creates the shared memory and `EventCount`s of one
//! channel from the boot Untyped. [`SerialChannelParts::attach_driver`]
//! installs the driver's side in its table at the slots the driver expects,
//! before its Threads start. A client that is an EL0 component gets its side
//! from [`SerialChannelParts::attach_client`]: the control page and data area
//! mapped read/write at the addresses both sides agree on, and the
//! `EventCount`s it advances and waits on, recorded in its init page.

use {
    crate::drivers::grant,
    libcomponent_loader::{
        builder::Builder,
        component::Component,
        keys::{SlotCursor, boot_key},
    },
    libdevice_driver::SerialKeys,
    libobject::{ObjectType, RawKey, Rights},
    libserial_channel::{
        CONTROL_FRAME_SLOT, CONTROL_VA, DATA_AREA_VA, DATA_FRAME_SLOT, DATA_PAGES,
        RECEIVE_COMPLETED_SLOT, TRANSMIT_COMPLETED_SLOT, TRANSMIT_SUBMITTED_SLOT,
    },
    nucleus::api::key_entry::KeyEntry,
};

/// The shared memory and `EventCount`s of one serial channel, as boot-table
/// keys Kickstart holds until it hands each side its part.
#[derive(Clone, Copy)]
pub struct SerialChannelParts {
    /// The control page's Frame
    pub control: RawKey,
    /// First boot slot of the data area's Frames, and their incarnation
    pub data_first: u32,
    pub data_incarnation: u32,
    /// The transmit channel's submit `EventCount`
    pub transmit_submitted: RawKey,
    /// The transmit and receive channels' completion `EventCount`s
    pub transmit_completed: RawKey,
    pub receive_completed: RawKey,
}

impl SerialChannelParts {
    /// Carve the control page, the data area and the three `EventCount`s of a
    /// channel from `builder`'s Untyped, in fresh boot slots.
    pub fn create(builder: &Builder<'_>, slots: &mut SlotCursor) -> Self {
        let control = builder.retype_frames(slots.take(1), 1);
        let data_first = slots.take(DATA_PAGES);
        let data_incarnation = builder.retype_frames(data_first, DATA_PAGES).incarnation();
        let mut event_count = |what: &str| {
            builder
                .untyped
                .retype(
                    ObjectType::EVENT_COUNT,
                    0,
                    0,
                    1,
                    builder.self_table,
                    slots.take(1),
                    Rights::all(),
                )
                .unwrap_or_else(|error| {
                    panic!("🥾 {what} EventCount Retype failed: {:?}", error.code())
                })
        };
        Self {
            control,
            data_first,
            data_incarnation,
            transmit_submitted: event_count("transmit submit"),
            transmit_completed: event_count("transmit completion"),
            receive_completed: event_count("receive completion"),
        }
    }

    /// Install the driver's side in `driver`'s table: copies of the Frames
    /// (the driver maps them itself) and its ends of the `EventCount`s, at the
    /// slots `libserial-channel` fixes. Before the driver's Threads start.
    pub fn attach_driver(&self, builder: &Builder<'_>, driver: &Component) {
        let driver_table = driver.table();
        let give_frame = |frame: RawKey, slot: u32| {
            let copy = builder
                .self_table
                .copy_derive(
                    frame,
                    &driver_table,
                    slot,
                    Rights(Rights::READ | Rights::WRITE),
                )
                .unwrap_or_else(|error| panic!("🥾 channel Frame copy failed: {:?}", error.code()));
            assert_eq!(copy, driver.local_key(slot, 1));
        };
        give_frame(self.control, CONTROL_FRAME_SLOT);
        for page in 0..DATA_PAGES {
            give_frame(
                boot_key(self.data_first + page, self.data_incarnation),
                DATA_FRAME_SLOT + page,
            );
        }
        for (event_count, rights, slot) in [
            (
                self.transmit_submitted,
                Rights::RECV,
                TRANSMIT_SUBMITTED_SLOT,
            ),
            (
                self.transmit_completed,
                Rights::SEND,
                TRANSMIT_COMPLETED_SLOT,
            ),
            (self.receive_completed, Rights::SEND, RECEIVE_COMPLETED_SLOT),
        ] {
            let key = grant(
                driver,
                event_count_entry(builder, event_count, rights),
                slot,
            );
            assert_eq!(key, driver.local_key(slot, 1));
        }
    }

    /// Give a client the other side: map the control page and the data area
    /// read/write at the addresses both sides agree on, and install its ends of
    /// the `EventCount`s in its table from `*next_slot` on.
    ///
    /// The client's address space needs the table covering those addresses
    /// (the device window every driver and program has). The Frames mapped
    /// are Kickstart's own: only the driver gets copies.
    pub fn attach_client(
        &self,
        builder: &Builder<'_>,
        client: &Component,
        next_slot: &mut u32,
    ) -> SerialKeys {
        let rights = Rights(Rights::READ | Rights::WRITE);
        let control_slot = self.control.slot().0;
        Builder::map_frames(
            control_slot,
            self.control.incarnation(),
            1,
            client.address_space_key,
            CONTROL_VA,
            rights,
        );
        Builder::map_frames(
            self.data_first,
            self.data_incarnation,
            DATA_PAGES,
            client.address_space_key,
            DATA_AREA_VA,
            rights,
        );
        let mut give = |event_count: RawKey, event_rights: u8| {
            let slot = *next_slot;
            *next_slot += 1;
            grant(
                client,
                event_count_entry(builder, event_count, event_rights),
                slot,
            )
            .to_wire()
        };
        SerialKeys {
            transmit_submitted: give(self.transmit_submitted, Rights::SEND),
            transmit_completed: give(self.transmit_completed, Rights::RECV),
            receive_completed: give(self.receive_completed, Rights::RECV),
        }
    }
}

/// A key entry for the boot-table `EventCount` `key` with `rights`.
fn event_count_entry(builder: &Builder<'_>, key: RawKey, rights: u8) -> KeyEntry {
    let id = builder
        .entry(key)
        .carved_id()
        .unwrap_or_else(|error| panic!("🥾 EventCount identity: {:?}", error.code()));
    KeyEntry::new_carved(id, Rights(rights), 0)
}

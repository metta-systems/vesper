//! Pipes to the host controller: the keys `usb_hcd_v1.open_pipe` installs in
//! the caller's table, and what a caller does with them.

use {
    libmeddle_runtime::{DataArea, ExclusiveStacks},
    libobject::{KeyTableKey, RawKey},
    usb_hcd_client::{
        pipe_v1::{self, TransferKind},
        usb_hcd_v1::{Client, Error, Speed},
    },
    vesper_usb_protocol::setup::SetupPacket,
};

/// Keys one pipe has: one per function of `pipe_v1`.
pub const PIPE_KEYS: usize = pipe_v1::FUNCTION_COUNT;

/// Groups of [`PIPE_KEYS`] slots set aside for pipe keys: the controller's pipes
/// and a few more for copies and refused attempts.
pub const GROUPS: usize = 20;

/// Why a pipe could not be opened and configured.
#[derive(Debug)]
pub enum OpenError {
    Open(Error),
    Configure(pipe_v1::Error),
}

/// An open pipe: its keys, and where they are in the caller's table.
pub struct Pipe {
    pub client: pipe_v1::Client,
    pub group: usize,
    pub keys: [u64; PIPE_KEYS],
}

/// The host controller, the data area it shares with the caller, and the table
/// the pipes' keys are installed in.
pub struct Pipes<'a> {
    client: &'a Client,
    area: DataArea,
    table: KeyTableKey,
    /// First slot of the first group.
    first_slot: u32,
    /// The target stack of calls into the controller, which pipe keys share.
    stack_end: u64,
    used: [bool; GROUPS],
}

impl<'a> Pipes<'a> {
    /// Pipes of `client`, whose data area is `area`: the keys go into `table`
    /// from `first_slot` for `GROUPS * PIPE_KEYS` slots, and calls run on the
    /// target stack ending at `stack_end` (the end of the stack of `open_pipe`'s
    /// import).
    #[must_use]
    pub const fn new(
        client: &'a Client,
        area: DataArea,
        table: KeyTableKey,
        first_slot: u32,
        stack_end: u64,
    ) -> Self {
        Self {
            client,
            area,
            table,
            first_slot,
            stack_end,
            used: [false; GROUPS],
        }
    }

    /// The host controller component.
    #[must_use]
    pub const fn client(&self) -> &'a Client {
        self.client
    }

    /// The data area shared with the host controller.
    #[must_use]
    pub const fn area(&self) -> DataArea {
        self.area
    }

    /// The first slot of group `group`.
    #[must_use]
    pub fn slot_of(&self, group: usize) -> u32 {
        self.first_slot + u32::try_from(group * PIPE_KEYS).expect("a slot fits in u32")
    }

    /// The table the keys are installed in.
    #[must_use]
    pub const fn table(&self) -> &KeyTableKey {
        &self.table
    }

    /// A group of vacant slots, marked used.
    ///
    /// # Panics
    ///
    /// When every group is in use.
    pub fn take_group(&mut self) -> usize {
        let group = self
            .used
            .iter()
            .position(|used| !used)
            .expect("a free group of slots");
        self.used[group] = true;
        group
    }

    /// A pipe client for the keys at `keys`.
    ///
    /// # Panics
    ///
    /// When `keys` is not one key per function.
    #[must_use]
    pub fn pipe_client(&self, keys: &[u64; PIPE_KEYS]) -> pipe_v1::Client {
        let bindings = pipe_v1::Bindings::from_installed(keys, self.stack_end, Some(self.area))
            .expect("one key per function");
        pipe_v1::Client::new(
            bindings,
            // SAFETY: the caller's one Thread that talks to the controller is
            // the only one calling through the controller's stacks.
            unsafe { ExclusiveStacks::new() },
        )
    }

    /// Open a pipe to `endpoint` (an endpoint address), installing its keys in
    /// a group of vacant slots, without configuring it.
    ///
    /// # Errors
    ///
    /// The controller's refusal.
    ///
    /// # Panics
    ///
    /// When the controller reports success but not the keys.
    pub fn open_unconfigured(
        &mut self,
        address: u32,
        endpoint: u32,
        speed: Speed,
    ) -> Result<Pipe, Error> {
        let group = self.take_group();
        let slot = self.slot_of(group);
        let opened = self
            .client
            .open_pipe(&self.table, slot, address, endpoint, speed);
        if let Err(error) = opened {
            self.used[group] = false;
            return Err(error);
        }
        let mut keys = [0; PIPE_KEYS];
        assert!(
            self.area.read_u64s(0, &mut keys),
            "the keys are in the area"
        );
        Ok(Pipe {
            client: self.pipe_client(&keys),
            group,
            keys,
        })
    }

    /// Open a pipe and say what it transfers. A pipe that cannot be
    /// configured is closed again.
    ///
    /// # Errors
    ///
    /// The controller's refusal of the open or of the configuration.
    pub fn open(
        &mut self,
        address: u32,
        endpoint: u32,
        speed: Speed,
        kind: TransferKind,
        max_packet: u16,
    ) -> Result<Pipe, OpenError> {
        let pipe = self
            .open_unconfigured(address, endpoint, speed)
            .map_err(OpenError::Open)?;
        if let Err(error) = pipe.client.configure(kind, max_packet, 0) {
            self.close(&pipe);
            return Err(OpenError::Configure(error));
        }
        Ok(pipe)
    }

    /// Close `pipe` through its key and give its slots back.
    ///
    /// # Panics
    ///
    /// When the controller refuses to close an open pipe, or a key cannot be
    /// deleted.
    pub fn close(&mut self, pipe: &Pipe) {
        pipe.client
            .close_pipe()
            .unwrap_or_else(|error| panic!("close_pipe failed: {error:?}"));
        self.release(pipe);
    }

    /// Delete the keys of `pipe` from the table and free its group.
    ///
    /// # Panics
    ///
    /// When a key cannot be deleted.
    pub fn release(&mut self, pipe: &Pipe) {
        for key in pipe.keys {
            self.table
                .delete(RawKey::from_wire(key))
                .unwrap_or_else(|error| panic!("deleting a pipe key failed: {:?}", error.code()));
        }
        self.used[pipe.group] = false;
    }

    /// Run `setup` on `pipe`, sending `out` as its data stage. A reply goes to
    /// `reply`; returns the bytes of it.
    ///
    /// # Errors
    ///
    /// The controller's refusal or the transfer's failure.
    ///
    /// # Panics
    ///
    /// When `out` or the data stage does not fit the data area, or `reply` is
    /// shorter than the data stage.
    pub fn control(
        &self,
        pipe: &Pipe,
        setup: SetupPacket,
        out: &[u8],
        reply: &mut [u8],
    ) -> Result<usize, pipe_v1::Error> {
        assert!(self.area.write_bytes(0, &setup.to_bytes()));
        assert!(self.area.write_bytes(8, out));
        let stage = self
            .area
            .reference(0, 8 + u64::from(setup.length))
            .expect("the stage lies in the area");
        let moved = usize::try_from(pipe.client.control(stage)?).expect("fits");
        let got = reply.get_mut(..moved).expect("the reply fits");
        assert!(self.area.read_bytes(8, got));
        Ok(moved)
    }
}

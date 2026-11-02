//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! The Invocation keys an exporter hands to its clients when a function of
//! its interface `installs` another interface (a Meddle `installs` clause), such as
//! `usb_hcd_v1.open_pipe` installing the `pipe_v1` functions.
//!
//! For each installed function the exporter keeps a *master*: an Invocation key
//! in its own table for the function's entry, whose calls run on stacks of the
//! exporter's stack pool. A master is created the first time it is needed, from
//! the exporter's own `AddressSpace` key (`GRANT`). Installing derives a copy of
//! each master with a badge and `CALL` authority only, into the table the
//! client lent for the call: the exporter needs `INSTALL` on that table and
//! nothing else, and the client can use, delegate or delete the copies as any
//! key it holds. The badge tells the exporter's callers apart.
//!
//! The values of the installed keys are not predictable (a slot's incarnation
//! changes when it is reused), so the caller returns them to its client, who
//! reads them from the data area.

use libobject::{AddressSpaceKey, CapError, KeySlot, KeyTableKey, RawKey, Rights, StackPoolHandle};

/// Why keys could not be installed.
pub enum InstallError {
    /// The destination slots do not fit the slot numbering.
    Unknown,
    /// Creating or deriving a key failed: for example the destination slot is
    /// occupied or lies outside the client's table.
    Key(CapError),
}

impl From<CapError> for InstallError {
    fn from(error: CapError) -> Self {
        Self::Key(error)
    }
}

/// The masters of the `FUNCTIONS` functions of one installed interface.
pub struct Masters<const FUNCTIONS: usize> {
    address_space: AddressSpaceKey,
    table: KeyTableKey,
    pool: StackPoolHandle,
    first_slot: u32,
    entries: [u64; FUNCTIONS],
    /// The wire key of each master, zero until it is created.
    keys: [u64; FUNCTIONS],
}

impl<const FUNCTIONS: usize> Masters<FUNCTIONS> {
    /// Slots of the exporter's table the masters take, from the `first_slot`
    /// given to [`new`](Self::new).
    #[allow(clippy::cast_possible_truncation, reason = "a handful of slots")]
    pub const SLOTS: u32 = FUNCTIONS as u32;

    /// Masters kept in `table` (the exporter's own, with `DERIVE` and
    /// `INSTALL`) from `first_slot` for `SLOTS` slots, created from
    /// `address_space` (the exporter's own, with `GRANT`) for the functions at
    /// `entries` (what the installed interface's export macro lists). Calls
    /// through them take their stacks from `pool`, a pool of `address_space`.
    pub const fn new(
        address_space: AddressSpaceKey,
        table: KeyTableKey,
        pool: StackPoolHandle,
        first_slot: u32,
        entries: [u64; FUNCTIONS],
    ) -> Self {
        Self {
            address_space,
            table,
            pool,
            first_slot,
            entries,
            keys: [0; FUNCTIONS],
        }
    }

    /// The slot of the exporter's table that holds the master of `function`.
    #[allow(clippy::cast_possible_truncation, reason = "bounded by SLOTS")]
    pub const fn master_slot(&self, function: usize) -> u32 {
        self.first_slot + function as u32
    }

    /// Install the installed interface's keys, badged with `badge`, into the
    /// table `into` the client lent, at `first_slot` and the slots after it.
    /// Returns their wire keys in the client's table, in declaration order.
    ///
    /// # Errors
    ///
    /// [`InstallError::Unknown`] for a slot range out of range,
    /// [`InstallError::Key`] for the first failing key operation. Keys
    /// derived before the failure stay in the client's table; masters created
    /// stay in the exporter's.
    pub fn install(
        &mut self,
        into: &KeyTableKey,
        first_slot: u32,
        badge: u64,
    ) -> Result<[u64; FUNCTIONS], InstallError> {
        let last = u32::try_from(FUNCTIONS).map_err(|_overflow| InstallError::Unknown)?;
        if first_slot.checked_add(last).is_none() {
            return Err(InstallError::Unknown);
        }
        let mut installed = [0; FUNCTIONS];
        for (function, wire) in installed.iter_mut().enumerate() {
            let master = self.master(function)?;
            let slot =
                first_slot + u32::try_from(function).map_err(|_overflow| InstallError::Unknown)?;
            let key =
                self.table
                    .copy_derive_badged(master, into, slot, Rights(Rights::CALL), badge)?;
            *wire = key.to_wire();
        }
        Ok(installed)
    }

    /// The master of `function`, created if this is the first time it is
    /// needed.
    fn master(&mut self, function: usize) -> Result<RawKey, InstallError> {
        let existing = self.keys[function];
        if existing != 0 {
            return Ok(RawKey::from_wire(existing));
        }
        let key = self.address_space.create_invocation(
            self.entries[function],
            &self.table,
            KeySlot(self.master_slot(function)),
            self.pool,
        )?;
        self.keys[function] = key.to_wire();
        Ok(key)
    }
}

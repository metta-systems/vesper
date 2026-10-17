//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! The pipes a client opened to device endpoints. A pipe is a handle the
//! client passes back on every call; the table records what it names and who
//! owns it. Nothing here touches hardware.
//!
//! A handle packs the table index and a generation, so a closed pipe's handle
//! fails even when its slot is reused. Zero is never a handle.
//!
//! The owner is an opaque identity the caller supplies. The component uses
//! the base of the client's data area, which it finds from the stack the call
//! runs on (the same identity the mailbox component relies on until badges
//! exist).

use vesper_usb_protocol::{descriptor::TransferKind, enumeration::Speed};

/// Pipes the controller keeps open at once.
pub const MAX_PIPES: usize = 16;

/// The hub port a low- or full-speed device sits behind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    pub hub_address: u8,
    pub hub_port: u8,
}

/// What a pipe names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pipe {
    pub device_address: u8,
    pub endpoint: u8,
    pub speed: Speed,
    pub kind: TransferKind,
    pub max_packet: u16,
    /// Who opened the pipe.
    pub owner: u64,
    /// The split-transaction route, once the owner set one.
    pub route: Option<Route>,
}

/// Why a pipe call failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeError {
    /// No pipe has this handle, or another client owns it.
    Invalid,
    /// Every slot is taken.
    Full,
}

#[derive(Clone, Copy)]
struct Slot {
    /// Counts the pipes this slot held; never zero once used.
    generation: u16,
    pipe: Option<Pipe>,
}

/// The open pipes.
pub struct PipeTable {
    slots: [Slot; MAX_PIPES],
}

impl PipeTable {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [Slot {
                generation: 0,
                pipe: None,
            }; MAX_PIPES],
        }
    }

    /// Open `pipe`, returning its handle.
    ///
    /// # Errors
    ///
    /// [`PipeError::Full`] if every slot is taken.
    pub fn open(&mut self, pipe: Pipe) -> Result<u32, PipeError> {
        let (index, slot) = self
            .slots
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.pipe.is_none())
            .ok_or(PipeError::Full)?;
        // Skip zero when the generation wraps, so no handle is ever zero.
        slot.generation = slot.generation.wrapping_add(1).max(1);
        slot.pipe = Some(pipe);
        Ok(u32::from(slot.generation) << 16 | u32::try_from(index).unwrap_or(0))
    }

    /// The pipe `handle` names, if `owner` opened it.
    ///
    /// # Errors
    ///
    /// [`PipeError::Invalid`] for a closed, stale or foreign handle.
    pub fn get(&self, handle: u32, owner: u64) -> Result<&Pipe, PipeError> {
        let (index, generation) = split(handle);
        self.slots
            .get(index)
            .filter(|slot| slot.generation == generation)
            .and_then(|slot| slot.pipe.as_ref())
            .filter(|pipe| pipe.owner == owner)
            .ok_or(PipeError::Invalid)
    }

    /// Like [`get`](Self::get), for a pipe to change.
    ///
    /// # Errors
    ///
    /// [`PipeError::Invalid`] for a closed, stale or foreign handle.
    pub fn get_mut(&mut self, handle: u32, owner: u64) -> Result<&mut Pipe, PipeError> {
        let (index, generation) = split(handle);
        self.slots
            .get_mut(index)
            .filter(|slot| slot.generation == generation)
            .and_then(|slot| slot.pipe.as_mut())
            .filter(|pipe| pipe.owner == owner)
            .ok_or(PipeError::Invalid)
    }

    /// Close the pipe `handle` names.
    ///
    /// # Errors
    ///
    /// [`PipeError::Invalid`] for a closed, stale or foreign handle.
    pub fn close(&mut self, handle: u32, owner: u64) -> Result<(), PipeError> {
        self.get(handle, owner)?;
        let (index, _) = split(handle);
        if let Some(slot) = self.slots.get_mut(index) {
            slot.pipe = None;
        }
        Ok(())
    }
}

impl Default for PipeTable {
    fn default() -> Self {
        Self::new()
    }
}

/// The slot index and generation a handle packs.
const fn split(handle: u32) -> (usize, u16) {
    ((handle & 0xffff) as usize, (handle >> 16) as u16)
}

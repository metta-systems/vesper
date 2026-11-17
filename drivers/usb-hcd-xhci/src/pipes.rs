//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! The pipes clients opened to device endpoints. A pipe is a handle; the
//! table records what it names. Nothing here touches hardware.
//!
//! A handle packs the table index and a generation, so a closed pipe's handle
//! fails even when its slot is reused. Zero is never a handle. The component
//! badges each pipe's Invocation keys with its handle.

use crate::ring::Producer;

/// Pipes the controller keeps open at once.
pub const MAX_PIPES: usize = 16;

/// What an interrupt IN pipe keeps once `configure` added its endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Interrupt {
    /// The endpoint's transfer ring.
    pub ring: Producer,
    /// Bytes of one report.
    pub max_packet: u16,
    /// The bus address of the Normal TRB the controller holds, while one is
    /// out: the endpoint is polled until it completes.
    pub in_flight: Option<u64>,
    /// The endpoint stalled: no TRB goes out until `clear_halt`.
    pub halted: bool,
}

impl Interrupt {
    /// Whether a TRB should go out as soon as the client has a buffer
    /// posted.
    #[must_use]
    pub const fn wants_buffer(&self) -> bool {
        self.in_flight.is_none() && !self.halted
    }
}

/// What a pipe names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pipe {
    pub device_address: u8,
    /// The endpoint's Device Context Index: 1 for endpoint 0, `2n + 1` for
    /// IN endpoint `n`.
    pub context_index: u8,
    /// The control packet size `configure` set; `None` before, and for an
    /// interrupt pipe.
    pub max_packet: Option<u16>,
    /// An interrupt pipe's endpoint, once configured.
    pub interrupt: Option<Interrupt>,
    /// The position, among the controller's clients, of the one that opened
    /// the pipe: the data area its control stages lie in, and the report
    /// channel its reports go to.
    pub owner: usize,
}

/// No pipe has the handle, or every slot is taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeError {
    Invalid,
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
        Ok(handle(slot.generation, index))
    }

    /// The pipe `handle` names.
    ///
    /// # Errors
    ///
    /// [`PipeError::Invalid`] for a closed or stale handle.
    pub fn get_mut(&mut self, handle: u32) -> Result<&mut Pipe, PipeError> {
        let (index, generation) = split(handle);
        self.slots
            .get_mut(index)
            .filter(|slot| slot.generation == generation)
            .and_then(|slot| slot.pipe.as_mut())
            .ok_or(PipeError::Invalid)
    }

    /// Close the pipe `handle` names.
    ///
    /// # Errors
    ///
    /// [`PipeError::Invalid`] for a closed or stale handle.
    pub fn close(&mut self, handle: u32) -> Result<(), PipeError> {
        let (index, generation) = split(handle);
        let slot = self
            .slots
            .get_mut(index)
            .filter(|slot| slot.generation == generation && slot.pipe.is_some())
            .ok_or(PipeError::Invalid)?;
        slot.pipe = None;
        Ok(())
    }

    /// The handles of the open pipes, with what they name.
    pub fn open_pipes(&self) -> impl Iterator<Item = (u32, Pipe)> + '_ {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            slot.pipe.map(|pipe| (handle(slot.generation, index), pipe))
        })
    }

    /// Close every pipe to the device at `device_address`.
    pub fn close_device(&mut self, device_address: u8) {
        for slot in &mut self.slots {
            if slot
                .pipe
                .is_some_and(|pipe| pipe.device_address == device_address)
            {
                slot.pipe = None;
            }
        }
    }
}

impl Default for PipeTable {
    fn default() -> Self {
        Self::new()
    }
}

fn handle(generation: u16, index: usize) -> u32 {
    u32::from(generation) << 16 | u32::try_from(index).unwrap_or(0)
}

/// The table index of the pipe `handle` names, below [`MAX_PIPES`] for a
/// handle the table gave out: where the pipe's own pages go.
#[must_use]
pub const fn index_of(handle: u32) -> usize {
    split(handle).0
}

/// The slot index and generation a handle packs.
const fn split(handle: u32) -> (usize, u16) {
    ((handle & 0xffff) as usize, (handle >> 16) as u16)
}

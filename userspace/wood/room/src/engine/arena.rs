//! Generational arenas for map objects and thinkers.
//!
//! C allocates every thinker (map objects, moving sectors, lights) with
//! `Z_Malloc` and links them by pointer. Here each thinker struct lives in a
//! slot of an [`Arena`] and is named by a [`Handle`]: the slot index plus the
//! generation the slot had when the object was put there.
//!
//! Freeing a slot bumps its generation, so handles to the freed object go
//! stale, but it keeps the object's data, as C's freed zone block keeps its
//! bytes until the zone hands it out again. Vanilla code reads removed
//! objects through stale pointers (a monster's target that exploded, a
//! player's attacker that was removed); a stale handle reads the same data.
//! Debug builds report each place that does so (see [`stale_accesses`]);
//! release builds do not check.
//!
//! Slots are not reused while a level runs: [`Arena::insert`] always takes
//! the next fresh slot, and only [`Arena::clear`] (at level setup) lets the
//! slots be handed out again. A stale handle therefore always reads the data
//! its object had when it was freed.
//!
//! Objects are read and written by indexing the arena with a handle
//! (`world.mobjs[handle]`), which reports stale handles at the indexing
//! site. The arenas are fields of the play `World`, so the borrow checker
//! keeps a reference into an arena from living across a call that takes the
//! world: the play code copies the fields it needs out before such a call
//! and reads them afresh after it, as C sees them through its pointer.
//!
//! Slots live in fixed-size chunks whose storage is never moved or freed
//! while the arena lives, so [`Arena::address`] of an object stays the same
//! while other objects are spawned and removed, as a C pointer does.

use std::{
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
    num::NonZeroU32,
    ops::{Index, IndexMut},
    panic::Location,
};

/// Slots per chunk; a chunk is allocated when the previous ones are full.
const CHUNK_LEN: usize = 256;

/// A type stored in an [`Arena`].
pub trait Pooled: Sized + 'static {
    /// Type name used in stale-access reports.
    const NAME: &'static str;
}

/// Names an object in the [`Arena`] of `T`: its slot and the generation the
/// slot had when the object was inserted.
pub struct Handle<T> {
    /// Slot index in the arena.
    index: u32,
    /// The slot's generation when the object was inserted.
    generation: NonZeroU32,
    marker: PhantomData<fn() -> T>,
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Handle<T> {}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

impl<T> Eq for Handle<T> {}

impl<T> Hash for Handle<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.index.hash(state);
        self.generation.hash(state);
    }
}

impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Handle({}v{})", self.index, self.generation)
    }
}

impl<T> Handle<T> {
    /// The slot index (for diagnostics and tests).
    pub fn index(self) -> u32 {
        self.index
    }
}

/// One arena slot: the generation of its current (or last) object and the
/// object's data.
struct Slot<T> {
    /// Generation of the object in the slot; bumped when it is freed.
    generation: NonZeroU32,
    /// The object, or the last one put in the slot.
    value: T,
}

/// Storage for objects of type `T`, named by [`Handle`]s.
pub struct Arena<T: 'static> {
    /// Slot chunks, each allocated with room for `CHUNK_LEN` slots and
    /// never filled past that, so a chunk's storage is never reallocated and
    /// slots keep their address for the arena's lifetime.
    chunks: Vec<Vec<Slot<T>>>,
    /// Slots handed out since the last [`Arena::clear`]; the next insertion
    /// takes slot `used`.
    used: u32,
}

impl<T: 'static> Arena<T> {
    /// An empty arena; chunks are allocated on first use.
    pub const fn new() -> Self {
        Self {
            chunks: Vec::new(),
            used: 0,
        }
    }

    /// The slot at `index`.
    ///
    /// # Panics
    ///
    /// If no object was ever put in slot `index`.
    #[inline]
    fn slot(&self, index: u32) -> &Slot<T> {
        let index = index as usize;
        &self.chunks[index / CHUNK_LEN][index % CHUNK_LEN]
    }

    /// The slot at `index`, for writing (see [`Arena::slot`]).
    #[inline]
    fn slot_mut(&mut self, index: u32) -> &mut Slot<T> {
        let index = index as usize;
        &mut self.chunks[index / CHUNK_LEN][index % CHUNK_LEN]
    }

    /// Put `value` in the next fresh slot and return its handle.
    pub fn insert(&mut self, value: T) -> Handle<T> {
        let index = self.used;
        let chunk_index = index as usize / CHUNK_LEN;
        let offset = index as usize % CHUNK_LEN;
        if chunk_index == self.chunks.len() {
            self.chunks.push(Vec::with_capacity(CHUNK_LEN));
        }
        self.used += 1;
        let chunk = &mut self.chunks[chunk_index];
        let generation = if let Some(slot) = chunk.get_mut(offset) {
            // A slot freed by `clear`: it keeps its bumped generation.
            slot.value = value;
            slot.generation
        } else {
            // Never past `CHUNK_LEN`, so the chunk does not reallocate.
            chunk.push(Slot {
                generation: NonZeroU32::MIN,
                value,
            });
            NonZeroU32::MIN
        };
        Handle {
            index,
            generation,
            marker: PhantomData,
        }
    }

    /// Free the object `handle` names: its handles go stale, its data stays
    /// in the slot.
    ///
    /// # Panics
    ///
    /// In debug builds, if the object was already freed.
    pub fn free(&mut self, handle: Handle<T>) {
        debug_assert!(self.is_live(handle), "{handle:?} freed twice");
        let slot = self.slot_mut(handle.index);
        slot.generation = next_generation(slot.generation);
    }

    /// Free every object and make all slots available again: the level is
    /// over. Every handle into the arena goes stale.
    pub fn clear(&mut self) {
        for index in 0..self.used {
            let slot = self.slot_mut(index);
            slot.generation = next_generation(slot.generation);
        }
        self.used = 0;
    }

    /// `true` while the object `handle` names has not been freed.
    pub fn is_live(&self, handle: Handle<T>) -> bool {
        handle.index < self.used && self.slot(handle.index).generation == handle.generation
    }

    /// Number of slots handed out since the last [`Arena::clear`].
    pub fn used(&self) -> u32 {
        self.used
    }

    /// The object's address, without a liveness check: the value C writes
    /// where it stores a pointer it never follows (save files, the
    /// intercepts overrun emulation). It stays the same while the arena
    /// lives; a slot reused after [`Arena::clear`] has its old address.
    pub fn address(&self, handle: Handle<T>) -> usize {
        std::ptr::from_ref(&self.slot(handle.index).value).addr()
    }
}

impl<T: Pooled> Arena<T> {
    /// The slot `handle` names, reporting a stale handle at the caller's
    /// location in debug builds (see [`stale_accesses`]).
    #[inline]
    #[track_caller]
    fn checked_slot(&self, handle: Handle<T>) -> &Slot<T> {
        let slot = self.slot(handle.index);
        #[cfg(debug_assertions)]
        if slot.generation != handle.generation {
            record_stale_access(Location::caller(), T::NAME);
        }
        slot
    }

    /// The slot `handle` names, for writing (see [`Arena::checked_slot`]).
    #[inline]
    #[track_caller]
    fn checked_slot_mut(&mut self, handle: Handle<T>) -> &mut Slot<T> {
        let slot = self.slot_mut(handle.index);
        #[cfg(debug_assertions)]
        if slot.generation != handle.generation {
            record_stale_access(Location::caller(), T::NAME);
        }
        slot
    }
}

/// `arena[handle]` reads the object `handle` names. A stale handle reads
/// the data its object had when it was freed, as C reads a freed zone
/// block; debug builds report the indexing site (see [`stale_accesses`]).
impl<T: Pooled> Index<Handle<T>> for Arena<T> {
    type Output = T;

    #[inline]
    #[track_caller]
    fn index(&self, handle: Handle<T>) -> &T {
        &self.checked_slot(handle).value
    }
}

/// `arena[handle]` writes the object `handle` names, with the same stale
/// handle semantics as [`Index`].
impl<T: Pooled> IndexMut<Handle<T>> for Arena<T> {
    #[inline]
    #[track_caller]
    fn index_mut(&mut self, handle: Handle<T>) -> &mut T {
        &mut self.checked_slot_mut(handle).value
    }
}

impl<T: 'static> Default for Arena<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// The generation after `generation`, skipping zero on wrap-around.
fn next_generation(generation: NonZeroU32) -> NonZeroU32 {
    generation.checked_add(1).unwrap_or(NonZeroU32::MIN)
}

/// A place that read an object through a stale handle, and how often.
#[derive(Clone, Copy, Debug)]
pub struct StaleAccess {
    /// The source location that resolved the stale handle.
    pub site: &'static Location<'static>,
    /// The type of the freed object.
    pub kind: &'static str,
    /// How many times it did so.
    pub count: u64,
}

/// Stale accesses recorded so far, one entry per site.
#[cfg(debug_assertions)]
static STALE_ACCESSES: std::sync::Mutex<Vec<StaleAccess>> = std::sync::Mutex::new(Vec::new());

/// Count a stale access at `site`, logging the first one there.
#[cfg(debug_assertions)]
#[cold]
#[inline(never)]
fn record_stale_access(site: &'static Location<'static>, kind: &'static str) {
    let mut accesses = STALE_ACCESSES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(access) = accesses.iter_mut().find(|access| access.site == site) {
        access.count += 1;
        return;
    }
    eprintln!("stale {kind} handle read at {site}");
    accesses.push(StaleAccess {
        site,
        kind,
        count: 1,
    });
}

/// Every place that has read a freed object through a stale handle, with
/// counts. Release builds do not check handles and always return nothing.
pub fn stale_accesses() -> Vec<StaleAccess> {
    #[cfg(debug_assertions)]
    {
        STALE_ACCESSES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    #[cfg(not(debug_assertions))]
    {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in object, so the stale-access reports of the tests are told
    /// apart from the engine's.
    #[derive(Clone, Copy, Debug, PartialEq)]
    struct Probe {
        value: i32,
    }

    impl Pooled for Probe {
        const NAME: &'static str = "Probe";
    }

    fn probe(value: i32) -> Probe {
        Probe { value }
    }

    #[test]
    fn freed_handle_is_stale_but_reads_the_old_data() {
        let mut probes = Arena::new();
        let handle = probes.insert(probe(7));
        assert!(probes.is_live(handle));
        probes.free(handle);
        assert!(!probes.is_live(handle));
        // The slot keeps the object's data, as C's freed zone block does.
        assert_eq!(probes[handle].value, 7);
        #[cfg(debug_assertions)]
        assert!(
            stale_accesses()
                .iter()
                .any(|access| access.kind == "Probe" && access.count >= 1)
        );
    }

    #[test]
    fn indexing_reads_and_writes_and_reports_stale_reads_at_the_site() {
        let mut probes = Arena::new();
        let handle = probes.insert(probe(1));
        probes[handle].value = 2;
        assert_eq!(probes[handle].value, 2);
        probes.free(handle);
        // A freed object still reads its last data through the index.
        let stale_line = line!() + 1;
        assert_eq!(probes[handle].value, 2);
        #[cfg(debug_assertions)]
        assert!(stale_accesses().iter().any(|access| {
            access.kind == "Probe"
                && access.site.file() == file!()
                && access.site.line() == stale_line
        }));
    }

    #[test]
    fn slots_are_not_reused_until_cleared() {
        let mut probes = Arena::new();
        let first = probes.insert(probe(1));
        probes.free(first);
        let second = probes.insert(probe(2));
        assert_ne!(first.index(), second.index());
        assert_eq!(probes[first].value, 1);
        assert_eq!(probes[second].value, 2);
    }

    #[test]
    fn clear_makes_every_handle_stale_and_reuses_slots() {
        let mut probes = Arena::new();
        probes.insert(probe(2));
        probes.clear();
        let before = probes.insert(probe(3));
        let address = probes.address(before);
        probes.clear();
        assert!(!probes.is_live(before));
        let after = probes.insert(probe(4));
        assert_eq!(after.index(), before.index());
        assert_ne!(after, before);
        assert!(probes.is_live(after));
        // Same slot, same address: the old handle now reads the new object.
        assert_eq!(probes.address(after), address);
        assert_eq!(probes[before].value, 4);
    }

    /// Addresses are what the save game and the intercepts overrun
    /// emulation store for an object; they must not change while other
    /// objects are spawned (C's zone blocks do not move).
    #[test]
    fn addresses_survive_growth() {
        let mut probes = Arena::new();
        let first = probes.insert(probe(10));
        let address = probes.address(first);
        let handles: Vec<_> = (0..3 * CHUNK_LEN as i32)
            .map(|value| probes.insert(probe(value)))
            .collect();
        assert_eq!(probes.address(first), address);
        assert_eq!(probes[first].value, 10);
        assert!(handles.iter().all(|&handle| probes.is_live(handle)));
        assert!(probes.chunks.iter().all(|chunk| chunk.len() <= CHUNK_LEN));
        let distinct: std::collections::HashSet<usize> = handles
            .iter()
            .map(|&handle| probes.address(handle))
            .collect();
        assert_eq!(distinct.len(), handles.len());
    }

    #[test]
    fn generation_wraps_past_zero() {
        assert_eq!(next_generation(NonZeroU32::MAX), NonZeroU32::MIN);
        assert_eq!(next_generation(NonZeroU32::MIN).get(), 2);
    }
}

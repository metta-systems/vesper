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
//! Slots live in fixed-size chunks that are never moved or freed, so a
//! pointer from [`Handle::ptr`] stays valid while other objects are spawned:
//! the play code keeps a reference to the object it is working on across
//! calls that spawn, move or remove other objects, as the C code does.

use std::{
    cell::{Cell, UnsafeCell},
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
    mem::MaybeUninit,
    num::NonZeroU32,
    panic::Location,
};

use crate::engine::global::Global;

/// Slots per chunk; a chunk is allocated when the previous ones are full.
const CHUNK_LEN: usize = 256;

/// A type stored in an [`Arena`] held by a [`Global`].
pub trait Pooled: Sized + 'static {
    /// Type name used in stale-access reports.
    const NAME: &'static str;

    /// The arena holding every object of this type.
    fn arena() -> &'static Global<Arena<Self>>;
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

impl<T: Pooled> Handle<T> {
    /// Pointer to the object, valid for the rest of the program.
    ///
    /// The pointer stays valid across insertions, frees and level resets;
    /// once the object is freed it still points at the object's data until
    /// the slot is reused by a later level. Dereferencing it is subject to
    /// the usual aliasing rules: the play code holds at most one reference
    /// per object at a time, or reads through the raw pointer.
    ///
    /// In debug builds a stale handle (its object freed) is recorded with
    /// the caller's location, see [`stale_accesses`].
    #[inline]
    #[track_caller]
    pub fn ptr(self) -> *mut T {
        let arena = T::arena().borrow();
        let slot = arena.slot(self.index);
        #[cfg(debug_assertions)]
        if slot.generation.get() != self.generation {
            record_stale_access(Location::caller(), T::NAME);
        }
        slot.value.get().cast()
    }

    /// `true` while the object has not been freed.
    pub fn is_live(self) -> bool {
        T::arena().borrow().is_live(self)
    }

    /// The object's address, without a liveness check: the value C writes
    /// where it stores a pointer it never follows (save files, the
    /// intercepts overrun emulation).
    pub fn address(self) -> usize {
        T::arena().borrow().slot(self.index).value.get().addr()
    }
}

/// One arena slot: the generation of its current (or last) object and the
/// object's data.
struct Slot<T> {
    /// Generation of the object in the slot; bumped when it is freed.
    generation: Cell<NonZeroU32>,
    /// The object, uninitialised until the slot is first used.
    value: UnsafeCell<MaybeUninit<T>>,
}

/// Storage for objects of type `T`, named by [`Handle`]s.
pub struct Arena<T: 'static> {
    /// Fixed-size slot chunks. They are leaked, so slots never move and
    /// pointers into them stay valid for the rest of the program.
    chunks: Vec<&'static [Slot<T>]>,
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
    /// If no chunk holds `index`.
    #[inline]
    fn slot(&self, index: u32) -> &Slot<T> {
        let index = index as usize;
        &self.chunks[index / CHUNK_LEN][index % CHUNK_LEN]
    }

    /// Put `value` in the next fresh slot and return its handle.
    pub fn insert(&mut self, value: T) -> Handle<T> {
        let index = self.used;
        if index as usize == self.chunks.len() * CHUNK_LEN {
            let chunk: Box<[Slot<T>]> = (0..CHUNK_LEN)
                .map(|_| Slot {
                    generation: Cell::new(NonZeroU32::MIN),
                    value: UnsafeCell::new(MaybeUninit::uninit()),
                })
                .collect();
            self.chunks.push(Box::leak(chunk));
        }
        self.used += 1;
        let slot = self.slot(index);
        // SAFETY: the arena is borrowed mutably, so no reference into the
        // slot's value is live; handles to its previous object only make
        // raw pointers, which the play code does not hold across a spawn.
        unsafe { (*slot.value.get()).write(value) };
        Handle {
            index,
            generation: slot.generation.get(),
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
        let slot = self.slot(handle.index);
        slot.generation.set(next_generation(slot.generation.get()));
    }

    /// Free every object and make all slots available again: the level is
    /// over. Every handle into the arena goes stale.
    pub fn clear(&mut self) {
        for index in 0..self.used {
            let slot = self.slot(index);
            slot.generation.set(next_generation(slot.generation.get()));
        }
        self.used = 0;
    }

    /// `true` while the object `handle` names has not been freed.
    pub fn is_live(&self, handle: Handle<T>) -> bool {
        handle.index < self.used && self.slot(handle.index).generation.get() == handle.generation
    }

    /// Number of slots handed out since the last [`Arena::clear`].
    pub fn used(&self) -> u32 {
        self.used
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

    /// A stand-in object with its own arena, so the tests share no state
    /// with the engine.
    #[derive(Clone, Copy, Debug, PartialEq)]
    struct Probe {
        value: i32,
    }

    static PROBES: Global<Arena<Probe>> = Global::new(Arena::new());

    impl Pooled for Probe {
        const NAME: &'static str = "Probe";

        fn arena() -> &'static Global<Arena<Self>> {
            &PROBES
        }
    }

    /// The tests share `PROBES`.
    static PROBES_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn insert(value: i32) -> Handle<Probe> {
        PROBES.borrow_mut().insert(Probe { value })
    }

    #[test]
    fn freed_handle_is_stale_but_reads_the_old_data() {
        let _guard = PROBES_LOCK.lock().unwrap();
        let handle = insert(7);
        assert!(handle.is_live());
        PROBES.borrow_mut().free(handle);
        assert!(!handle.is_live());
        // The slot keeps the object's data, as C's freed zone block does.
        assert_eq!(unsafe { (*handle.ptr()).value }, 7);
        #[cfg(debug_assertions)]
        assert!(
            stale_accesses()
                .iter()
                .any(|access| access.kind == "Probe" && access.count >= 1)
        );
    }

    #[test]
    fn slots_are_not_reused_until_cleared() {
        let _guard = PROBES_LOCK.lock().unwrap();
        let first = insert(1);
        PROBES.borrow_mut().free(first);
        let second = insert(2);
        assert_ne!(first.index(), second.index());
        assert_eq!(unsafe { (*first.ptr()).value }, 1);
        assert_eq!(unsafe { (*second.ptr()).value }, 2);
    }

    #[test]
    fn clear_makes_every_handle_stale_and_reuses_slots() {
        let _guard = PROBES_LOCK.lock().unwrap();
        PROBES.borrow_mut().clear();
        let before = insert(3);
        let pointer = before.ptr();
        PROBES.borrow_mut().clear();
        assert!(!before.is_live());
        let after = insert(4);
        assert_eq!(after.index(), before.index());
        assert_ne!(after, before);
        assert!(after.is_live());
        // Same slot, same address: the old handle now reads the new object.
        assert_eq!(after.ptr(), pointer);
        assert_eq!(after.address(), before.address());
    }

    #[test]
    fn pointers_survive_growth() {
        let _guard = PROBES_LOCK.lock().unwrap();
        let first = insert(10);
        let pointer = first.ptr();
        let handles: Vec<_> = (0..3 * CHUNK_LEN as i32).map(insert).collect();
        assert_eq!(first.ptr(), pointer);
        assert_eq!(unsafe { (*pointer).value }, 10);
        assert!(handles.iter().all(|handle| handle.is_live()));
    }

    #[test]
    fn generation_wraps_past_zero() {
        assert_eq!(next_generation(NonZeroU32::MAX), NonZeroU32::MIN);
        assert_eq!(next_generation(NonZeroU32::MIN).get(), 2);
    }
}

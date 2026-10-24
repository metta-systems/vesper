//! Engine-wide state cells replacing C's file-scope globals.
//!
//! The engine runs on one thread, as the C original does, so a subsystem's
//! state lives in a plain `static` of [`Global`] instead of a `static mut`.
//! Access goes through scoped [`Global::borrow`] / [`Global::borrow_mut`]
//! guards. Debug builds count the live guards and panic on a conflicting
//! borrow (a mutable borrow alongside any other, from any thread), which
//! catches re-entrant access and tests that share state without a lock.
//! Release builds keep no count, so the guards cost nothing.
//!
//! Code that needs a stable address inside the state (the config table
//! binds settings by pointer, status bar widgets point into `players`)
//! takes it from [`Global::as_ptr`].

use std::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
};

#[cfg(debug_assertions)]
use std::sync::atomic::{AtomicIsize, Ordering};

/// Debug-build borrow count: `n > 0` shared borrows, `-1` one mutable borrow.
#[cfg(debug_assertions)]
const WRITING: isize = -1;

/// A single-threaded global holding one subsystem's state.
pub struct Global<T> {
    value: UnsafeCell<T>,
    #[cfg(debug_assertions)]
    borrows: AtomicIsize,
}

// SAFETY: the engine accesses its state from one thread at a time. Debug
// builds enforce the borrow rules at run time, including across threads.
unsafe impl<T> Sync for Global<T> {}

impl<T> Global<T> {
    /// Wrap the initial state.
    pub const fn new(value: T) -> Self {
        Self {
            value: UnsafeCell::new(value),
            #[cfg(debug_assertions)]
            borrows: AtomicIsize::new(0),
        }
    }

    /// Borrow the state for reading.
    ///
    /// # Panics
    ///
    /// In debug builds, if a mutable borrow is live.
    #[track_caller]
    pub fn borrow(&self) -> GlobalRef<'_, T> {
        #[cfg(debug_assertions)]
        {
            let previous = self
                .borrows
                .try_update(Ordering::Acquire, Ordering::Relaxed, |count| {
                    (count >= 0).then_some(count + 1)
                });
            assert!(previous.is_ok(), "Global already mutably borrowed");
        }
        GlobalRef { global: self }
    }

    /// Borrow the state for writing.
    ///
    /// # Panics
    ///
    /// In debug builds, if any other borrow is live.
    #[track_caller]
    pub fn borrow_mut(&self) -> GlobalMut<'_, T> {
        #[cfg(debug_assertions)]
        {
            let previous =
                self.borrows
                    .compare_exchange(0, WRITING, Ordering::Acquire, Ordering::Relaxed);
            assert!(previous.is_ok(), "Global already borrowed");
        }
        GlobalMut { global: self }
    }

    /// Raw pointer to the state, for addresses that must stay stable.
    ///
    /// Accesses through it are not counted by the debug borrow check.
    pub const fn as_ptr(&self) -> *mut T {
        self.value.get()
    }
}

impl<T: Copy> Global<T> {
    /// Copy the value out.
    #[track_caller]
    pub fn get(&self) -> T {
        *self.borrow()
    }

    /// Replace the value.
    #[track_caller]
    pub fn set(&self, value: T) {
        *self.borrow_mut() = value;
    }
}

/// Shared borrow of a [`Global`], released when dropped.
pub struct GlobalRef<'a, T> {
    global: &'a Global<T>,
}

impl<T> Deref for GlobalRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: a shared guard is live, so no mutable guard exists.
        unsafe { &*self.global.value.get() }
    }
}

impl<T> Drop for GlobalRef<'_, T> {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        self.global.borrows.fetch_sub(1, Ordering::Release);
    }
}

/// Mutable borrow of a [`Global`], released when dropped.
pub struct GlobalMut<'a, T> {
    global: &'a Global<T>,
}

impl<T> Deref for GlobalMut<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: this is the only live guard.
        unsafe { &*self.global.value.get() }
    }
}

impl<T> DerefMut for GlobalMut<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: this is the only live guard.
        unsafe { &mut *self.global.value.get() }
    }
}

impl<T> Drop for GlobalMut<'_, T> {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        self.global.borrows.store(0, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy)]
    struct Counters {
        ticks: i32,
        frames: i32,
    }

    #[test]
    fn get_set_and_guards_see_the_same_state() {
        let counters = Global::new(Counters {
            ticks: 0,
            frames: 7,
        });
        counters.borrow_mut().ticks += 3;
        assert_eq!(counters.borrow().ticks, 3);
        assert_eq!(counters.get().frames, 7);
        counters.set(Counters {
            ticks: 1,
            frames: 2,
        });
        assert_eq!(counters.borrow().frames, 2);
    }

    #[test]
    fn shared_borrows_coexist_and_release() {
        let value = Global::new(5);
        {
            let first = value.borrow();
            let second = value.borrow();
            assert_eq!(*first + *second, 10);
        }
        *value.borrow_mut() = 6;
        assert_eq!(value.get(), 6);
    }

    #[test]
    fn as_ptr_is_stable_and_aliases_the_state() {
        let value = Global::new([1, 2, 3]);
        let second = unsafe { &raw mut (*value.as_ptr())[1] };
        unsafe { *second = 20 };
        assert_eq!(value.borrow()[1], 20);
        assert_eq!(value.as_ptr(), value.as_ptr());
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "Global already borrowed")]
    fn mutable_borrow_during_shared_borrow_panics() {
        let value = Global::new(0);
        let _reading = value.borrow();
        let _writing = value.borrow_mut();
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "Global already mutably borrowed")]
    fn shared_borrow_during_mutable_borrow_panics() {
        let value = Global::new(0);
        let _writing = value.borrow_mut();
        let _reading = value.borrow();
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "Global already borrowed")]
    fn reentrant_mutable_borrow_panics() {
        let value = Global::new(0);
        let _outer = value.borrow_mut();
        value.set(1);
    }
}

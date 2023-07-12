/*
 * SPDX-License-Identifier: MIT OR BlueOak-1.0.0
 * Copyright (c) 2019 Andre Richter <andre.o.richter@gmail.com>
 * Original code distributed under MIT, additional changes are under BlueOak-1.0.0
 */

use core::cell::UnsafeCell;

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// Synchronization interfaces.
pub mod interface {

    /// Any object implementing this trait guarantees exclusive access to the data wrapped within
    /// the Mutex for the duration of the provided closure.
    pub trait Mutex {
        /// The type of the data that is wrapped by this mutex.
        type Data;

        /// Locks the mutex and grants the closure temporary mutable access to the wrapped data.
        fn lock<R>(&self, f: impl FnOnce(&mut Self::Data) -> R) -> R;
    }
}

/// A pseudo-lock for teaching purposes.
///
/// In contrast to a real Mutex implementation, does not protect against concurrent access from
/// other cores to the contained data.
///
/// The lock can only be used as long as it is safe to do so, i.e. as long as the kernel is
/// executing single-threaded, aka only running on a single core with interrupts disabled.
pub struct NullLock<T>
where
    T: ?Sized,
{
    data: UnsafeCell<T>,
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

/// Since we are instantiating this struct as a static variable, which could
/// potentially be shared between different threads, we need to tell the compiler
/// that sharing of this struct is safe by marking it with the Sync trait.
///
/// At this point in time, we can do so without worrying, because the kernel
/// anyways runs on a single core and interrupts are disabled. In short, multiple
/// threads don't exist yet in our code.
///
/// Literature:
/// * <https://doc.rust-lang.org/beta/nomicon/send-and-sync.html>
/// * <https://doc.rust-lang.org/book/ch16-04-extensible-concurrency-sync-and-send.html>
unsafe impl<T> Send for NullLock<T> where T: ?Sized + Send {}
unsafe impl<T> Sync for NullLock<T> where T: ?Sized + Send {}

impl<T> NullLock<T> {
    /// Create an instance.
    pub const fn new(data: T) -> Self {
        Self {
            data: UnsafeCell::new(data),
        }
    }
}

//------------------------------------------------------------------------------
// OS Interface Code
//------------------------------------------------------------------------------

impl<T> interface::Mutex for NullLock<T> {
    type Data = T;

    fn lock<R>(&self, f: impl FnOnce(&mut Self::Data) -> R) -> R {
        // In a real lock, there would be code encapsulating this line that ensures that this
        // mutable reference will ever only be given out once at a time.
        let data = unsafe { &mut *self.data.get() };

        f(data)
    }
}

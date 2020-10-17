/*
 * SPDX-License-Identifier: MIT OR BlueOak-1.0.0
 * Copyright (c) 2019 Andre Richter <andre.o.richter@gmail.com>
 * Original code distributed under MIT, additional changes are under BlueOak-1.0.0
 */

use core::cell::UnsafeCell;

pub struct NullLock<T> {
    data: UnsafeCell<T>,
}

/// Since we are instantiating this struct as a static variable, which could
/// potentially be shared between different threads, we need to tell the compiler
/// that sharing of this struct is safe by marking it with the Sync trait.
///
/// At this point in time, we can do so without worrying, because the kernel
/// anyways runs on a single core and interrupts are disabled. In short, multiple
/// threads don't exist yet in our code.
///
/// Literature:
/// https://doc.rust-lang.org/beta/nomicon/send-and-sync.html
/// https://doc.rust-lang.org/book/ch16-04-extensible-concurrency-sync-and-send.html
unsafe impl<T> Sync for NullLock<T> {}

impl<T> NullLock<T> {
    pub const fn new(data: T) -> NullLock<T> {
        NullLock {
            data: UnsafeCell::new(data),
        }
    }
}

impl<T> NullLock<T> {
    pub fn lock<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        // In a real lock, there would be code around this line that ensures
        // that this mutable reference will ever only be given out to one thread
        // at a time.
        f(unsafe { &mut *self.data.get() })
    }
}

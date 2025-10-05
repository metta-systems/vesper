/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

// @todo Use alloc-fmt crate for logging in allocators
// @todo move to liballoc or sth, outside of the kernel

use {
    core::{
        alloc::{AllocError, Allocator, Layout},
        cell::Cell,
        ptr::NonNull,
    },
    liblog::println,
};

pub struct BumpAllocator {
    next: Cell<usize>,
    pool_end: usize,
    name: &'static str,
}

unsafe impl Allocator for BumpAllocator {
    /// Allocate a memory block from the pool.
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let start = crate::mm::aligned_addr_unchecked(self.next.get(), layout.align());
        let end = start + layout.size();

        println!(
            "[i] {}:\n    Allocating Start {start:#010x} End {end:#010x}",
            self.name,
        );

        if end > self.pool_end {
            return Err(AllocError);
        }
        self.next.set(end);

        println!(
            "[i] {}:\n    Allocated Addr {:#010x} Size {:#x}",
            self.name,
            start,
            layout.size()
        );

        Ok(NonNull::slice_from_raw_parts(
            unsafe { NonNull::new_unchecked(start as *mut u8) },
            layout.size(),
        ))
    }

    /// A bump allocator doesn't care about releasing memory.
    unsafe fn deallocate(&self, _ptr: NonNull<u8>, _layout: Layout) {}
}

impl BumpAllocator {
    /// Create a named bump allocator between start and end addresses.
    #[allow(dead_code)]
    pub const fn new(pool_start: usize, pool_end: usize, name: &'static str) -> Self {
        Self {
            next: Cell::new(pool_start),
            pool_end,
            name,
        }
    }
}

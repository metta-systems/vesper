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

// SAFETY: Allocator trait is unsafe, HERE BE POKEMONS
unsafe impl Allocator for BumpAllocator {
    /// Allocate a memory block from the pool.
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let name = self.name;
        let size = layout.size();
        let start = libaddress::align::aligned_addr_unchecked(
            self.next.get() as u64,
            layout.align() as u64,
        );
        let end = start + layout.size() as u64;

        println!("[i] {name}:\n    Allocating Start {start:#010x} End {end:#010x}",);

        if end > self.pool_end as u64 {
            return Err(AllocError);
        }
        self.next.set(end.try_into().unwrap());

        println!("[i] {name}:\n    Allocated Addr {start:#010x} Size {size:#x}",);

        Ok(NonNull::slice_from_raw_parts(
            // SAFETY: We just pray and hope for the best.
            unsafe { NonNull::new_unchecked(start as *mut u8) },
            size,
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

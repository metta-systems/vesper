/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

// @todo Use alloc-fmt crate for logging in allocators

use {
    crate::println,
    core::{
        alloc::{AllocError, AllocRef, Layout},
        cell::Cell,
        ptr::NonNull,
    },
};

pub struct BumpAllocator {
    next: Cell<usize>,
    pool_end: usize,
    name: &'static str,
}

unsafe impl AllocRef for BumpAllocator {
    /// Allocate a memory block from the pool.
    fn alloc(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let start = crate::mm::aligned_addr_unchecked(self.next.get(), layout.align());
        let end = start + layout.size();

        println!(
            "[i] {}:\n      Allocating Start {:#010x} End {:#010x}",
            self.name, start, end
        );

        if end > self.pool_end {
            return Err(AllocError);
        }
        self.next.set(end);

        println!(
            "[i] {}:\n      Allocated Addr {:#010x} Size {:#x}",
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
    unsafe fn dealloc(&self, _ptr: NonNull<u8>, _layout: Layout) {}
}

impl BumpAllocator {
    /// Create a named bump allocator between start and end addresses.
    pub const fn new(pool_start: usize, pool_end: usize, name: &'static str) -> Self {
        Self {
            next: Cell::new(pool_start),
            pool_end,
            name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Validate allocator allocates from the provided address range
    // Validate allocation fails when range is exhausted
    #[test_case]
    fn test_allocates_within_init_range() {
        let allocator = BumpAllocator::new(256, 512, "Test allocator 1");
        let result1 = allocator.alloc(unsafe { Layout::from_size_align_unchecked(128, 1) });
        assert!(result1.is_ok());
        let result2 = allocator.alloc(unsafe { Layout::from_size_align_unchecked(128, 32) });
        println!("{:?}", result2);
        assert!(result2.is_ok());
        let result3 = allocator.alloc(unsafe { Layout::from_size_align_unchecked(1, 1) });
        assert!(result3.is_err());
    }
    // Creating with end <= start sshould fail
    // @todo return Result<> from new?
    #[test_case]
    fn test_bad_allocator() {
        let bad_allocator = BumpAllocator::new(512, 256, "Test allocator 2");
        let result1 = bad_allocator.alloc(unsafe { Layout::from_size_align_unchecked(1, 1) });
        assert!(result1.is_err());
    }
}

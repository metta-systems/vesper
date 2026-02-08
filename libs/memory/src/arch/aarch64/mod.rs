/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Memory management functions for aarch64.

pub mod features; // @todo make only pub re-export?
pub mod mmu;
mod page_size;
mod phys_frame;
pub(crate) mod translation_table;
mod virt_page;

pub use phys_frame::PhysFrame;

/// @todo ??
pub trait FrameAllocator {
    /// Allocate a physical memory frame.
    fn allocate_frame(&mut self) -> Option<PhysFrame>; // @todo Result<>
    /// Deallocate a physical frame.
    fn deallocate_frame(&mut self, frame: PhysFrame);
}

// Identity-map things for now.
//
// aarch64 granules and page sizes howto:
// https://stackoverflow.com/questions/34269185/simultaneous-existence-of-different-sized-pages-on-aarch64

/// Default page size used by the kernel.
pub const PAGE_SIZE: usize = 65536;

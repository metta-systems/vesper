/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Memory management functions for aarch64.

mod addr;
pub mod mmu;

pub use addr::{PhysAddr, VirtAddr};

// aarch64 granules and page sizes howto:
// https://stackoverflow.com/questions/34269185/simultaneous-existence-of-different-sized-pages-on-aarch64

/// Default page size used by the kernel.
pub const PAGE_SIZE: usize = 4096;

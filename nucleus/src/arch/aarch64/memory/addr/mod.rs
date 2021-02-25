/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

mod asid;
mod phys_addr;
mod virt_addr;

pub use asid::*;
pub use phys_addr::*;
pub use virt_addr::*;

// @todo Check largest VA supported, calculate physical_memory_offset
// @todo Keep in mind amount of physical memory present, the following
// @todo will only work for 1Gb board:
pub const PHYSICAL_MEMORY_OFFSET: u64 = 0xffff_8000_0000_0000; // Last 1GiB of VA space

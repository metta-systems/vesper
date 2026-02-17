/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

#![no_std]
#![feature(allocator_api)]
#![feature(format_args_nl)]

mod bump_allocator;
pub use bump_allocator::BumpAllocator;

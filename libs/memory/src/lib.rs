// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2018-2022 Andre Richter <andre.o.richter@gmail.com>

//! Memory Management.

#![no_std]
#![allow(dead_code)] // while refactoring
#![allow(incomplete_features)]
#![feature(generic_const_exprs)] // incomplete_features
#![feature(const_trait_impl)]
#![feature(format_args_nl)]
#![allow(internal_features)]
#![feature(allocator_api)]
#![feature(core_intrinsics)]
#![feature(step_trait)]
#![feature(custom_test_frameworks)]

mod arch;
pub mod mmu;
pub mod platform;

/// Convert a size into human readable format.
pub const fn size_human_readable_ceil(size: usize) -> (usize, &'static str) {
    const KIB: usize = 1024;
    const MIB: usize = 1024 * 1024;
    const GIB: usize = 1024 * 1024 * 1024;

    if (size / GIB) > 0 {
        (size.div_ceil(GIB), "GiB")
    } else if (size / MIB) > 0 {
        (size.div_ceil(MIB), "MiB")
    } else if (size / KIB) > 0 {
        (size.div_ceil(KIB), "KiB")
    } else {
        (size, "Byte")
    }
}

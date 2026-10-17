// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2020-2022 Andre Richter <andre.o.richter@gmail.com>

#![no_std]

//! MMIO helpers shared by the drivers. Register blocks themselves are
//! `#[derive(derive_mmio::Mmio)]` structs: their handles wrap a raw pointer and
//! never form a reference to device memory.

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

pub mod mmio_descriptor;
pub use mmio_descriptor::*;

/// Check at compile time that each named register of a `#[repr(C)]` register
/// block sits at its datasheet offset, and that the block has its datasheet
/// size. derive-mmio rejects padding between fields; this catches a wrong
/// reserved-gap length.
///
/// ```text
/// assert_register_offsets!(RegisterBlock, 0x1000, {
///     data: 0x000,
///     flag: 0x018,
/// });
/// ```
#[macro_export]
macro_rules! assert_register_offsets {
    ($block:ty, $size:expr, { $($register:ident: $offset:expr),* $(,)? }) => {
        const _: () = {
            $(
                assert!(
                    ::core::mem::offset_of!($block, $register) == $offset,
                    concat!(stringify!($block), "::", stringify!($register), " is not at its offset"),
                );
            )*
            assert!(
                ::core::mem::size_of::<$block>() == $size,
                concat!(stringify!($block), " is not its size"),
            );
        };
    };
}

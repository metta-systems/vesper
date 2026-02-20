/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

#![no_std]
#![no_main]
#![feature(decl_macro)]
#![feature(allocator_api)]
#![feature(format_args_nl)]
// #![feature(custom_test_frameworks)]
// #![test_runner(libtest::test_runner)]
// #![reexport_test_harness_main = "test_main"]

#[cfg(any(board_rpi3, board_rpi4, test, feature = "test_build"))]
pub mod raspberrypi;

#[cfg(any(board_rpi3, board_rpi4, test, feature = "test_build"))]
pub use raspberrypi::*;

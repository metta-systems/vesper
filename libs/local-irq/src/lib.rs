#![no_std]
#![no_main]
// #![feature(custom_test_frameworks)]
// #![test_runner(libtest::test_runner)]
// #![reexport_test_harness_main = "test_main"]

pub mod arch;
pub use arch::*;

#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(libtest::test_runner)]
#![reexport_test_harness_main = "test_main"]

mod common;

// This test essentially validates that a relocated chainboot executable runs correctly
// #[test_case]
// fn relocated_binary_works() {
//     assert_eq!(2 + 2, 4);
// }

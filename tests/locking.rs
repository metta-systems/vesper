#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(libtest::test_runner)]
#![reexport_test_harness_main = "test_main"]

// mod common;

// /// InitStateLock must be transparent.
// #[test_case]
// fn init_state_lock_is_transparent() {
//     use core::mem::size_of;

//     assert_eq!(
//         size_of::<liblocking::InitStateLock<u64>>(),
//         size_of::<u64>()
//     );
// }

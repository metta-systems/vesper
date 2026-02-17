#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(libtest::test_runner)]
#![reexport_test_harness_main = "test_main"]

use libexception::exception::{current_privilege_level, PrivilegeLevel};

// mod common;

// /// libmachine unit tests must execute in kernel mode.
// #[test_case]
// fn test_runner_executes_in_kernel_mode() {
//     let (level, _) = current_privilege_level();

//     assert!(level == PrivilegeLevel::Kernel)
// }

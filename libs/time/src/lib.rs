#![no_std]
#![no_main]
#![feature(format_args_nl)]
#![feature(custom_test_frameworks)]
#![test_runner(libtest::test_runner)]
#![reexport_test_harness_main = "test_main"]

pub mod arch;
pub mod time;

pub fn _time() -> core::time::Duration {
    crate::time::time_manager().uptime()
}

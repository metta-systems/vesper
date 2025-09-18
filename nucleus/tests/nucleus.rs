#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(libtest::test_runner)]
#![reexport_test_harness_main = "test_main"]

use liblog::info;

mod common;

#[allow(dead_code)]
fn check_data_abort_trap() {
    // Cause an exception by accessing a virtual address for which no
    // address translations have been set up.
    //
    // This line of code accesses the address 3 GiB, but page tables are
    // only set up for the range [0..1) GiB.
    let big_addr: u64 = 3 * 1024 * 1024 * 1024;
    unsafe { core::ptr::read_volatile(big_addr as *mut u64) };

    info!("[i] Whoa! We recovered from an exception.");
}

#[test_case]
fn test_data_abort_trap() {
    check_data_abort_trap() //-- this needs setup and properly configured mmu to recover ("test mode" in libmemory)
}

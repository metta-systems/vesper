#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(libtest::test_runner)]
#![reexport_test_harness_main = "test_main"]

use liblog::info;

// mod common;

// fn check_data_abort_trap() {
//     // Cause an exception by accessing a virtual address for which no
//     // address translations have been set up.
//     //
//     // This line of code accesses the address 2 GiB, but page tables are
//     // only set up for the range [0..1) GiB.
//     let big_addr: u64 = 2 * 1024 * 1024 * 1024;
//     unsafe { core::ptr::read_volatile(big_addr as *mut u64) };

//     info!("[i] Whoa! We recovered from an exception.");
// }

// #[test_case]
// fn test_data_abort_trap() {
//     libexception::exception::handling_init();

//     let phys_kernel_tables_base_addr = match unsafe { libmemory::mmu::kernel_map_binary() } {
//         Err(string) => panic!("Error mapping kernel binary: {}", string),
//         Ok(addr) => addr,
//     };

//     if let Err(e) = unsafe { libmemory::mmu::enable_mmu_and_caching(phys_kernel_tables_base_addr) }
//     {
//         panic!("Enabling MMU failed: {}", e);
//     }

//     libmemory::mmu::post_enable_init();

//     check_data_abort_trap() //-- this needs setup and properly configured mmu to recover ("test mode" in libmemory)
// }

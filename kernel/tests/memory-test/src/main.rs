#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! memory-test: explicitly managed translation tables and mappings —
//! `PageTable` Retype and installation, ASID binding, `AddressSpace.Retire`
//! (whole-ASID invalidation, ASID release and reuse), Frame.Map/Unmap with the
//! alias policy, activation making the carved tables hardware-live with
//! observable TLB invalidation, and page-table pool accounting — all through
//! the real SVC path.
//!
//! Runs on the real Kickstart boot path; in-guest assertions and the QEMU
//! semihosting exit status are the result (`just test-memory`). The suite
//! needs the `debug_kernel` console grant (Bounce's provisioning derives it).

#[cfg(feature = "debug_kernel")]
mod suite;

// `libkicktest` supplies the panic handler, so it is linked in every build.
use {kickstart::kickstart_init_el2, libkicktest as _, libqemu::semihosting as semi};

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

pub fn run() -> ! {
    semi::println!("memory-test: enabled MMU and dropped to EL1");
    #[cfg(not(feature = "debug_kernel"))]
    panic!("memory-test needs the debug_kernel feature");
    #[cfg(feature = "debug_kernel")]
    {
        suite::run();
        cfg_if::cfg_if! {
            if #[cfg(feature = "qemu")] {
                libqemu::semihosting::exit_success()
            } else {
                libcpu::endless_sleep()
            }
        }
    }
}

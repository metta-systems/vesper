#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! sync-test: `Notification` and `EventCount` through the real SVC path, then
//! blocking end to end — the boot Thread parks on a Notification and on
//! `EventCount` Awaits, and the Bounce fixture Thread (its own `AddressSpace`,
//! root and ASID; see `libkicktest::bounce`) signals and advances, with every
//! context switch observing the selected translation context — and
//! `Thread.Retire` of the parked Bounce Thread.
//!
//! Runs on the real Kickstart boot path; in-guest assertions and the QEMU
//! semihosting exit status are the result (`just test-sync`). The suite
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
    semi::println!("sync-test: enabled MMU and dropped to EL1");
    #[cfg(not(feature = "debug_kernel"))]
    panic!("sync-test needs the debug_kernel feature");
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

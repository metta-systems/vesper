#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! ppc-test: same-Thread protected procedure calls. `AddressSpace.CreateInvocation`
//! construction and its rejection matrix, then `Invocation.Call` /
//! `Thread.Return` round trips from the boot Thread into the Bounce fixture's
//! `AddressSpace` (see `libkicktest::bounce`): register scrubbing and
//! restore, a compiled `ppc_export!` entry and its Return-fault handler,
//! rejected Returns inside a migrated call, and live values across deliberate
//! target clobbers — all through the real SVC path.
//!
//! Runs on the real Kickstart boot path; in-guest assertions and the QEMU
//! semihosting exit status are the result (`just test-ppc`). The suite
//! needs the `debug_kernel` console grant (the PPC target prints through it).

#[cfg(feature = "debug_kernel")]
mod ppc;
#[cfg(feature = "debug_kernel")]
mod suite;

// `libkicktest` supplies the panic handler, so it is linked in every build.
use {kickstart::kickstart_init_el2, libkicktest as _, libqemu::semihosting as semi};

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

pub fn run() -> ! {
    semi::println!("ppc-test: enabled MMU and dropped to EL1");
    #[cfg(not(feature = "debug_kernel"))]
    panic!("ppc-test needs the debug_kernel feature");
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

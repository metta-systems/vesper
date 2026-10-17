#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! fault-test's component without a fault handler: its one fault is
//! unhandled, so its Thread must never run again.

use {fault_protocol::breakpoint, libuser::semihosting as semi};

libuser::entry!(main);

fn main(_argument: u64) -> ! {
    semi::println!("bare: faulting with no handler");
    breakpoint();
    panic!("bare: resumed after an unhandled fault");
}

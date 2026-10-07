#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! Negative-test EL0 component. Its loader passes a Notification key in `x0`;
//! the component executes one FP/SIMD instruction (see `fp-trap-protocol`),
//! signals whether it trapped, then parks forever.

use {
    fp_trap_protocol::{NOT_TRAPPED_BIT, TRAPPED_BIT, probe, trapped},
    libobject::{NotificationKey, RawKey},
    libuser::semihosting as semi,
};

libuser::entry!(main);

fn main(notification: u64) -> ! {
    let notification = NotificationKey::from_key(RawKey::from_wire(notification));
    semi::println!("fp-probe: executing an FP/SIMD instruction at EL0");
    let result = probe();
    let bits = if trapped(result) {
        TRAPPED_BIT
    } else {
        NOT_TRAPPED_BIT
    };
    notification
        .signal(bits)
        .unwrap_or_else(|error| panic!("fp-probe: signal failed: {:?}", error.code()));
    match notification.wait(NotificationKey::WAIT_INFINITE) {
        Ok(bits) => panic!("fp-probe: unexpected wakeup with bits {bits:#x}"),
        Err(error) => panic!("fp-probe: park failed: {:?}", error.code()),
    }
}

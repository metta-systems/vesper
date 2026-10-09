#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! Smoke-test EL0 component. Its loader passes a Notification key in `x0`;
//! the component prints from EL0, signals that Notification, then parks on it
//! forever.

use {
    libobject::{NotificationKey, RawKey},
    libuser::semihosting as semi,
};

libuser::entry!(main);

/// The bits this component signals to its loader.
pub const HELLO_BITS: u64 = 0b1010;

fn main(notification: u64) -> ! {
    let notification = NotificationKey::from_key(RawKey::from_wire(notification));
    semi::println!("hello: running at EL0");
    notification
        .signal(HELLO_BITS)
        .unwrap_or_else(|error| panic!("hello: signal failed: {:?}", error.code()));
    match notification.wait(NotificationKey::WAIT_INFINITE) {
        Ok(bits) => panic!("hello: unexpected wakeup with bits {bits:#x}"),
        Err(error) => panic!("hello: park failed: {:?}", error.code()),
    }
}

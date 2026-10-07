#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! The rendezvous server: a Thread in its own `AddressSpace`. It only calls
//! the endpoint — `receive` once, then `reply_receive` per request — and does
//! the work here, with a salt only this `AddressSpace` can read.

use {
    endpoint_protocol::{ServerInit, init, work as server_work},
    libobject::{RawKey, invocation::InvocationKey},
    libuser::{self as component, semihosting as semi},
};

component::entry!(main);

fn call(key: u64, args: [u64; 6], stack_end: u64) -> (u64, u64) {
    // SAFETY: `stack_end` tops the endpoint stack published with this
    // Invocation, used only by this Thread.
    unsafe { InvocationKey::from_key(RawKey::from_wire(key)).call(args, stack_end) }
        .unwrap_or_else(|error| panic!("server: endpoint Call failed: {:?}", error.code()))
}

fn main(_argument: u64) -> ! {
    // SAFETY: the builder fills this component's init page with a
    // `ServerInit` before starting the server.
    let init: &ServerInit = unsafe { init() };
    semi::println!("server: receiving at EL0");
    let (mut ticket, mut request) = call(init.receive, [0; 6], init.stack_end);
    loop {
        // Back in the server's own AddressSpace: only it can read the salt.
        let reply = server_work(request, init.salt);
        (ticket, request) = call(
            init.reply_receive,
            [ticket, reply, 0, 0, 0, 0],
            init.stack_end,
        );
        assert_ne!(ticket, 0, "server: the endpoint rejected a reply");
    }
}

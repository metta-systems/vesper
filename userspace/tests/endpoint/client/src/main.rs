#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! A rendezvous client Thread. Several run in the client `AddressSpace`; each
//! gets its index in `x0`, sends its requests through the endpoint, records
//! `(ticket, reply)` in the report page, tells the builder it is done, and
//! parks.

use {
    core::sync::atomic::Ordering,
    endpoint_protocol::{
        CLIENT_REQUESTS, CLIENT_TLS, ClientInit, ClientReport, REPORT_VA, init, set_tls, tls,
    },
    libobject::{NotificationKey, RawKey, invocation::InvocationKey},
    libuser::{self as component, semihosting as semi},
};

component::entry!(main);

fn main(index: u64) -> ! {
    // SAFETY: the builder fills this component's init page with a
    // `ClientInit` before starting any client Thread.
    let init: &ClientInit = unsafe { init() };
    // SAFETY: the report page is mapped read/write at REPORT_VA in the client
    // AddressSpace; each client Thread writes only its own row.
    let report = unsafe { &*(REPORT_VA as *const ClientReport) };
    let client = usize::try_from(index).unwrap_or(usize::MAX);
    let send = InvocationKey::from_key(RawKey::from_wire(init.send[client]));
    let own_tls = CLIENT_TLS + index;
    set_tls(own_tls);
    for (row, &request) in CLIENT_REQUESTS[client].iter().enumerate() {
        semi::println!("client {index}: sending {request:#x} at EL0");
        // SAFETY: the endpoint stack published with this client's Invocation
        // is used only by this Thread.
        let (ticket, reply) =
            unsafe { send.call([request, 0, 0, 0, 0, 0], init.stack_end[client]) }
                .unwrap_or_else(|error| panic!("client {index}: send failed: {:?}", error.code()));
        // The call migrated into the endpoint and blocked there: this Thread's
        // TLS must be its own again, whatever the endpoint and the server set.
        assert_eq!(tls(), own_tls, "client {index}: TLS not restored");
        report.replies[client][row][0].store(ticket, Ordering::Relaxed);
        report.replies[client][row][1].store(reply, Ordering::Release);
    }
    NotificationKey::from_key(RawKey::from_wire(init.done))
        .signal(1 << index)
        .unwrap_or_else(|error| panic!("client {index}: done signal failed: {:?}", error.code()));
    match NotificationKey::from_key(RawKey::from_wire(init.park))
        .wait(NotificationKey::WAIT_INFINITE)
    {
        Ok(bits) => panic!("client {index}: unexpected wakeup with bits {bits:#x}"),
        Err(error) => panic!("client {index}: park failed: {:?}", error.code()),
    }
}

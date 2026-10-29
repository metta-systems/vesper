// Host builds only check that the crate builds; the component itself runs on
// Vesper.
#![cfg_attr(target_os = "vesper", no_std, no_main)]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(all(target_os = "vesper", feature = "qemu"), feature(format_args_nl))]

//! The log server component (`doc/agentic/logging-design.md`): the image's
//! one holder of the PL011's serial channel (`serial = true`), writing the
//! lines its clients put on their `log_v1` io-channels (see
//! `interfaces/log_v1.if`).
//!
//! Its one Thread takes, in turn, at most one line from each client's channel
//! (so a client that floods gets no more of the UART than the others),
//! formats it with its timestamp and level ([`log_server::format_line`]),
//! gathers the lines in buffers of the serial channel's data area and hands
//! them to the PL011 driver. When the serial buffers are all in flight it
//! stops taking lines: the clients' buffers fill up and the clients drop
//! lines and count them, never waiting. With nothing to do it sleeps until a
//! client's `flush` or a few milliseconds pass.
//!
//! `flush` runs on the client's Thread: it only advances the server's own
//! `EventCount` (`DriverInit::event_count`), which the server's Thread sleeps
//! on.

#[cfg(target_os = "vesper")]
mod component;

#[cfg(not(target_os = "vesper"))]
fn main() {}

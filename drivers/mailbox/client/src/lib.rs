#![no_std]

//! Calls to the `mailbox_v1` interface, generated from
//! `drivers/mailbox/interfaces/mailbox_v1.if`: a [`mailbox_v1::Client`]
//! holds the Invocations its component was granted and makes typed calls
//! through them.

include!(concat!(env!("OUT_DIR"), "/mailbox_v1.rs"));

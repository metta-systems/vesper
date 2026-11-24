#![no_std]

//! Calls to the `display_v1` interface, generated from
//! `drivers/display/interfaces/display_v1.if`: a `display_v1::Client` holds
//! the Invocations its component was granted and makes typed calls through
//! them.

include!(concat!(env!("OUT_DIR"), "/display_v1.rs"));

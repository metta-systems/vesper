#![no_std]

//! Calls to the `power_v1` interface, generated from
//! `drivers/power/interfaces/power_v1.if`: a `power_v1::Client` holds the Invocations
//! its component was granted and makes typed calls through them.

include!(concat!(env!("OUT_DIR"), "/power_v1.rs"));

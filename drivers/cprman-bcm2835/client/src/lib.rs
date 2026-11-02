#![no_std]

//! Calls to the `clock_v1` interface, generated from
//! `drivers/cprman-bcm2835/interfaces/clock_v1.if`: a `clock_v1::Client` holds the
//! Invocations its component was granted and makes typed calls through them.

include!(concat!(env!("OUT_DIR"), "/clock_v1.rs"));

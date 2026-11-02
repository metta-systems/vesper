#![no_std]

//! Calls to the `block_v1` interface, generated from
//! `drivers/sdhost-bcm2835/interfaces/block_v1.if`: a `block_v1::Client` holds the
//! Invocations its component was granted and makes typed calls through them.

include!(concat!(env!("OUT_DIR"), "/block_v1.rs"));

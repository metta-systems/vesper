#![no_std]

//! Calls to the `fb_v1` interface, generated from
//! `drivers/fb/interfaces/fb_v1.if`: a `fb_v1::Client` holds the Invocations
//! its component was granted and makes typed calls through them.

include!(concat!(env!("OUT_DIR"), "/fb_v1.rs"));

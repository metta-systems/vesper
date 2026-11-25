#![no_std]

//! Calls to every-test's `echo_v1` interface, generated from
//! `userspace/tests/every/interfaces/echo_v1.if`: an `echo_v1::Client` holds
//! the Invocations its component was granted and makes typed calls through
//! them.

include!(concat!(env!("OUT_DIR"), "/echo_v1.rs"));

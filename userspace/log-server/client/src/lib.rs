#![no_std]

//! Calls to the log server component, generated from
//! `userspace/log-server/interfaces/log_v1.if`. Components log through
//! `liblog-client`, which uses this crate for the channel and `flush`.

include!(concat!(env!("OUT_DIR"), "/log_v1.rs"));

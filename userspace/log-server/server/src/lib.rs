#![no_std]

//! The `log_v1` interface, generated from
//! `userspace/log-server/interfaces/log_v1.if`: its types, the
//! [`log_v1::LogV1`] trait the log server implements, and [`log_v1_export!`],
//! which exports an implementation's functions as PPC entries.

include!(concat!(env!("OUT_DIR"), "/log_v1.rs"));

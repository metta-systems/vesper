#![no_std]

//! every-test's `echo_v1` interface, generated from
//! `userspace/tests/every/interfaces/echo_v1.if`: its types, the
//! [`echo_v1::EchoV1`] trait the echo programs implement, and
//! `echo_v1_export!`, which exports an implementation's functions as PPC
//! entries.

include!(concat!(env!("OUT_DIR"), "/echo_v1.rs"));

#![no_std]

//! Calls to the compositor, generated from
//! `userspace/gui/compositor/interfaces/compositor_v1.if`: a
//! [`compositor_v1::Client`] makes windows, says what changed in them and moves
//! them.

include!(concat!(env!("OUT_DIR"), "/compositor_v1.rs"));

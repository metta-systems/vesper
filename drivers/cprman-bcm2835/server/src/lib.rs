#![no_std]

//! The `clock_v1` interface, generated from `drivers/cprman-bcm2835/interfaces/clock_v1.if`:
//! its types, the [`clock_v1::ClockV1`] trait the cprman component implements, and
//! `clock_v1_export!`, which exports an implementation's functions as PPC entries.

include!(concat!(env!("OUT_DIR"), "/clock_v1.rs"));

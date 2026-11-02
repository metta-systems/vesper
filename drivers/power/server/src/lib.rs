#![no_std]

//! The `power_v1` interface, generated from `drivers/power/interfaces/power_v1.if`:
//! its types, the [`power_v1::PowerV1`] trait the power component implements, and
//! `power_v1_export!`, which exports an implementation's functions as PPC entries.

include!(concat!(env!("OUT_DIR"), "/power_v1.rs"));

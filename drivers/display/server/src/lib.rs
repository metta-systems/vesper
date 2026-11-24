#![no_std]

//! The `display_v1` interface, generated from `drivers/display/interfaces/display_v1.if`:
//! its types, the [`display_v1::DisplayV1`] trait every display output
//! implements, and `display_v1_export!`, which exports an implementation's
//! functions as PPC entries.

include!(concat!(env!("OUT_DIR"), "/display_v1.rs"));

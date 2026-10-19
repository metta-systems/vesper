#![no_std]

//! The `input_v1` interface, generated from `drivers/usb-hid/interfaces/input_v1.if`:
//! its types, the [`input_v1::InputV1`] trait the USB HID component implements,
//! and [`input_v1_export!`], which exports an implementation's functions as PPC
//! entries.

include!(concat!(env!("OUT_DIR"), "/input_v1.rs"));

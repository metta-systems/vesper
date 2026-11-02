#![no_std]

//! The `usb_v1` interface, generated from `drivers/usb/interfaces/usb_v1.if`:
//! its types, the [`usb_v1::UsbV1`] trait the USB core component implements,
//! and [`usb_v1_export!`], which exports an implementation's functions as PPC
//! entries.

include!(concat!(env!("OUT_DIR"), "/usb_v1.rs"));

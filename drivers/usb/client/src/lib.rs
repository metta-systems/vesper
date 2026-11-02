#![no_std]

//! Calls to the USB core, generated from `drivers/usb/interfaces/usb_v1.if`: a
//! [`usb_v1::Client`] claims interfaces of attached devices.

include!(concat!(env!("OUT_DIR"), "/usb_v1.rs"));

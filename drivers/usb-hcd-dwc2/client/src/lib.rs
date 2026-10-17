#![no_std]

//! Calls to the `usb_hcd_v1` interface, generated from
//! `drivers/usb-hcd-dwc2/interfaces/usb_hcd_v1.if`: a [`usb_hcd_v1::Client`]
//! holds the Invocations its component was granted and makes typed calls
//! through them.

include!(concat!(env!("OUT_DIR"), "/usb_hcd_v1.rs"));

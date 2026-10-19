#![no_std]

//! Calls to the USB host controller, generated from
//! `drivers/usb-hcd-dwc2/interfaces/`: a [`usb_hcd_v1::Client`] opens pipes,
//! and the keys it installs become the [`pipe_v1::Client`] of each pipe.

include!(concat!(env!("OUT_DIR"), "/usb_hcd_v1.rs"));
include!(concat!(env!("OUT_DIR"), "/pipe_v1.rs"));

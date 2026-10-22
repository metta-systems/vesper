#![no_std]

//! The interfaces of the USB host controller, generated from
//! `drivers/usb-hcd-dwc2/interfaces/`: `usb_hcd_v1`, with its
//! [`usb_hcd_v1::UsbHcdV1`] trait and [`usb_hcd_v1_export!`], and `pipe_v1`,
//! the functions of one open pipe, with [`pipe_v1::PipeV1`] and
//! [`pipe_v1_export!`]. A component implements both traits and exports both.

include!(concat!(env!("OUT_DIR"), "/usb_hcd_v1.rs"));
include!(concat!(env!("OUT_DIR"), "/pipe_v1.rs"));

#![no_std]

//! The `usb_hcd_v1` interface, generated from
//! `drivers/usb-hcd-dwc2/interfaces/usb_hcd_v1.if`: its types, the
//! [`usb_hcd_v1::UsbHcdV1`] trait the host controller component implements,
//! and [`usb_hcd_v1_export!`], which exports an implementation's functions as
//! PPC entries.

include!(concat!(env!("OUT_DIR"), "/usb_hcd_v1.rs"));

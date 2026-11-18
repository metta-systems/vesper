#![no_std]

//! The host side of the USB stack for components that call a host controller
//! component (`usb_hcd_v1`: DWC2 or xHCI): [`pipes::Pipes`] opens pipes, whose
//! keys the controller installs in the caller's table, and
//! [`enumerate::enumerate`] has the controller address a device and runs the
//! enumeration state machine of `vesper_usb_protocol` over them.

pub mod enumerate;
pub mod pipes;

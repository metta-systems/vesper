#![no_std]

//! The host side of the USB stack for components that call the DWC2 host
//! controller component (`usb_hcd_v1`): [`pipes::Pipes`] opens pipes, whose
//! keys the controller installs in the caller's table, and
//! [`enumerate::enumerate`] runs the enumeration state machine of
//! `vesper_usb_protocol` over them.

pub mod enumerate;
pub mod pipes;

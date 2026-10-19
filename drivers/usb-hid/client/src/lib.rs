#![no_std]

//! Calls to an input producer, generated from
//! `drivers/usb-hid/interfaces/input_v1.if`: an [`input_v1::Client`] reads the
//! descriptor and the state of each device the producer serves.

include!(concat!(env!("OUT_DIR"), "/input_v1.rs"));

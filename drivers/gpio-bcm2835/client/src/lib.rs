#![no_std]

//! Calls to the `gpio_v1` interface, generated from
//! `drivers/gpio-bcm2835/interfaces/gpio_v1.if`: a `gpio_v1::Client` holds the Invocations
//! its component was granted and makes typed calls through them.

include!(concat!(env!("OUT_DIR"), "/gpio_v1.rs"));

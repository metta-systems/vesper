#![no_std]

//! Calls to the `spi_v1` interface, generated from
//! `drivers/spi-bcm2835/interfaces/spi_v1.if`: a `spi_v1::Client` holds the
//! Invocations its component was granted and makes typed calls through them.

include!(concat!(env!("OUT_DIR"), "/spi_v1.rs"));

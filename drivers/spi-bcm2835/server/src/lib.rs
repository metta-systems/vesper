#![no_std]

//! The `spi_v1` interface, generated from `drivers/spi-bcm2835/interfaces/spi_v1.if`:
//! its types, the [`spi_v1::SpiV1`] trait the spi component implements, and
//! `spi_v1_export!`, which exports an implementation's functions as PPC entries.

include!(concat!(env!("OUT_DIR"), "/spi_v1.rs"));

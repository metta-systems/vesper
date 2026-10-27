#![no_std]

//! Calls to the `dma_v1` interface, generated from
//! `drivers/dma-bcm2835/interfaces/dma_v1.if`: a `dma_v1::Client` holds the
//! Invocations its component was granted and makes typed calls through them.

include!(concat!(env!("OUT_DIR"), "/dma_v1.rs"));

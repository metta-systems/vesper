#![no_std]

//! The `dma_v1` interface, generated from `drivers/dma-bcm2835/interfaces/dma_v1.if`:
//! its types, the [`dma_v1::DmaV1`] trait the dma component implements, and
//! `dma_v1_export!`, which exports an implementation's functions as PPC entries.

include!(concat!(env!("OUT_DIR"), "/dma_v1.rs"));

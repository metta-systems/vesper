#![no_std]

//! The `block_v1` interface, generated from `drivers/sdhost-bcm2835/interfaces/block_v1.if`:
//! its types, the [`block_v1::BlockV1`] trait a block device driver implements, and
//! `block_v1_export!`, which exports an implementation's functions as PPC entries.

include!(concat!(env!("OUT_DIR"), "/block_v1.rs"));

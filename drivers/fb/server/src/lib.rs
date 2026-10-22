#![no_std]

//! The `fb_v1` interface, generated from `drivers/fb/interfaces/fb_v1.if`:
//! its types, the [`fb_v1::FbV1`] trait the fb component implements, and
//! `fb_v1_export!`, which exports an implementation's functions as PPC entries.

include!(concat!(env!("OUT_DIR"), "/fb_v1.rs"));

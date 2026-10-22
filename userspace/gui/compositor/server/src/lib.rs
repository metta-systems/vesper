#![no_std]

//! The `compositor_v1` interface, generated from
//! `userspace/gui/compositor/interfaces/compositor_v1.if`: its types, the
//! [`compositor_v1::CompositorV1`] trait the compositor implements, and
//! [`compositor_v1_export!`], which exports an implementation's functions as
//! PPC entries.

include!(concat!(env!("OUT_DIR"), "/compositor_v1.rs"));

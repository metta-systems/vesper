#![no_std]

//! The `compositor_v1` and `display_layout_v1` interfaces, generated from
//! `userspace/gui/compositor/interfaces/`: their types, the
//! [`compositor_v1::CompositorV1`] and
//! [`display_layout_v1::DisplayLayoutV1`] traits the compositor implements,
//! and the macros that export an implementation's functions as PPC entries.

include!(concat!(env!("OUT_DIR"), "/compositor_v1.rs"));
include!(concat!(env!("OUT_DIR"), "/display_layout_v1.rs"));

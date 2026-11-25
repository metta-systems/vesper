#![no_std]

//! Calls to the compositor, generated from
//! `userspace/gui/compositor/interfaces/`: a [`compositor_v1::Client`] makes
//! windows, says what changed in them and moves them; a
//! [`display_layout_v1::Client`], built from the keys
//! `compositor_v1.display_layout` installs, reads and arranges the outputs.

include!(concat!(env!("OUT_DIR"), "/compositor_v1.rs"));
include!(concat!(env!("OUT_DIR"), "/display_layout_v1.rs"));

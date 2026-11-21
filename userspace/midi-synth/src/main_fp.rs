// Host builds only check that the crate builds; the component itself runs on
// Vesper.
#![cfg_attr(target_os = "vesper", no_std, no_main)]

//! The MIDI synthesiser component with the f64 engine (`fp-mode`, audio
//! decision D9.7): `src/main.rs`'s component, built hard-float as an FP
//! component. It starts on the f64 engine; an engine packet switches it.

#[cfg(target_os = "vesper")]
extern crate alloc;

#[cfg(target_os = "vesper")]
mod component;
#[cfg(target_os = "vesper")]
mod heap;

#[cfg(not(target_os = "vesper"))]
fn main() {}

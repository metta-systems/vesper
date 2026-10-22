//! Top-level crate module for the `room` engine.
//!
//! `room` is a Rust port of `chocolate-doom`/`doomgeneric`.  This file is the
//! library crate root that re-exports the major subsystems:
//!
//! - `audio` (crate-private) - rodio-based sound and music backend that
//!   replaces the SDL2 audio plumbing in `i_sound.c` / `i_oplmusic.c`.
//! - [`doom`] - per-`.c`-file Rust ports living under `vendor/doomgeneric/`.
//! - [`headless`] - thread-local frame/tick counters used by the testing and
//!   benchmarking harnesses to run the engine without a window or audio device.
//! - [`types`] - cross-cutting FFI-safe type aliases (currently just the
//!   tri-state [`types::Boolean`] mirroring Doom's `unsigned int boolean`).
//!
//! The binary front-ends (the GPU/winit player in `src/main.rs`, the
//! struct-size dumper in `src/bin/struct_sizes.rs`) and the test harness in
//! `doom::c_tests` all consume this crate.
//!
//! # Crate-wide lints
//!
//! Several lints are intentionally relaxed because the codebase is mid-port:
//! - `clippy::missing_safety_doc` - filled in progressively per-symbol; tracked
//!   by the documentation drive in `DOCUMENTING.md`.
//! - `dead_code`, `clashing_extern_declarations`, `private_interfaces` -
//!   symbols are defined for FFI completeness even when Rust does not yet
//!   call them, and a few `libc` prototypes intentionally differ.
//! - `unpredictable_function_pointer_comparisons` - Doom's thinker dispatch
//!   identifies thinkers by comparing their action function pointers; this is
//!   reliable within a single statically-linked binary.
//! - `clippy::not_unsafe_ptr_arg_deref` - many `extern "C"` ports accept raw
//!   pointers that the C callers (re)validate before invoking.
//! - `clippy::needless_range_loop`, `clippy::explicit_counter_loop` - direct
//!   index loops are kept where the originals were so the diff with C stays
//!   minimal; many arrays are `static mut` and would otherwise need
//!   `slice::from_raw_parts` wrappers.

// TODO: Add `# Safety` documentation to all unsafe functions.
#![allow(clippy::missing_safety_doc)]
// This crate is a C-to-Rust port. Many symbols are defined for FFI completeness
// but not yet called from Rust code; libc functions are redeclared with
// compatible-but-different types; and internal pointer types are intentionally
// less visible than their containing statics.
#![allow(dead_code, clashing_extern_declarations, private_interfaces)]
// Doom thinker dispatch compares function pointers for identity within a single
// binary, which is a well-understood and reliable pattern in this context.
#![allow(unpredictable_function_pointer_comparisons)]
// This is a C-to-Rust port: extern "C" functions routinely take raw pointer
// arguments that are dereferenced in the body. The C callers ensure validity.
#![allow(clippy::not_unsafe_ptr_arg_deref)]
// C-style indexed loops are idiomatic in this port; many arrays are static mut,
// so converting to iterators would require from_raw_parts complexity.
#![allow(clippy::needless_range_loop, clippy::explicit_counter_loop)]

pub(crate) mod audio;
pub mod doom;
pub mod headless;
pub mod types;

// Re-export `dhat` when the `dhat-heap` Cargo feature is enabled so binaries
// can install the heap profiler without listing `dhat` as a direct dependency.
#[cfg(feature = "dhat-heap")]
pub use dhat;

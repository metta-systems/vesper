//! Top-level crate module for the `room` engine.
//!
//! `room` is a Rust port of `chocolate-doom`/`doomgeneric`.  This file is the
//! library crate root that re-exports the major subsystems:
//!
//! - [`engine`] - Rust ports of the `doomgeneric` C sources, grouped into
//!   subsystem modules by their C prefix (see `doom.md`). The engine reaches
//!   the platform only through the
//!   [`Platform`](engine::platform::Platform) trait; the `room` binary
//!   implements it in `src/platform/`.
//! - [`types`] - cross-cutting FFI-safe type aliases (currently just the
//!   tri-state [`types::Boolean`] mirroring Doom's `unsigned int boolean`).
//!
//! The `room` binary (`src/main.rs`, with its platforms in `src/platform/`)
//! and the regression tests in `tests/` consume this crate.
//!
//! # Crate-wide lints
//!
//! Several lints are intentionally relaxed because the codebase is mid-port:
//! - `clippy::missing_safety_doc` - filled in progressively per-symbol; tracked
//!   by the documentation drive in `DOCUMENTING.md`.
//! - `dead_code`, `clashing_extern_declarations`, `private_interfaces` -
//!   ported symbols are kept even when nothing calls them yet, and a few
//!   `libc` prototypes are redeclared per module with differing types.
//! - `unpredictable_function_pointer_comparisons` - Doom's thinker dispatch
//!   identifies thinkers by comparing their action function pointers; this is
//!   reliable within a single statically-linked binary.
//! - `clippy::needless_range_loop`, `clippy::explicit_counter_loop` - direct
//!   index loops are kept where the originals were so the diff with C stays
//!   minimal.
//! - `unsafe_op_in_unsafe_fn` - the ported `unsafe fn`s keep C's style of one
//!   unsafe body; wrapping each operation in its own `unsafe {}` block is
//!   left for a separate pass.

// TODO: Add `# Safety` documentation to all unsafe functions.
#![allow(clippy::missing_safety_doc)]
// This crate is a C-to-Rust port. Many ported symbols are not yet called;
// libc functions are redeclared with
// compatible-but-different types; and internal pointer types are intentionally
// less visible than their containing statics.
#![allow(dead_code, clashing_extern_declarations, private_interfaces)]
// Doom thinker dispatch compares function pointers for identity within a single
// binary, which is a well-understood and reliable pattern in this context.
#![allow(unpredictable_function_pointer_comparisons)]
// C-style indexed loops are kept where the original had them.
#![allow(clippy::needless_range_loop, clippy::explicit_counter_loop)]
// Edition 2024 warns on unsafe operations in `unsafe fn` bodies without their
// own `unsafe {}` block; the ported functions still use C's whole-body style.
#![allow(unsafe_op_in_unsafe_fn)]

pub mod engine;
pub mod types;

// Re-export `dhat` when the `dhat-heap` Cargo feature is enabled so binaries
// can install the heap profiler without listing `dhat` as a direct dependency.
#[cfg(feature = "dhat-heap")]
pub use dhat;

//! `room` – a Rust port of doomgeneric.
//!
//! The engine lives in the `room` library and reaches the platform only
//! through the `DG_*` functions (see `platform/mod.rs`). This binary picks a
//! platform and runs it: on a host, a winit window with a wgpu renderer and
//! rodio audio (`platform/host`); on Vesper, placeholder implementations
//! (`platform/vesper`).
//!
//! ## Usage
//!
//! ```text
//! room -- -iwad /path/to/doom1.wad
//! ```
//!
//! Any arguments following `--` are forwarded to the Doom engine as-is.
//! A DOOM WAD file (`doom1.wad`, `doom.wad`, `doom2.wad`, etc.) must be
//! provided.

/// Heap-profiling global allocator, enabled only when the `dhat-heap`
/// Cargo feature is active.  The matching [`dhat::Profiler::new_heap`] handle
/// in [`main`] enables collection for the lifetime of the binary.
#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

mod platform;

/// Binary entry point: start the heap profiler if enabled, then run the
/// platform.
fn main() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    platform::run();
}

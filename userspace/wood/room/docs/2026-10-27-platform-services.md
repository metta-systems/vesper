# Platform services: files, memory, exit, logging

The engine (`room::engine`) reaches the operating system only through the
`Platform` trait (`src/engine/platform.rs`), so it builds as `no_std` +
`alloc` and runs both on the host (winit/wgpu/rodio) and on Vesper.

## Decisions (2026-10-27)

1. **Files.** WAD access is handle-based: `open`, `len`, `read_at(offset,
   buf)`, `close`. Everything else uses whole-file helpers: `read_file`,
   `write_file`, `rename`, `remove`, `exists`, `create_dir`. On Vesper,
   `open` serves the WAD from the `[[resource]]` memory blob, so no separate
   memio layer is needed for it.
2. **Memory.** The engine allocates only through `alloc::alloc` (and
   `Vec`/`Box`); the zone takes its block from the global allocator like
   everything else. The trait has no allocator method. Each binary installs
   its `#[global_allocator]`: the system allocator on the host, rlsf on top
   of a 16 MB carved block on Vesper, which must cover the zone and the
   other heap allocations together.
3. **Console output.** The engine logs through the `log` crate (`info!`,
   `warn!`, `error!`). The host installs `env_logger`; Vesper installs a
   logger that writes to its semihosting/serial output.
4. **Exit and errors.** `Platform::quit(code) -> !` and
   `Platform::error_box(message)`. The host keeps its Zenity dialog and
   `std::process::exit`.
5. **`no_std`.** The engine library becomes `no_std` + `alloc` in the same
   step (`#![cfg_attr(not(test), no_std)]`), as proof the extraction is
   complete.
6. **Paths.** The engine keeps composing relative config and save paths; the
   platform's file functions map them to whatever storage it has.

## Implemented

`Platform` methods beyond the `DG_*`, sound and music ones (every one has a
default: no files, `quit` panics):

```rust
fn open(&self, path: &str) -> Option<FileHandle>;
fn len(&self, file: FileHandle) -> u64;
fn read_at(&self, file: FileHandle, offset: u64, buffer: &mut [u8]) -> usize;
fn close(&self, file: FileHandle);
fn read_file(&self, path: &str) -> Option<Vec<u8>>;
fn write_file(&self, path: &str, data: &[u8]) -> bool;
fn rename(&self, from: &str, to: &str) -> bool;
fn remove(&self, path: &str) -> bool;
fn exists(&self, path: &str) -> bool;
fn create_dir(&self, path: &str) -> bool;
fn error_box(&self, message: &str);
fn quit(&self, code: i32) -> !;
```

`FileHandle(pub usize)` is the platform's number for an open file. The zone
takes its block from the global allocator (`alloc::alloc` in
`AutoAllocMemory`). The host implements the file methods with `std::fs`,
`error_box` with Zenity and `quit` with `std::process::exit`
(`src/platform/host/system.rs`); the Vesper placeholder keeps the defaults.

# doomgeneric-sys

Raw FFI bindings to the [doomgeneric](https://github.com/ozkl/doomgeneric)
Doom engine.

This crate compiles the vendored doomgeneric C source (located in
`../vendor/doomgeneric`) into a static library using the
[`cc`](https://docs.rs/cc) crate, and exposes the minimal set of C symbols
needed to drive the engine from Rust:

| Symbol | Description |
|--------|-------------|
| `DG_ScreenBuffer` | Pointer to the 640 × 400 BGRA frame buffer |
| `doomgeneric_Create` | Initialise the engine (call once) |
| `doomgeneric_Tick` | Advance the engine by one tick |

Platform callbacks (`DG_Init`, `DG_DrawFrame`, `DG_SleepMs`, `DG_GetTicksMs`,
`DG_GetKey`, `DG_SetWindowTitle`) must be provided by the linking binary as
`#[no_mangle] pub extern "C"` functions.  The `room` crate provides these.

## Build flags

The C source is compiled with `-DNORMALUNIX -DLINUX -D_DEFAULT_SOURCE`.
`FEATURE_MULTIPLAYER` and `FEATURE_SOUND` are intentionally *not* defined,
which causes the networking and sound subsystems to be compiled out via
`#ifdef` guards in the engine source.

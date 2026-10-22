# room

<div align="center">
  <img src=".readme/room.png" alt="room screenshot" />
</div>

A complete Rust port of [doomgeneric](https://github.com/ozkl/doomgeneric) using
[winit](https://github.com/rust-windowing/winit) and [wgpu](https://github.com/gfx-rs/wgpu)
for the platform layer.

## What is this?

`room` is a **complete Rust port** of the classic DOOM engine based on
[doomgeneric](https://github.com/ozkl/doomgeneric). Every engine module has
been rewritten in native Rust; no C engine code remains linked. However, it is not
idiomatic Rust yet, and contributions are welcome.

The port was done **module-by-module**:

1. Each module was rewritten in Rust inside `room/src/doom/` and exported
   with `#[no_mangle] extern "C"` so the linker picked the Rust symbol
   instead of the C one.
2. Once replaced, the corresponding `.c` file was removed from
   `doomgeneric-sys/build.rs`.
3. The platform layer (window creation, GPU rendering, keyboard input) is
   pure Rust, built on [winit](https://github.com/rust-windowing/winit)
   and [wgpu](https://github.com/gfx-rs/wgpu).

The `lib_sources` list in `doomgeneric-sys/build.rs` is now empty — all engine
subsystems (`r_draw`, `p_setup`, `z_zone`, `g_game`, `d_main`, and every other
module) are native Rust.

A regression-test harness (`room/src/doom/c_tests/`) runs the original C
functions alongside their Rust replacements to verify bit-for-bit behavioural
compatibility before a module is declared ported.

Sound effects and music are implemented via [rodio](https://github.com/RustAudio/rodio). SFX uses stereo panning; music uses the [Roland SC-55 SoundFont](https://github.com/GuihongWang/SC55Soundfont) via [rustysynth](https://github.com/sinshu/rustysynth). A custom soundfont can be specified with `-sf2 <path>`.

## Project layout

```
room/
├── Cargo.toml               Workspace manifest
├── vendor/
│   └── doomgeneric/         Vendored C source from ozkl/doomgeneric
├── doomgeneric-sys/         FFI + C compilation crate
│   ├── build.rs             Compiles remaining C modules via the `cc` crate
│   └── src/lib.rs           Declarations for C entry points (doomgeneric_Create, etc.)
└── room/                    Rust binary crate
    └── src/
        ├── main.rs          winit ApplicationHandler and entry point
        ├── gpu.rs           wgpu renderer (texture upload + fullscreen blit)
        ├── platform/
        │   ├── mod.rs       DG_* C-callable platform callbacks
        │   └── keys.rs      winit KeyCode → Doom key byte mapping
        └── doom/
            ├── mod.rs       Rust reimplementations of ported engine modules
            ├── c_ffi.rs     FFI declarations for still-C modules (used by tests)
            ├── c_tests/     Regression tests comparing C vs Rust behaviour
            ├── d_main.rs    Ported main entry point and game init
            ├── r_draw.rs    Ported renderer core
            ├── p_setup.rs   Ported map loader
            ├── z_zone.rs    Ported zone memory allocator
            └── …            One `.rs` module per original `.c` file
```

## Prerequisites

- A Rust toolchain (stable, ≥ 1.75).
- A C compiler (`gcc` or `clang`).
- A Doom WAD file (`doom1.wad`, `doom.wad`, `doom2.wad`, `freedoom1.wad`, …).
  The shareware episode is freely available from many sources.

## Building

```bash
cargo build --release
```

## Running

```bash
cargo run --release -- -iwad /path/to/doom1.wad
```

Any arguments after `--` are forwarded to the Doom engine unchanged.

## How it works

The engine runs inside the winit event loop:

1. `ApplicationHandler::resumed` creates the window and initialises the wgpu
   renderer, storing both in thread-local statics accessible to the `DG_*`
   callbacks.
2. `ApplicationHandler::about_to_wait` calls `doomgeneric_Create` on the first
   invocation (which initialises all Doom subsystems and runs the first tick),
   then `doomgeneric_Tick` on every subsequent invocation.
3. `DG_DrawFrame` (called from inside `doomgeneric_Tick`) uploads the 640 × 400
   BGRA pixel buffer produced by `I_FinishUpdate` to a wgpu texture and blits
   it to the swapchain surface via a fullscreen-quad render pass.
4. Keyboard events collected by `ApplicationHandler::window_event` are placed
   into a `VecDeque`; `DG_GetKey` pops them one at a time per tick.

## Known limitations

- **No mouse support.**  Mouse aiming / strafing are not yet implemented.
- **No joystick support.**
- **Single player only.**  Networking (`FEATURE_MULTIPLAYER`) is not compiled in.
- The renderer performs a nearest-neighbour upscale from the native 640 × 400
  resolution to the window size; the window is currently fixed at 640 × 400.

## Ported modules

All vendored `.c` modules have been removed from `doomgeneric-sys/build.rs` and
fully replaced by Rust code in the `room` crate. Port complete.

### Engine core / game loop

- [x] `d_event.c`
- [x] `d_items.c`
- [x] `d_iwad.c`
- [x] `d_loop.c`
- [x] `d_main.c`
- [x] `d_mode.c`
- [x] `d_net.c`
- [x] `doomdef.c`
- [x] `doomstat.c`
- [x] `dstrings.c`
- [x] `dummy.c`
- [x] `doomgeneric.c`

### Game logic (`g_*`, `p_*`)

- [x] `g_game.c`
- [x] `p_ceilng.c`
- [x] `p_doors.c`
- [x] `p_enemy.c`
- [x] `p_floor.c`
- [x] `p_inter.c`
- [x] `p_lights.c`
- [x] `p_map.c`
- [x] `p_maputl.c`
- [x] `p_mobj.c`
- [x] `p_plats.c`
- [x] `p_pspr.c`
- [x] `p_saveg.c`
- [x] `p_setup.c`
- [x] `p_sight.c`
- [x] `p_spec.c`
- [x] `p_switch.c`
- [x] `p_telept.c`
- [x] `p_tick.c`
- [x] `p_user.c`

### Renderer (`r_*`)

- [x] `r_bsp.c`
- [x] `r_data.c`
- [x] `r_draw.c`
- [x] `r_main.c`
- [x] `r_plane.c`
- [x] `r_segs.c`
- [x] `r_sky.c`
- [x] `r_things.c`

### Automap / HUD / status bar / finale / intermission

- [x] `am_map.c`
- [x] `hu_lib.c`
- [x] `hu_stuff.c`
- [x] `st_lib.c`
- [x] `st_stuff.c`
- [x] `f_finale.c`
- [x] `f_wipe.c`
- [x] `wi_stuff.c`
- [x] `statdump.c`

### Menu / misc / math

- [x] `m_argv.c`
- [x] `m_bbox.c`
- [x] `m_cheat.c`
- [x] `m_config.c`
- [x] `m_controls.c`
- [x] `m_fixed.c`
- [x] `m_menu.c`
- [x] `m_misc.c`
- [x] `m_random.c`
- [x] `tables.c`
- [x] `info.c`

### Platform / system (doomgeneric side, not the Rust host)

- [x] `i_cdmus.c`
- [x] `i_endoom.c`
- [x] `i_input.c`
- [x] `i_joystick.c`
- [x] `i_scale.c`
- [x] `i_sound.c`
- [x] `i_system.c`
- [x] `i_timer.c`
- [x] `i_video.c`

### Sound effects and music subsystem

- [x] `s_sound.c` - sound and music state machine fully ported
- [x] `sounds.c` - SFX and music tables fully ported
- [x] `i_sound.c` - SFX playback via rodio (stereo pan); music via rustysynth + SC-55 soundfont

### Video / WAD / memory / utilities

- [x] `v_video.c`
- [x] `w_checksum.c`
- [x] `w_file.c`
- [x] `w_file_stdc.c`
- [x] `w_main.c`
- [x] `w_wad.c`
- [x] `memio.c`
- [x] `sha1.c`
- [x] `z_zone.c`

## License

This repository includes the unmodified Doom shareware IWAD, `doom1.wad`,
copyright id Software. It is included under the Doom shareware distribution
terms. The full registered/commercial Doom IWADs are not included and are
not redistributable.

# Doom source prefixes and Room modules

Source files: `prefix_file` becomes `file` in the `prefix/` module.

## Module prefixes

| Prefix   | Meaning            | Responsibility                                                                 | Module            |
|----------|--------------------|--------------------------------------------------------------------------------|-------------------|
| `d_`     | Doom               | Main game loop, events, game state, startup definitions                        | `doom::`          |
| `g_`     | Game               | Gameplay control, game actions, saving/loading, level transitions              | `game::`          |
| `p_`     | Play               | World simulation: physics, collision, movement, thinkers, monsters, weapons    | `play::`          |
| `r_`     | Renderer           | BSP traversal, walls, floors, sprites, textures, lighting                      | `render_3d::`     |
| `s_`     | Sound              | High-level sound effects and music management                                  | `sound::`         |
| `i_`     | Interface          | Platform-dependent implementation: timing, sound, input, system services       | `interface::`     |
| `v_`     | Video              | Screen buffers, palette handling, drawing primitives                           | `render_2d::`     |
| `w_`     | WAD                | WAD archive access, lump lookup and loading                                    | `wad::`           |
| `z_`     | Zone               | Zone memory allocator and memory management                                    | `zone::`          |
| `m_`     | Miscellaneous      | Menus, configuration, random numbers, utility functions                        | `misc::`          |
| `hu_`    | Heads-Up           | HUD, status messages, text rendering                                           | `hud::`           |
| `st_`    | Status             | Status bar, health/armor/ammo displays, face animation                         | `status::`        |
| `am_`    | AutoMap            | Automap rendering and controls                                                 | `automap::`       |
| `wi_`    | World Intermission | Intermission screens, level statistics, map progression                        | `intermission::`  |
| `in_`    | Intermission       | Intermission-related definitions; less common than `wi_`                       | `intermission::`  |
| `f_`     | Finale             | End-of-episode and endgame sequences                                           | `finale::`        |

## Files placed by contents

Files without a prefix, and the `m_` math files, go to the module their contents belong to.

| File          | Contents                                          | Module                   |
|---------------|---------------------------------------------------|--------------------------|
| `doomgeneric` | `Platform` trait and engine entry points          | `platform`               |
| `doomstat`    | Game-wide state globals                           | `doom::stat`             |
| `dstrings`    | Quit messages                                     | `doom::strings`          |
| `dummy`       | Network stubs `drone`, `net_client_connected`     | `doom::net`              |
| `info`        | Thing and state tables                            | `play::info`             |
| `statenum`    | `S_*` state indices (`info.h` in C)               | `play::info`             |
| `sounds`      | Sound effect and music tables                     | `sound::info`            |
| `mus2midi`    | MUS to MIDI conversion                            | `sound::mus2midi`        |
| `doomdata`    | On-disk WAD record formats                        | `wad::format`            |
| `tables`      | Fixed-point trigonometry and gamma tables         | `math::tables`           |
| `m_fixed`     | Fixed-point arithmetic                            | `math::fixed`            |
| `m_bbox`      | Bounding boxes                                    | `math::bbox`             |
| `sha1`        | SHA-1 hashing                                     | `misc::sha1`             |
| `deh_str`     | `DEH_String` hook                                 | `dehacked`               |
| `doomkeys`    | Key codes                                         | `interface::keys`        |
| `statdump`    | End-of-level statistics capture                   | `intermission::statdump` |
| `memio`       | In-memory `FILE` streams                          | not placed yet           |
| `c_ffi`       | Room-only: C-layout copies of shared types        | not placed yet           |

## Function prefixes

These appear in function names rather than identifying entire source modules.

| Prefix | Meaning                                                                   | Module       |
|--------|---------------------------------------------------------------------------|--------------|
| `A_`   | Action routines invoked by animation/state frames, such as monster attacks | `action::`   |
| `T_`   | Thinker routines executed on game ticks                                   | `thinker::`  |
| `PIT_` | Callbacks for spatial blockmap iterators                                  |              |
| `DG_`  | DoomGeneric platform interface, such as `DG_DrawFrame`                    | `platform::` |

## Navigating the code

- `p_` is the gameplay simulation, while `g_` is the high-level game controller.
- `r_` implements the 3D renderer, whereas `v_` provides general-purpose 2D drawing operations, and `i_` handles platform integration.
- `wi_` is the actual intermission module prefix; `in_` is not a standard Doom subsystem prefix.

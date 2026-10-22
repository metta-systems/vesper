//! The Doom engine: Rust ports of the `vendor/doomgeneric/` C sources.
//!
//! Each C file `prefix_file.c` is ported as `file` inside the subsystem
//! module its prefix names (`p_enemy.c` → [`play::enemy`]); see `doom.md`
//! for the prefix table. A subsystem with a single C file is a single
//! module (`z_zone.c` → [`zone`]), and a C file named after its subsystem is
//! that module's root (`m_misc.c` → [`misc`]). Files without a prefix go to
//! the subsystem their contents belong to (`info.c` → [`play::info`]), also
//! listed in `doom.md`.
//!
//! Each submodule carries its own `//!` module header documenting the C file
//! it ports and what it does; see the individual files for details.

pub mod automap;
pub mod dehacked;
pub mod doom;
pub mod finale;
pub mod game;
pub mod hud;
pub mod interface;
pub mod intermission;
pub mod map;
pub mod math;
pub mod misc;
pub mod platform;
pub mod play;
pub mod render_2d;
pub mod render_3d;
pub mod sound;
pub mod status;
pub mod wad;
pub mod zone;

pub mod memio;

//! The Doom engine: Rust ports of the `vendor/doomgeneric/` C sources.
//!
//! Each C file `prefix_file.c` is ported as `file` inside the subsystem
//! module its prefix names (`p_enemy.c` → [`play::enemy`]); see `doom.md`
//! for the prefix table. A subsystem with a single C file is a single
//! module (`z_zone.c` → [`zone`]), and a C file named after its subsystem is
//! that module's root (`m_misc.c` → [`misc`]). Files without a prefix are
//! still direct children of this module.
//!
//! Each submodule carries its own `//!` module header documenting the C file
//! it ports and what it does; see the individual files for details.

pub mod automap;
pub mod doom;
pub mod finale;
pub mod game;
pub mod hud;
pub mod interface;
pub mod intermission;
pub mod misc;
pub mod play;
pub mod render_2d;
pub mod render_3d;
pub mod sound;
pub mod status;
pub mod wad;
pub mod zone;

pub mod c_ffi;
pub mod deh_str;
pub mod doomgeneric;
pub mod doomkeys;
pub mod doomstat;
pub mod dstrings;
pub mod dummy;
pub mod info;
pub mod memio;
pub mod mus2midi;
pub mod sha1;
pub mod sounds;
pub mod statdump;
pub mod statenum;
pub mod tables;

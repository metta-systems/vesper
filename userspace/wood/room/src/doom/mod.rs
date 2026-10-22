//! Module aggregator for all `doom/*.rs` submodules.
//!
//! This file mirrors the file layout of `vendor/doomgeneric/` one-for-one:
//! each `pub mod` below corresponds to a single `.c` file in the original
//! engine source.  Submodules export their public API with
//! `#[no_mangle] extern "C"` so the residual C code (or other Rust ports that
//! still go through the C ABI for parity testing) resolves them at final link
//! time.
//!
//! Adding a new ported file means dropping a `pub mod foo;` here in
//! alphabetical order within its subsystem block.  The grouping (core, render,
//! physics, HUD, system, utilities) matches `DOCUMENTING.md` and the spec at
//! `docs/superpowers/specs/2026-05-17-doc-commenting-design.md`.
//!
//! Each submodule carries its own `//!` module header documenting the C file
//! it ports and what it does; see the individual files for details.

pub mod am_map;
pub mod c_ffi;

pub mod d_event;
pub mod d_items;
pub mod d_iwad;
pub mod d_loop;
pub mod d_main;
pub mod d_mode;
pub mod d_net;
pub mod d_player;
pub mod doomgeneric;
pub mod doomkeys;
pub mod doomstat;
pub mod dstrings;
pub mod dummy;
pub mod f_finale;
pub mod f_wipe;
pub mod g_game;
pub mod hu_lib;
pub mod hu_stuff;
pub mod i_cdmus;
pub mod i_endoom;
pub mod i_input;
pub mod i_joystick;
pub mod i_scale;
pub mod i_sound;
pub mod i_system;
pub mod i_timer;
pub mod i_video;
pub mod info;
pub mod m_argv;
pub mod m_bbox;
pub mod m_cheat;
pub mod m_config;
pub mod m_controls;
pub mod m_fixed;
pub mod m_menu;
pub mod m_misc;
pub mod m_random;
pub mod memio;
pub mod p_ceilng;
pub mod p_doors;
pub mod p_enemy;
pub mod p_floor;
pub mod p_inter;
pub mod p_lights;
pub mod p_map;
pub mod p_maputl;
pub mod p_mobj;
pub mod p_plats;
pub mod p_pspr;
pub mod p_saveg;
pub mod p_setup;
pub mod p_sight;
pub mod p_spec;
pub mod p_switch;
pub mod p_telept;
pub mod p_tick;
pub mod p_user;
pub mod r_bsp;
pub mod r_data;
pub mod r_draw;
pub mod r_main;
pub mod r_plane;
pub mod r_segs;
pub mod r_sky;
pub mod r_things;
pub mod s_sound;
pub mod sha1;
pub mod sounds;
pub mod st_lib;
pub mod st_stuff;
pub mod statdump;
pub mod statenum;
pub mod tables;
pub mod v_video;
pub mod w_checksum;
pub mod w_file;
pub mod w_main;
pub mod w_wad;
pub mod wi_stuff;
pub mod z_zone;

#[cfg(test)]
mod c_tests;

//! Unit tests that call original C functions directly.
//!
//! Each submodule tests a specific unported C module, establishing
//! behavioral baselines before the module is ported to Rust.

mod am_map_c;
mod d_loop_c;
mod d_main_c;
mod g_game_c;
mod harness;
mod hu_stuff_c;
mod i_scale_c;
mod lookup_tables;
mod map_data_structs;
mod p_enemy_c;
mod p_inter_c;
mod p_map_c;
mod p_maputl_c;
mod p_mobj_c;
mod p_pspr_c;
mod p_saveg_c;
mod p_spec_c;
mod p_switch_c;
mod r_data_c;
mod r_draw_c;
mod r_segs_c;
mod r_things_c;
mod st_stuff_c;
mod struct_layouts;
mod wi_stuff_c;
mod wrapping_arithmetic;

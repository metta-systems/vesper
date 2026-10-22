//! Regression tests of the ported engine modules.
//!
//! Each submodule pins the constants, table contents and behaviour of one
//! original C module (named `<c_module>_c`), so a change to the Rust port
//! that drifts from the C engine fails here. They test the `room` library
//! through its public API.

// These tests pin constant values, so asserting on constants is the point.
#![allow(clippy::assertions_on_constants)]
#![allow(non_snake_case, non_upper_case_globals, static_mut_refs)]

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

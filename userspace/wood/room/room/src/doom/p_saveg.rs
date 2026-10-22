//! Rust port of vendor/doomgeneric/p_saveg.c.
//!
//! Implements save-game serialization and deserialization for the entire Doom
//! game state. All active map objects (mobjs), player state, world geometry
//! deltas, thinker chains, and sector specials (ceilings, doors, floors,
//! platforms, lights) are written to or read from a `FILE *` stream managed
//! by `g_game.c`.
//!
//! The on-disk format is a flat byte stream. Multi-byte integers are always
//! little-endian regardless of host byte order. Sector and state pointers are
//! serialized as array indices; player pointers use a 1-based index (0 means
//! NULL). Thinker function pointers stored in saves are raw addresses that
//! must be re-established on load - this port guards against transmuting zero
//! to a function pointer (which is UB) by mapping `0` to `None`.
//!
//! Notable differences from the C source:
//! - `saveg_write8` increments `savegamelength`; the C version does not.
//! - `saveg_read16`/`saveg_write16` operate on `u16`/`u16` rather than C
//!   `short`/`short`, avoiding sign-extension ambiguity.
//! - State indices are bounds-checked on read; out-of-range values set
//!   `savegame_error` and clamp to `states[0]`.
//! - `P_SaveGameFile` always rebuilds the filename from the current `slot`
//!   argument (matching C behavior); the allocation is reused across calls.
//! - `TEMP_SAVE_FILENAME` and `SAVE_FILENAME` are module-level statics rather
//!   than function-local statics.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::i_error;
use std::ffi::{c_char, c_int, c_void};
use std::ptr;

use crate::doom::c_ffi::{SAVEGAME_EOF, VERSIONSIZE};
use crate::doom::d_player::{
    players, PlayerT, PspdefT, TiccmdT, MAXPLAYERS, NUMAMMO, NUMCARDS, NUMPOWERS, NUMPSPRITES,
    NUMWEAPONS,
};
use crate::doom::info::{mobjinfo, states, MobjInfo, State, NUMSTATES};
use crate::doom::p_ceilng::{ceiling_t, P_AddActiveCeiling, T_MoveCeiling, MAXCEILINGS};
use crate::doom::p_doors::{vldoor_t, T_VerticalDoor};
use crate::doom::p_floor::{floormove_t, side_t, T_MoveFloor};
use crate::doom::p_lights::{
    glow_t, lightflash_t, line_t, sector_t, strobe_t, T_Glow, T_LightFlash, T_StrobeFlash,
};
use crate::doom::p_plats::{plat_t, P_AddActivePlat, T_PlatRaise};
use crate::doom::p_tick::{
    actionf_t, leveltime, thinker_t, thinkercap, P_AddThinker, P_InitThinkers,
};
use crate::doom::z_zone::{Z_Free, Z_Malloc, PU_LEVEL, PU_LEVSPEC};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum length (in bytes, including NUL) of the human-readable save-game
/// description string written at the start of every save file.
/// Corresponds to `SAVESTRINGSIZE` in the C source.
const SAVESTRINGSIZE: usize = 24;

// ---------------------------------------------------------------------------
// Globals — now owned by Rust since p_saveg.c is ported.
// g_game.c still writes save_stream to open/close the file.
// ---------------------------------------------------------------------------

/// Active save-game `FILE *` stream; opened and closed by `g_game.c`.
/// Exported with C linkage so `g_game.c` can assign it before calling any
/// archive or unarchive function.
#[no_mangle]
pub static mut save_stream: *mut libc::FILE = ptr::null_mut();

/// Running byte count of data written to the current save stream.
/// Incremented by `saveg_write8`; read by `g_game.c` after saving completes.
/// C origin: `savegamelength` in `p_saveg.c`.
#[no_mangle]
pub static mut savegamelength: c_int = 0;

/// Non-zero when a read or write error has occurred on `save_stream`.
/// Set to `1` on the first I/O failure; callers check this after
/// archiving/unarchiving to decide whether to accept the save.
/// C origin: `savegame_error` in `p_saveg.c`.
#[no_mangle]
pub static mut savegame_error: c_int = 0;

// ---------------------------------------------------------------------------
// External C declarations
// ---------------------------------------------------------------------------

extern "C" {
    /// C-side mobj thinker; used for type dispatch when reading/writing the
    /// thinker chain.
    fn P_MobjThinker(mobj: *mut c_void);
    /// Places a map object into the sector and blockmap spatial structures.
    fn P_SetThingPosition(thing: *mut c_void);
    /// Removes a mobj from the world without calling its death logic; used
    /// when clearing the thinker list before loading.
    fn P_RemoveMobj(th: *mut c_void);
    /// Returns the vanilla Doom version code (e.g. 109) used in the save
    /// header version string.
    fn G_VanillaVersionCode() -> c_int;

    /// Global array of all map sectors; indexed by sector number.
    static mut sectors: *mut sector_t;
    /// Global array of all map linedefs; indexed by linedef number.
    static mut lines: *mut line_t;
    /// Global array of all map sidedefs; indexed by sidedef number.
    static mut sides: *mut side_t;
    /// Count of entries in `sectors`.
    static mut numsectors: c_int;
    /// Count of entries in `lines`.
    static mut numlines: c_int;

    /// Path to the save-game directory (NUL-terminated C string); assigned
    /// from the command-line or platform default by `g_game.c`.
    static mut savegamedir: *mut c_char;
}

// ---------------------------------------------------------------------------
// Low-level I/O — uses the C FILE * managed by g_game.c.
// ---------------------------------------------------------------------------

use std::os::raw::c_long;

/// Reads one byte from `save_stream`.
///
/// Sets `savegame_error = 1` on the first short read; subsequent calls still
/// return `0` but do not double-set the flag. C origin: `saveg_read8`.
unsafe fn saveg_read8() -> u8 {
    let mut result: u8 = 0;
    let n = libc::fread(&raw mut result as *mut c_void, 1, 1, save_stream);
    if n < 1 && savegame_error == 0 {
        savegame_error = 1;
    }
    result
}

/// Writes one byte to `save_stream` and increments `savegamelength`.
///
/// Sets `savegame_error = 1` on the first short write.
///
/// Note: the C version does not increment `savegamelength` here; this port
/// does so to avoid a separate accounting pass.
/// C origin: `saveg_write8`.
unsafe fn saveg_write8(value: u8) {
    let n = libc::fwrite(&value as *const u8 as *const c_void, 1, 1, save_stream);
    if n < 1 && savegame_error == 0 {
        savegame_error = 1;
    }
    savegamelength += 1;
}

/// Reads a little-endian 16-bit unsigned integer from `save_stream`.
///
/// Returns the reconstructed value; any read error is recorded in
/// `savegame_error`. C origin: `saveg_read16` (which used `short`; here `u16`
/// avoids sign-extension ambiguity).
unsafe fn saveg_read16() -> u16 {
    let a = saveg_read8() as u16;
    let b = saveg_read8() as u16;
    a | (b << 8)
}

/// Writes a little-endian 16-bit unsigned integer to `save_stream`.
///
/// C origin: `saveg_write16`.
unsafe fn saveg_write16(value: u16) {
    saveg_write8((value & 0xff) as u8);
    saveg_write8(((value >> 8) & 0xff) as u8);
}

/// Reads a little-endian 32-bit unsigned integer from `save_stream`.
///
/// C origin: `saveg_read32` (which returned `int`; here `u32` to make
/// bit-pattern semantics explicit before callers cast to signed types).
unsafe fn saveg_read32() -> u32 {
    let a = saveg_read8() as u32;
    let b = saveg_read8() as u32;
    let c = saveg_read8() as u32;
    let d = saveg_read8() as u32;
    a | (b << 8) | (c << 16) | (d << 24)
}

/// Writes a little-endian 32-bit unsigned integer to `save_stream`.
///
/// C origin: `saveg_write32`.
unsafe fn saveg_write32(value: u32) {
    saveg_write8((value & 0xff) as u8);
    saveg_write8(((value >> 8) & 0xff) as u8);
    saveg_write8(((value >> 16) & 0xff) as u8);
    saveg_write8(((value >> 24) & 0xff) as u8);
}

/// Reads and discards padding bytes to align the stream to the next 4-byte
/// boundary.
///
/// Padding is `(4 - (pos & 3)) & 3` bytes, where `pos` is the current stream
/// position. A stream already aligned reads zero bytes. C origin:
/// `saveg_read_pad`.
unsafe fn saveg_read_pad() {
    let pos = libc::ftell(save_stream) as c_long;
    let padding = (4 - (pos & 3)) & 3;
    for _ in 0..padding {
        saveg_read8();
    }
}

/// Writes NUL padding bytes to align the stream to the next 4-byte boundary.
///
/// See `saveg_read_pad` for the alignment formula. C origin:
/// `saveg_write_pad`.
unsafe fn saveg_write_pad() {
    let pos = libc::ftell(save_stream) as c_long;
    let padding = (4 - (pos & 3)) & 3;
    for _ in 0..padding {
        saveg_write8(0);
    }
}

/// Reads a 32-bit enum value from the stream as a `u32`.
///
/// Enum values are always stored as 32-bit little-endian integers.
/// C origin: the `saveg_read_enum` macro (alias for `saveg_read32`).
unsafe fn saveg_read_enum() -> u32 {
    saveg_read32()
}

/// Writes a 32-bit enum value to the stream.
///
/// C origin: the `saveg_write_enum` macro (alias for `saveg_write32`).
unsafe fn saveg_write_enum(value: u32) {
    saveg_write32(value);
}

// ---------------------------------------------------------------------------
// Struct serialization helpers
// ---------------------------------------------------------------------------

/// Serializes a `sector_t` pointer as a zero-based index into the global
/// `sectors` array.
///
/// Returns `0` for a null pointer. The C equivalent uses pointer subtraction
/// (`sector - sectors`). Callers must ensure `sector` actually points into
/// `sectors` when non-null.
unsafe fn saveg_write_sector_ptr(sector: *const sector_t) -> u32 {
    if sector.is_null() {
        0
    } else {
        sector.offset_from(sectors) as u32
    }
}

/// Deserializes a sector index from the save stream into a `*mut sector_t`.
///
/// `index` is an offset into the global `sectors` array. No bounds check is
/// performed; callers must trust that the index was written by a valid
/// `saveg_write_sector_ptr` call.
unsafe fn saveg_read_sector_ptr(index: u32) -> *mut sector_t {
    sectors.add(index as usize)
}

/// Serializes a `State` pointer as a zero-based index into the global `states`
/// array.
///
/// Returns `0` for a null pointer. The C equivalent computes `state - states`.
unsafe fn saveg_write_state_ptr(state: *const State) -> u32 {
    if state.is_null() {
        0
    } else {
        state.offset_from(std::ptr::addr_of!(states[0])) as u32
    }
}

/// Deserializes a state index into a `*mut State`.
///
/// If `index >= NUMSTATES`, sets `savegame_error = 1` and clamps to
/// `states[0]` rather than accessing out-of-bounds memory. The C version
/// performs no such guard (`&states[saveg_read32()]` would silently
/// out-of-bounds).
unsafe fn saveg_read_state_ptr(index: u32) -> *mut State {
    if index as usize >= NUMSTATES {
        savegame_error = 1;
        return std::ptr::addr_of_mut!(states[0]);
    }
    std::ptr::addr_of_mut!(states[0]).add(index as usize)
}

/// Serializes a `PlayerT` pointer as a 1-based player index.
///
/// Returns `0` for a null pointer; otherwise returns
/// `(player - players) + 1`. The 1-based encoding reserves `0` to represent
/// NULL. C origin: `str->player - players + 1`.
unsafe fn saveg_write_player_ptr(player: *const PlayerT) -> u32 {
    if player.is_null() {
        0
    } else {
        (player.offset_from(std::ptr::addr_of!(players[0])) as u32) + 1
    }
}

/// Deserializes a 1-based player index into a `*mut PlayerT`.
///
/// `value == 0` maps to null; `value > 0` indexes `players[value - 1]`.
/// C origin: `&players[pl - 1]`.
unsafe fn saveg_read_player_ptr(value: u32) -> *mut PlayerT {
    if value == 0 {
        std::ptr::null_mut()
    } else {
        std::ptr::addr_of_mut!(players[0]).add((value - 1) as usize)
    }
}

// ---------------------------------------------------------------------------
// Thinker function pointer reassignment helpers
// ---------------------------------------------------------------------------

/// Generates a helper function that constructs an `actionf_t` holding a
/// type-erased pointer to a specific thinker function.
///
/// Each generated function (e.g. `actionf_p1_move_ceiling`) transmutes the
/// concrete thinker function pointer (`T_MoveCeiling`, etc.) to the generic
/// `unsafe extern "C" fn(*mut c_void)` required by `actionf_t::acp1`. This
/// is the Rust equivalent of the C cast
/// `(actionf_p1)T_MoveCeiling`.
macro_rules! make_actionf_p1 {
    ($fn_name:ident, $arg_ty:ty, $c_fn:expr) => {
        fn $fn_name() -> actionf_t {
            actionf_t {
                acp1: Some(unsafe {
                    core::mem::transmute::<
                        unsafe extern "C" fn(*mut $arg_ty),
                        unsafe extern "C" fn(*mut c_void),
                    >($c_fn)
                }),
            }
        }
    };
}

// Returns an `actionf_t` for `T_MoveCeiling` (ceiling special thinker).
make_actionf_p1!(actionf_p1_move_ceiling, ceiling_t, T_MoveCeiling);
// Returns an `actionf_t` for `T_VerticalDoor` (door special thinker).
make_actionf_p1!(actionf_p1_vertical_door, vldoor_t, T_VerticalDoor);
// Returns an `actionf_t` for `T_MoveFloor` (floor special thinker).
make_actionf_p1!(actionf_p1_move_floor, floormove_t, T_MoveFloor);
// Returns an `actionf_t` for `T_PlatRaise` (platform special thinker).
make_actionf_p1!(actionf_p1_plat_raise, plat_t, T_PlatRaise);
// Returns an `actionf_t` for `T_LightFlash` (random light-flash thinker).
make_actionf_p1!(actionf_p1_light_flash, lightflash_t, T_LightFlash);
// Returns an `actionf_t` for `T_StrobeFlash` (strobe light thinker).
make_actionf_p1!(actionf_p1_strobe_flash, strobe_t, T_StrobeFlash);
// Returns an `actionf_t` for `T_Glow` (glow light thinker).
make_actionf_p1!(actionf_p1_glow, glow_t, T_Glow);

// ---------------------------------------------------------------------------
// Struct serialization
// ---------------------------------------------------------------------------

//
// mapthing_t
//

/// Reads a `mapthing_t` from the save stream into `*mt`.
///
/// Reads five `i16` fields in order: x, y, angle, type, options.
/// C origin: `saveg_read_mapthing_t`.
unsafe fn saveg_read_mapthing_t(mt: *mut crate::doom::c_ffi::mapthing_t) {
    let s = &mut *mt;
    s.x = saveg_read16() as i16;
    s.y = saveg_read16() as i16;
    s.angle = saveg_read16() as i16;
    s.r#type = saveg_read16() as i16;
    s.options = saveg_read16() as i16;
}

/// Writes a `mapthing_t` to the save stream from `*mt`.
///
/// Writes five `i16` fields in order: x, y, angle, type, options.
/// C origin: `saveg_write_mapthing_t`.
unsafe fn saveg_write_mapthing_t(mt: *const crate::doom::c_ffi::mapthing_t) {
    let s = &*mt;
    saveg_write16(s.x as u16);
    saveg_write16(s.y as u16);
    saveg_write16(s.angle as u16);
    saveg_write16(s.r#type as u16);
    saveg_write16(s.options as u16);
}

//
// thinker_t
//

/// Reads a `thinker_t` header (prev, next, function) from the save stream.
///
/// The `prev` and `next` pointer values stored in the stream are raw addresses
/// from the saving session and are not meaningful in the loading session; they
/// will be overwritten when the thinker is re-linked. A stream value of `0`
/// for the function pointer becomes `None` rather than `Some(NULL)` to avoid
/// calling a null function pointer (which would be UB).
/// C origin: `saveg_read_thinker_t`.
unsafe fn saveg_read_thinker_t(th: *mut thinker_t) {
    let s = &mut *th;
    // Read prev/next as raw pointer indices (will be rebuilt)
    s.prev = saveg_read32() as usize as *mut thinker_t;
    s.next = saveg_read32() as usize as *mut thinker_t;
    // 0 in the save stream means no function; transmuting 0 to fn is UB.
    let raw = saveg_read32() as usize;
    s.function.acp1 = if raw == 0 {
        None
    } else {
        Some(core::mem::transmute::<
            usize,
            unsafe extern "C" fn(*mut c_void),
        >(raw))
    };
}

/// Writes a `thinker_t` header (prev, next, function) to the save stream.
///
/// Pointer fields are written as raw 32-bit addresses; the function pointer is
/// written as `0` when absent (`None`). C origin: `saveg_write_thinker_t`.
unsafe fn saveg_write_thinker_t(th: *const thinker_t) {
    let s = &*th;
    saveg_write32(s.prev as u32);
    saveg_write32(s.next as u32);
    saveg_write32(s.function.acp1.map(|f| f as usize as u32).unwrap_or(0));
}

//
// mobj_t — serialized as C struct via FFI
//

/// Reads a full `mobj_t` record from the save stream into the memory at `mobj`.
///
/// The `mobj` pointer must point to a valid `mobj_t`-sized allocation.
/// After reading:
/// - `prev`/`next` in the embedded thinker are raw addresses (stale from the
///   saving session) and will be rebuilt by `P_AddThinker`.
/// - `snext`, `sprev`, `bnext`, `bprev`, `subsector`, `target`, `tracer` are
///   raw saved addresses that callers must NULL out or re-resolve.
/// - `state` is resolved from a stream index via `saveg_read_state_ptr`.
/// - `player` is a 1-based index; re-resolved and the back-pointer
///   `player->mo` is updated in place.
/// - `info` is a raw saved pointer (will be overwritten by caller with
///   `&mobjinfo[type]`).
/// C origin: `saveg_read_mobj_t`.
unsafe fn saveg_read_mobj_t(mobj: *mut c_void) {
    let mo: *mut crate::doom::c_ffi::mobj_t = mobj as *mut crate::doom::c_ffi::mobj_t;

    // thinker_t
    {
        let thinker_ptr = std::ptr::addr_of_mut!((*mo).thinker_prev) as *mut thinker_t;
        (*thinker_ptr).prev = saveg_read32() as usize as *mut thinker_t;
        (*thinker_ptr).next = saveg_read32() as usize as *mut thinker_t;
        let raw = saveg_read32() as usize;
        (*thinker_ptr).function.acp1 = if raw == 0 {
            None
        } else {
            Some(core::mem::transmute::<
                usize,
                unsafe extern "C" fn(*mut c_void),
            >(raw))
        };
    }

    // x, y, z
    (*mo).x = saveg_read32() as c_int;
    (*mo).y = saveg_read32() as c_int;
    (*mo).z = saveg_read32() as c_int;

    // snext, sprev
    (*mo).snext = saveg_read32() as *mut c_void;
    (*mo).sprev = saveg_read32() as *mut c_void;

    // angle, sprite, frame
    (*mo).angle = saveg_read32();
    (*mo).sprite = saveg_read_enum() as c_int;
    (*mo).frame = saveg_read32() as c_int;

    // bnext, bprev
    (*mo).bnext = saveg_read32() as *mut c_void;
    (*mo).bprev = saveg_read32() as *mut c_void;

    // subsector
    (*mo).subsector = saveg_read32() as *mut c_void;

    // floorz, ceilingz, radius, height
    (*mo).floorz = saveg_read32() as c_int;
    (*mo).ceilingz = saveg_read32() as c_int;
    (*mo).radius = saveg_read32() as c_int;
    (*mo).height = saveg_read32() as c_int;

    // momx, momy, momz
    (*mo).momx = saveg_read32() as c_int;
    (*mo).momy = saveg_read32() as c_int;
    (*mo).momz = saveg_read32() as c_int;

    // validcount
    (*mo).validcount = saveg_read32() as c_int;

    // type
    (*mo).type_ = saveg_read_enum() as c_int;

    // info (raw pointer)
    (*mo).info = saveg_read32() as *mut crate::doom::c_ffi::mobjinfo_t;

    // tics
    (*mo).tics = saveg_read32() as c_int;

    // state (index into states array)
    let state_idx = saveg_read32();
    (*mo).state = saveg_read_state_ptr(state_idx) as *mut crate::doom::c_ffi::state_t;

    // flags
    (*mo).flags = saveg_read32() as c_int;

    // health
    (*mo).health = saveg_read32() as c_int;

    // movedir, movecount
    (*mo).movedir = saveg_read32() as c_int;
    (*mo).movecount = saveg_read32() as c_int;

    // target (raw pointer)
    (*mo).target = saveg_read32() as *mut c_void;

    // reactiontime, threshold
    (*mo).reactiontime = saveg_read32() as c_int;
    (*mo).threshold = saveg_read32() as c_int;

    // player (index + 1)
    let pl = saveg_read32();
    if pl > 0 {
        (*mo).player = saveg_read_player_ptr(pl) as *mut c_void;
        let player = (*mo).player as *mut PlayerT;
        if !player.is_null() {
            (*player).mo = mobj as *mut crate::doom::d_player::mobj_t;
        }
    } else {
        (*mo).player = std::ptr::null_mut();
    }

    // lastlook
    (*mo).lastlook = saveg_read32() as c_int;

    // spawnpoint
    saveg_read_mapthing_t(std::ptr::addr_of_mut!((*mo).spawnpoint));

    // tracer (raw pointer)
    (*mo).tracer = saveg_read32() as *mut c_void;
}

/// Writes a full `mobj_t` record to the save stream from `mobj`.
///
/// Pointers (snext, sprev, bnext, bprev, subsector, target, tracer, info) are
/// written as raw 32-bit addresses; they are not portable across sessions but
/// are reconstructed on load. `state` is written as an index into `states`.
/// `player` is written as a 1-based player index (0 for NULL).
/// C origin: `saveg_write_mobj_t`.
unsafe fn saveg_write_mobj_t(mobj: *const c_void) {
    let mo: *const crate::doom::c_ffi::mobj_t = mobj as *const crate::doom::c_ffi::mobj_t;

    // thinker_t
    {
        let thinker_ptr = std::ptr::addr_of!((*mo).thinker_prev) as *const thinker_t;
        saveg_write32((*thinker_ptr).prev as u32);
        saveg_write32((*thinker_ptr).next as u32);
        saveg_write32(
            (*thinker_ptr)
                .function
                .acp1
                .map(|f| f as usize as u32)
                .unwrap_or(0),
        );
    }

    // x, y, z
    saveg_write32((*mo).x as u32);
    saveg_write32((*mo).y as u32);
    saveg_write32((*mo).z as u32);

    // snext, sprev
    saveg_write32((*mo).snext as u32);
    saveg_write32((*mo).sprev as u32);

    // angle, sprite, frame
    saveg_write32((*mo).angle);
    saveg_write_enum((*mo).sprite as u32);
    saveg_write32((*mo).frame as u32);

    // bnext, bprev
    saveg_write32((*mo).bnext as u32);
    saveg_write32((*mo).bprev as u32);

    // subsector
    saveg_write32((*mo).subsector as u32);

    // floorz, ceilingz, radius, height
    saveg_write32((*mo).floorz as u32);
    saveg_write32((*mo).ceilingz as u32);
    saveg_write32((*mo).radius as u32);
    saveg_write32((*mo).height as u32);

    // momx, momy, momz
    saveg_write32((*mo).momx as u32);
    saveg_write32((*mo).momy as u32);
    saveg_write32((*mo).momz as u32);

    // validcount
    saveg_write32((*mo).validcount as u32);

    // type
    saveg_write_enum((*mo).type_ as u32);

    // info
    saveg_write32((*mo).info as u32);

    // tics
    saveg_write32((*mo).tics as u32);

    // state (index)
    let state = (*mo).state as *const State;
    if state.is_null() {
        saveg_write32(0);
    } else {
        saveg_write32(saveg_write_state_ptr(state));
    }

    // flags
    saveg_write32((*mo).flags as u32);

    // health
    saveg_write32((*mo).health as u32);

    // movedir, movecount
    saveg_write32((*mo).movedir as u32);
    saveg_write32((*mo).movecount as u32);

    // target
    saveg_write32((*mo).target as u32);

    // reactiontime, threshold
    saveg_write32((*mo).reactiontime as u32);
    saveg_write32((*mo).threshold as u32);

    // player
    let player = (*mo).player as *const PlayerT;
    if player.is_null() {
        saveg_write32(0);
    } else {
        saveg_write32(saveg_write_player_ptr(player));
    }

    // lastlook
    saveg_write32((*mo).lastlook as u32);

    // spawnpoint
    saveg_write_mapthing_t(std::ptr::addr_of!((*mo).spawnpoint));

    // tracer
    saveg_write32((*mo).tracer as u32);
}

//
// ticcmd_t
//
// Only serialize the 6 fields the C version writes. TiccmdT has extra fields.
//

/// Reads a `ticcmd_t` (player input command snapshot) from the save stream.
///
/// Only the 6 fields serialized by the C version are read:
/// `forwardmove`, `sidemove`, `angleturn`, `consistancy`, `chatchar`,
/// `buttons`. Extra fields present in `TiccmdT` are not touched.
/// C origin: `saveg_read_ticcmd_t`.
unsafe fn saveg_read_ticcmd_t(cmd: *mut TiccmdT) {
    let s = &mut *cmd;
    s.forwardmove = saveg_read8() as i8;
    s.sidemove = saveg_read8() as i8;
    s.angleturn = saveg_read16() as i16;
    s.consistancy = saveg_read16() as u8;
    s.chatchar = saveg_read8();
    s.buttons = saveg_read8();
}

/// Writes the 6 canonical `ticcmd_t` fields to the save stream.
///
/// C origin: `saveg_write_ticcmd_t`.
unsafe fn saveg_write_ticcmd_t(cmd: *const TiccmdT) {
    let s = &*cmd;
    saveg_write8(s.forwardmove as u8);
    saveg_write8(s.sidemove as u8);
    saveg_write16(s.angleturn as u16);
    saveg_write16(s.consistancy as u16);
    saveg_write8(s.chatchar);
    saveg_write8(s.buttons);
}

//
// pspdef_t
//

/// Reads a `pspdef_t` (player sprite definition) from the save stream.
///
/// The `state` field uses a 1-based convention for psprites: index `0` maps
/// to NULL (weapon not active), while any positive index maps to
/// `states[index]`. This differs from `mobj_t` state handling where index `0`
/// refers to `states[0]`.
/// C origin: `saveg_read_pspdef_t`.
unsafe fn saveg_read_pspdef_t(psp: *mut PspdefT) {
    let s = &mut *psp;
    let state_idx = saveg_read32();
    // C guards state == 0 → NULL for pspdef (unlike mobj which always indexes states[]).
    s.state = if state_idx == 0 {
        std::ptr::null_mut()
    } else {
        saveg_read_state_ptr(state_idx) as *mut crate::doom::d_player::state_t
    };
    s.tics = saveg_read32() as c_int;
    s.sx = saveg_read32() as c_int;
    s.sy = saveg_read32() as c_int;
}

/// Writes a `pspdef_t` to the save stream.
///
/// Writes the state index (`state - states`) or `0` for NULL, followed by
/// `tics`, `sx` (fixed-point screen x offset), and `sy` (fixed-point screen y
/// offset). C origin: `saveg_write_pspdef_t`.
unsafe fn saveg_write_pspdef_t(psp: *const PspdefT) {
    let s = &*psp;
    if s.state.is_null() {
        saveg_write32(0);
    } else {
        saveg_write32(saveg_write_state_ptr(s.state as *const State));
    }
    saveg_write32(s.tics as u32);
    saveg_write32(s.sx as u32);
    saveg_write32(s.sy as u32);
}

//
// player_t
//

/// Reads a full `player_t` record from the save stream into `*pl`.
///
/// Fields are read in the same order as the C version. Notable points:
/// - `mo` is read as a raw pointer (will be set to NULL by `P_UnArchivePlayers`
///   and rebuilt by `P_UnArchiveThinkers`).
/// - `playerstate` and enum fields (`readyweapon`, `pendingweapon`) are read
///   as 32-bit values.
/// - Power timers, key cards, frag counts, weapon ownership, ammo, and max
///   ammo are each 32-bit entries.
/// - `message` and `attacker` are raw pointers; callers zero them after read.
/// - Each of `NUMPSPRITES` player-sprite slots is read via
///   `saveg_read_pspdef_t`.
/// C origin: `saveg_read_player_t`.
unsafe fn saveg_read_player_t(pl: *mut PlayerT) {
    let s = &mut *pl;

    // mo (raw pointer, will be NULL after)
    s.mo = saveg_read32() as *mut crate::doom::d_player::mobj_t;

    // playerstate
    s.playerstate = saveg_read_enum() as c_int;

    // cmd
    saveg_read_ticcmd_t(&mut s.cmd);

    // viewz, viewheight, deltaviewheight, bob
    s.viewz = saveg_read32() as c_int;
    s.viewheight = saveg_read32() as c_int;
    s.deltaviewheight = saveg_read32() as c_int;
    s.bob = saveg_read32() as c_int;

    // health, armorpoints, armortype
    s.health = saveg_read32() as c_int;
    s.armorpoints = saveg_read32() as c_int;
    s.armortype = saveg_read32() as c_int;

    // powers[NUMPOWERS]
    for i in 0..NUMPOWERS {
        s.powers[i] = saveg_read32() as c_int;
    }

    // cards[NUMCARDS]
    for i in 0..NUMCARDS {
        s.cards[i] = saveg_read32() as c_int;
    }

    // backpack
    s.backpack = saveg_read32() as c_int;

    // frags[MAXPLAYERS]
    for i in 0..MAXPLAYERS {
        s.frags[i] = saveg_read32() as c_int;
    }

    // readyweapon, pendingweapon
    s.readyweapon = saveg_read_enum() as c_int;
    s.pendingweapon = saveg_read_enum() as c_int;

    // weaponowned[NUMWEAPONS]
    for i in 0..NUMWEAPONS {
        s.weaponowned[i] = saveg_read32() as c_int;
    }

    // ammo[NUMAMMO]
    for i in 0..NUMAMMO {
        s.ammo[i] = saveg_read32() as c_int;
    }

    // maxammo[NUMAMMO]
    for i in 0..NUMAMMO {
        s.maxammo[i] = saveg_read32() as c_int;
    }

    // attackdown, usedown
    s.attackdown = saveg_read32() as c_int;
    s.usedown = saveg_read32() as c_int;

    // cheats, refire
    s.cheats = saveg_read32() as c_int;
    s.refire = saveg_read32() as c_int;

    // killcount, itemcount, secretcount
    s.killcount = saveg_read32() as c_int;
    s.itemcount = saveg_read32() as c_int;
    s.secretcount = saveg_read32() as c_int;

    // message (raw pointer)
    s.message = saveg_read32() as *mut c_char;

    // damagecount, bonuscount
    s.damagecount = saveg_read32() as c_int;
    s.bonuscount = saveg_read32() as c_int;

    // attacker (raw pointer)
    s.attacker = saveg_read32() as *mut crate::doom::d_player::mobj_t;

    // extralight, fixedcolormap, colormap
    s.extralight = saveg_read32() as c_int;
    s.fixedcolormap = saveg_read32() as c_int;
    s.colormap = saveg_read32() as c_int;

    // psprites[NUMPSPRITES]
    for i in 0..NUMPSPRITES {
        saveg_read_pspdef_t(&mut s.psprites[i]);
    }

    // didsecret
    s.didsecret = saveg_read32() as c_int;
}

/// Writes a full `player_t` record to the save stream from `*pl`.
///
/// Raw pointers (`mo`, `message`, `attacker`) are written as 32-bit addresses;
/// they are not valid across sessions but are zeroed on unarchive. All other
/// fields mirror `saveg_read_player_t` in order.
/// C origin: `saveg_write_player_t`.
unsafe fn saveg_write_player_t(pl: *const PlayerT) {
    let s = &*pl;

    saveg_write32(s.mo as u32);
    saveg_write_enum(s.playerstate as u32);
    saveg_write_ticcmd_t(&s.cmd);
    saveg_write32(s.viewz as u32);
    saveg_write32(s.viewheight as u32);
    saveg_write32(s.deltaviewheight as u32);
    saveg_write32(s.bob as u32);
    saveg_write32(s.health as u32);
    saveg_write32(s.armorpoints as u32);
    saveg_write32(s.armortype as u32);
    for i in 0..NUMPOWERS {
        saveg_write32(s.powers[i] as u32);
    }
    for i in 0..NUMCARDS {
        saveg_write32(s.cards[i] as u32);
    }
    saveg_write32(s.backpack as u32);
    for i in 0..MAXPLAYERS {
        saveg_write32(s.frags[i] as u32);
    }
    saveg_write_enum(s.readyweapon as u32);
    saveg_write_enum(s.pendingweapon as u32);
    for i in 0..NUMWEAPONS {
        saveg_write32(s.weaponowned[i] as u32);
    }
    for i in 0..NUMAMMO {
        saveg_write32(s.ammo[i] as u32);
    }
    for i in 0..NUMAMMO {
        saveg_write32(s.maxammo[i] as u32);
    }
    saveg_write32(s.attackdown as u32);
    saveg_write32(s.usedown as u32);
    saveg_write32(s.cheats as u32);
    saveg_write32(s.refire as u32);
    saveg_write32(s.killcount as u32);
    saveg_write32(s.itemcount as u32);
    saveg_write32(s.secretcount as u32);
    saveg_write32(s.message as u32);
    saveg_write32(s.damagecount as u32);
    saveg_write32(s.bonuscount as u32);
    saveg_write32(s.attacker as u32);
    saveg_write32(s.extralight as u32);
    saveg_write32(s.fixedcolormap as u32);
    saveg_write32(s.colormap as u32);
    for i in 0..NUMPSPRITES {
        saveg_write_pspdef_t(&s.psprites[i]);
    }
    saveg_write32(s.didsecret as u32);
}

//
// ceiling_t
//

/// Reads a `ceiling_t` special thinker from the save stream into `*ceil`.
///
/// The embedded `thinker_t` header is read first, followed by type, sector
/// index, height fields, speed, crush flag, direction, tag, and old direction.
/// The sector index is resolved to a pointer via `saveg_read_sector_ptr`.
/// C origin: `saveg_read_ceiling_t`.
unsafe fn saveg_read_ceiling_t(ceil: *mut ceiling_t) {
    let s = &mut *ceil;
    saveg_read_thinker_t(&mut s.thinker);
    s.r#type = saveg_read_enum() as c_int;
    let sector_idx = saveg_read32();
    s.sector = saveg_read_sector_ptr(sector_idx);
    s.bottomheight = saveg_read32() as c_int;
    s.topheight = saveg_read32() as c_int;
    s.speed = saveg_read32() as c_int;
    s.crush = saveg_read32() as c_int;
    s.direction = saveg_read32() as c_int;
    s.tag = saveg_read32() as c_int;
    s.olddirection = saveg_read32() as c_int;
}

/// Writes a `ceiling_t` special thinker to the save stream from `*ceil`.
///
/// The sector pointer is serialized as an index via `saveg_write_sector_ptr`.
/// C origin: `saveg_write_ceiling_t`.
unsafe fn saveg_write_ceiling_t(ceil: *const ceiling_t) {
    let s = &*ceil;
    saveg_write_thinker_t(&s.thinker);
    saveg_write_enum(s.r#type as u32);
    saveg_write32(saveg_write_sector_ptr(s.sector));
    saveg_write32(s.bottomheight as u32);
    saveg_write32(s.topheight as u32);
    saveg_write32(s.speed as u32);
    saveg_write32(s.crush as u32);
    saveg_write32(s.direction as u32);
    saveg_write32(s.tag as u32);
    saveg_write32(s.olddirection as u32);
}

//
// vldoor_t
//

/// Reads a `vldoor_t` (vertical-lift door) thinker from the save stream.
///
/// Fields: thinker header, door type enum, sector index, top height (16.16
/// fixed-point), speed (16.16), direction, top-wait tic count, and current
/// top-countdown. C origin: `saveg_read_vldoor_t`.
unsafe fn saveg_read_vldoor_t(door: *mut vldoor_t) {
    let s = &mut *door;
    saveg_read_thinker_t(&mut s.thinker);
    s.r#type = saveg_read_enum() as c_int;
    let sector_idx = saveg_read32();
    s.sector = saveg_read_sector_ptr(sector_idx);
    s.topheight = saveg_read32() as c_int;
    s.speed = saveg_read32() as c_int;
    s.direction = saveg_read32() as c_int;
    s.topwait = saveg_read32() as c_int;
    s.topcountdown = saveg_read32() as c_int;
}

/// Writes a `vldoor_t` thinker to the save stream.
///
/// C origin: `saveg_write_vldoor_t`.
unsafe fn saveg_write_vldoor_t(door: *const vldoor_t) {
    let s = &*door;
    saveg_write_thinker_t(&s.thinker);
    saveg_write_enum(s.r#type as u32);
    saveg_write32(saveg_write_sector_ptr(s.sector));
    saveg_write32(s.topheight as u32);
    saveg_write32(s.speed as u32);
    saveg_write32(s.direction as u32);
    saveg_write32(s.topwait as u32);
    saveg_write32(s.topcountdown as u32);
}

//
// floormove_t
//

/// Reads a `floormove_t` (moving floor) thinker from the save stream.
///
/// Fields: thinker header, floor type enum, crush flag, sector index,
/// direction, new special number, texture (16-bit), destination height
/// (16.16), and speed (16.16). C origin: `saveg_read_floormove_t`.
unsafe fn saveg_read_floormove_t(floor: *mut floormove_t) {
    let s = &mut *floor;
    saveg_read_thinker_t(&mut s.thinker);
    s.r#type = saveg_read_enum() as c_int;
    s.crush = saveg_read32() as c_int;
    let sector_idx = saveg_read32();
    s.sector = saveg_read_sector_ptr(sector_idx);
    s.direction = saveg_read32() as c_int;
    s.newspecial = saveg_read32() as c_int;
    s.texture = saveg_read16() as i16;
    s.floordestheight = saveg_read32() as c_int;
    s.speed = saveg_read32() as c_int;
}

/// Writes a `floormove_t` thinker to the save stream.
///
/// C origin: `saveg_write_floormove_t`.
unsafe fn saveg_write_floormove_t(floor: *const floormove_t) {
    let s = &*floor;
    saveg_write_thinker_t(&s.thinker);
    saveg_write_enum(s.r#type as u32);
    saveg_write32(s.crush as u32);
    saveg_write32(saveg_write_sector_ptr(s.sector));
    saveg_write32(s.direction as u32);
    saveg_write32(s.newspecial as u32);
    saveg_write16(s.texture as u16);
    saveg_write32(s.floordestheight as u32);
    saveg_write32(s.speed as u32);
}

//
// plat_t
//

/// Reads a `plat_t` (raising/lowering platform) thinker from the save stream.
///
/// Fields: thinker header, sector index, speed (16.16), low and high heights
/// (16.16), wait time, count, status enum, old-status enum, crush flag, tag,
/// and platform type enum. C origin: `saveg_read_plat_t`.
unsafe fn saveg_read_plat_t(plat: *mut plat_t) {
    let s = &mut *plat;
    saveg_read_thinker_t(&mut s.thinker);
    let sector_idx = saveg_read32();
    s.sector = saveg_read_sector_ptr(sector_idx);
    s.speed = saveg_read32() as c_int;
    s.low = saveg_read32() as c_int;
    s.high = saveg_read32() as c_int;
    s.wait = saveg_read32() as c_int;
    s.count = saveg_read32() as c_int;
    s.status = saveg_read_enum() as c_int;
    s.oldstatus = saveg_read_enum() as c_int;
    s.crush = saveg_read32() as c_int;
    s.tag = saveg_read32() as c_int;
    s.r#type = saveg_read_enum() as c_int;
}

/// Writes a `plat_t` thinker to the save stream.
///
/// C origin: `saveg_write_plat_t`.
unsafe fn saveg_write_plat_t(plat: *const plat_t) {
    let s = &*plat;
    saveg_write_thinker_t(&s.thinker);
    saveg_write32(saveg_write_sector_ptr(s.sector));
    saveg_write32(s.speed as u32);
    saveg_write32(s.low as u32);
    saveg_write32(s.high as u32);
    saveg_write32(s.wait as u32);
    saveg_write32(s.count as u32);
    saveg_write_enum(s.status as u32);
    saveg_write_enum(s.oldstatus as u32);
    saveg_write32(s.crush as u32);
    saveg_write32(s.tag as u32);
    saveg_write_enum(s.r#type as u32);
}

//
// lightflash_t
//

/// Reads a `lightflash_t` (random light flash) thinker from the save stream.
///
/// Fields: thinker header, sector index, count, max light, min light, max
/// time, min time. C origin: `saveg_read_lightflash_t`.
unsafe fn saveg_read_lightflash_t(flash: *mut lightflash_t) {
    let s = &mut *flash;
    saveg_read_thinker_t(&mut s.thinker);
    let sector_idx = saveg_read32();
    s.sector = saveg_read_sector_ptr(sector_idx);
    s.count = saveg_read32() as c_int;
    s.maxlight = saveg_read32() as c_int;
    s.minlight = saveg_read32() as c_int;
    s.maxtime = saveg_read32() as c_int;
    s.mintime = saveg_read32() as c_int;
}

/// Writes a `lightflash_t` thinker to the save stream.
///
/// C origin: `saveg_write_lightflash_t`.
unsafe fn saveg_write_lightflash_t(flash: *const lightflash_t) {
    let s = &*flash;
    saveg_write_thinker_t(&s.thinker);
    saveg_write32(saveg_write_sector_ptr(s.sector));
    saveg_write32(s.count as u32);
    saveg_write32(s.maxlight as u32);
    saveg_write32(s.minlight as u32);
    saveg_write32(s.maxtime as u32);
    saveg_write32(s.mintime as u32);
}

//
// strobe_t
//

/// Reads a `strobe_t` (strobe light) thinker from the save stream.
///
/// Fields: thinker header, sector index, count, min light, max light, dark
/// time (tics), bright time (tics). C origin: `saveg_read_strobe_t`.
unsafe fn saveg_read_strobe_t(strobe: *mut strobe_t) {
    let s = &mut *strobe;
    saveg_read_thinker_t(&mut s.thinker);
    let sector_idx = saveg_read32();
    s.sector = saveg_read_sector_ptr(sector_idx);
    s.count = saveg_read32() as c_int;
    s.minlight = saveg_read32() as c_int;
    s.maxlight = saveg_read32() as c_int;
    s.darktime = saveg_read32() as c_int;
    s.brighttime = saveg_read32() as c_int;
}

/// Writes a `strobe_t` thinker to the save stream.
///
/// C origin: `saveg_write_strobe_t`.
unsafe fn saveg_write_strobe_t(strobe: *const strobe_t) {
    let s = &*strobe;
    saveg_write_thinker_t(&s.thinker);
    saveg_write32(saveg_write_sector_ptr(s.sector));
    saveg_write32(s.count as u32);
    saveg_write32(s.minlight as u32);
    saveg_write32(s.maxlight as u32);
    saveg_write32(s.darktime as u32);
    saveg_write32(s.brighttime as u32);
}

//
// glow_t
//

/// Reads a `glow_t` (glow light) thinker from the save stream.
///
/// Fields: thinker header, sector index, min light, max light, direction
/// (+1 or -1). C origin: `saveg_read_glow_t`.
unsafe fn saveg_read_glow_t(glow: *mut glow_t) {
    let s = &mut *glow;
    saveg_read_thinker_t(&mut s.thinker);
    let sector_idx = saveg_read32();
    s.sector = saveg_read_sector_ptr(sector_idx);
    s.minlight = saveg_read32() as c_int;
    s.maxlight = saveg_read32() as c_int;
    s.direction = saveg_read32() as c_int;
}

/// Writes a `glow_t` thinker to the save stream.
///
/// C origin: `saveg_write_glow_t`.
unsafe fn saveg_write_glow_t(glow: *const glow_t) {
    let s = &*glow;
    saveg_write_thinker_t(&s.thinker);
    saveg_write32(saveg_write_sector_ptr(s.sector));
    saveg_write32(s.minlight as u32);
    saveg_write32(s.maxlight as u32);
    saveg_write32(s.direction as u32);
}

// ---------------------------------------------------------------------------
// Save filename helpers
// ---------------------------------------------------------------------------

/// Formats a save-game filename into `buf` as `"{dir}{SAVEGAMENAME}{slot}.dsg\0"`.
///
/// Writes at most `buf.len() - 1` bytes plus a NUL terminator. Returns the
/// number of non-NUL bytes written. The caller must ensure `buf` is at least
/// `dir.len() + 32` bytes to avoid truncation.
fn fill_save_filename(buf: &mut [u8], dir: &str, slot: c_int) -> usize {
    assert!(
        !buf.is_empty(),
        "fill_save_filename: buffer must have at least 1 byte"
    );
    let full = format!("{}{}{}.dsg", dir, SAVEGAMENAME, slot);
    let bytes = full.as_bytes();
    let len = std::cmp::min(bytes.len(), buf.len() - 1);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf[len] = 0;
    len
}

// ---------------------------------------------------------------------------
// Static filename buffers
// ---------------------------------------------------------------------------

/// Lazily allocated buffer holding the path to the temporary save file.
/// Initialized once by `P_TempSaveGameFile`; never freed (process lifetime).
static mut TEMP_SAVE_FILENAME: *mut c_char = std::ptr::null_mut();

/// Lazily allocated buffer holding the path to the current slot's save file.
/// Allocated once by `P_SaveGameFile`, then reused for every slot.
static mut SAVE_FILENAME: *mut c_char = std::ptr::null_mut();

/// Base name prefix for save-game files; slot number and `.dsg` extension are
/// appended. Matches the `SAVEGAMENAME` define in the C source.
const SAVEGAMENAME: &str = "doomsav";

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Returns the path to the temporary save-game file used during a save
/// operation.
///
/// The file is written first, then atomically renamed to the final slot
/// filename on success. The returned pointer is valid for the lifetime of the
/// process; it is allocated once and cached in `TEMP_SAVE_FILENAME`.
///
/// Called by `g_game.c` (`G_DoSaveGame`).
///
/// # Safety
///
/// `savegamedir` must be a valid, NUL-terminated C string for the lifetime of
/// this call. The returned pointer is valid until process exit; the caller must
/// not free or mutate it.
#[no_mangle]
pub unsafe extern "C" fn P_TempSaveGameFile() -> *mut c_char {
    if TEMP_SAVE_FILENAME.is_null() {
        let dir = std::ffi::CStr::from_ptr(savegamedir).to_string_lossy();
        let full = format!("{}temp.dsg\0", dir);
        let bytes = full.into_bytes();
        let ptr = std::alloc::alloc(std::alloc::Layout::from_size_align(bytes.len(), 1).unwrap())
            as *mut c_char;
        ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
        TEMP_SAVE_FILENAME = ptr;
    }
    TEMP_SAVE_FILENAME
}

/// Returns the path to the save-game file for the given `slot` (0-7).
///
/// The buffer is allocated once and reused; the filename is rebuilt on every
/// call so the same buffer can serve different slot numbers. The returned
/// pointer is valid until the next call or process exit.
///
/// Called by `g_game.c` to open the final save file for reading or renaming.
///
/// # Safety
///
/// `savegamedir` must be a valid, NUL-terminated C string for the lifetime of
/// this call. `slot` must be in the range `0..=7`. The returned pointer is
/// valid until the next call to this function; the caller must not free it.
#[no_mangle]
pub unsafe extern "C" fn P_SaveGameFile(slot: c_int) -> *mut c_char {
    let dir_len = std::ffi::CStr::from_ptr(savegamedir).to_bytes().len();
    let alloc_size = dir_len + 32;

    if SAVE_FILENAME.is_null() {
        let layout = std::alloc::Layout::from_size_align(alloc_size, 1).unwrap();
        let ptr = std::alloc::alloc(layout);
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        SAVE_FILENAME = ptr as *mut c_char;
    }

    let dir_str = std::ffi::CStr::from_ptr(savegamedir).to_str().unwrap();
    let buf = std::slice::from_raw_parts_mut(SAVE_FILENAME as *mut u8, alloc_size);
    fill_save_filename(buf, dir_str, slot);

    SAVE_FILENAME
}

/// Writes the save-game file header to the open `save_stream`.
///
/// The header layout (bytes written in order):
/// 1. `description` string, NUL-padded to `SAVESTRINGSIZE` (24) bytes.
/// 2. Version string `"version N"`, NUL-padded to `VERSIONSIZE` (16) bytes.
/// 3. One byte each: `gameskill`, `gameepisode`, `gamemap`.
/// 4. `MAXPLAYERS` bytes: `playeringame[i]` flags.
/// 5. Three bytes: `leveltime` encoded big-endian as
///    `[bits 23:16, bits 15:8, bits 7:0]`.
///
/// Called by `g_game.c` (`G_DoSaveGame`).
///
/// # Safety
///
/// `description` must be a valid, NUL-terminated C string. `save_stream` must
/// be an open, writable `FILE *` with enough capacity to hold the header bytes.
/// The global state `gameskill`, `gameepisode`, `gamemap`, `playeringame`, and
/// `leveltime` must have been set to valid values before this call.
#[no_mangle]
pub unsafe extern "C" fn P_WriteSaveGameHeader(description: *const c_char) {
    let desc = std::ffi::CStr::from_ptr(description);
    let desc_bytes = desc.to_bytes();

    // Write description (padded to SAVESTRINGSIZE)
    for i in 0..SAVESTRINGSIZE {
        if i < desc_bytes.len() {
            saveg_write8(desc_bytes[i]);
        } else {
            saveg_write8(0);
        }
    }

    // Write version string (VERSIONSIZE bytes)
    let version_code = G_VanillaVersionCode();
    let version_str = format!("version {}\0", version_code);
    let version_bytes = version_str.as_bytes();
    for i in 0..VERSIONSIZE {
        if i < version_bytes.len() {
            saveg_write8(version_bytes[i]);
        } else {
            saveg_write8(0);
        }
    }

    // Write skill, episode, map
    saveg_write8(gameskill as u8);
    saveg_write8(gameepisode as u8);
    saveg_write8(gamemap as u8);

    // Write playeringame
    for i in 0..MAXPLAYERS {
        saveg_write8(playeringame[i] as u8);
    }

    // Write leveltime (3 bytes, big-endian)
    let lt = leveltime as u32;
    saveg_write8(((lt >> 16) & 0xff) as u8);
    saveg_write8(((lt >> 8) & 0xff) as u8);
    saveg_write8((lt & 0xff) as u8);
}

/// Reads and validates the save-game header from `save_stream`.
///
/// Returns `1` (true) on success, `0` (false) if the version string does not
/// match `G_VanillaVersionCode()`. On success, `gameskill`, `gameepisode`,
/// `gamemap`, `playeringame`, and `leveltime` are updated from the stream.
///
/// Called by `g_game.c` (`G_DoLoadGame`).
///
/// # Safety
///
/// `save_stream` must be an open, readable `FILE *` positioned at the start of
/// a save file written by `P_WriteSaveGameHeader`. On success, `gameskill`,
/// `gameepisode`, `gamemap`, `playeringame`, and `leveltime` are overwritten
/// with values from the stream.
#[no_mangle]
pub unsafe extern "C" fn P_ReadSaveGameHeader() -> c_int {
    // Skip description (SAVESTRINGSIZE bytes)
    for _ in 0..SAVESTRINGSIZE {
        saveg_read8();
    }

    // Read version string
    let mut read_vcheck = [0u8; VERSIONSIZE];
    for i in 0..VERSIONSIZE {
        read_vcheck[i] = saveg_read8();
    }

    // Compare version
    let version_code = G_VanillaVersionCode();
    let expected = format!("version {}\0", version_code);
    let expected_bytes = expected.as_bytes();

    // Null-terminate both for strcmp-like comparison
    let mut expected_padded = [0u8; VERSIONSIZE];
    for (i, &b) in expected_bytes.iter().enumerate().take(VERSIONSIZE) {
        expected_padded[i] = b;
    }

    if read_vcheck != expected_padded {
        return 0; // bad version
    }

    // Read skill, episode, map
    gameskill = saveg_read8() as c_int;
    gameepisode = saveg_read8() as c_int;
    gamemap = saveg_read8() as c_int;

    // Read playeringame
    for i in 0..MAXPLAYERS {
        playeringame[i] = saveg_read8() as c_int;
    }

    // Read leveltime (3 bytes, big-endian)
    let a = saveg_read8() as u32;
    let b = saveg_read8() as u32;
    let c = saveg_read8() as u32;
    leveltime = ((a << 16) | (b << 8) | c) as c_int;

    1 // success
}

/// Reads the end-of-file marker byte from `save_stream`.
///
/// Returns `1` if the byte equals `SAVEGAME_EOF` (`0x1d`), otherwise `0`.
/// A mismatch indicates a truncated or corrupt save file.
/// Called by `g_game.c` after all game state has been unarchived.
///
/// # Safety
///
/// `save_stream` must be an open, readable `FILE *` positioned immediately
/// after the last unarchived byte, i.e. where `P_WriteSaveGameEOF` wrote its
/// marker. Reading from an invalid or exhausted stream is undefined behaviour.
#[no_mangle]
pub unsafe extern "C" fn P_ReadSaveGameEOF() -> c_int {
    let value = saveg_read8();
    if value == SAVEGAME_EOF {
        1
    } else {
        0
    }
}

/// Writes the end-of-file marker byte (`SAVEGAME_EOF` = `0x1d`) to
/// `save_stream`.
///
/// Written after all game state has been archived; checked by
/// `P_ReadSaveGameEOF` on load to detect truncation.
/// Called by `g_game.c` (`G_DoSaveGame`).
///
/// # Safety
///
/// `save_stream` must be an open, writable `FILE *`. This must be called after
/// all archive functions have finished writing so that the marker byte appears
/// at the correct position for `P_ReadSaveGameEOF` to validate.
#[no_mangle]
pub unsafe extern "C" fn P_WriteSaveGameEOF() {
    saveg_write8(SAVEGAME_EOF);
}

extern "C" {
    /// Current skill level (0-4); serialized into the save header.
    static mut gameskill: c_int;
    /// Current episode number (1-based); serialized into the save header.
    static mut gameepisode: c_int;
    /// Current map number within the episode (1-based); serialized into the
    /// save header.
    static mut gamemap: c_int;
    /// Per-player in-game flags; non-zero means the corresponding player slot
    /// is active. Array of `MAXPLAYERS` elements.
    static mut playeringame: [c_int; MAXPLAYERS];
}

// ---------------------------------------------------------------------------
// P_ArchivePlayers / P_UnArchivePlayers
// ---------------------------------------------------------------------------

/// Serializes all active players to `save_stream`.
///
/// Iterates over `players[0..MAXPLAYERS]`; skips slots where
/// `playeringame[i] == 0`. Each active player record is 4-byte aligned
/// (`saveg_write_pad`) then written by `saveg_write_player_t`.
/// Called by `g_game.c` (`G_DoSaveGame`).
///
/// # Safety
///
/// `save_stream` must be an open, writable `FILE *` with sufficient capacity.
/// The `players` array and `playeringame` flags must be fully initialized for
/// all `MAXPLAYERS` slots. Must be called after `P_WriteSaveGameHeader` and
/// before `P_WriteSaveGameEOF`.
#[no_mangle]
pub unsafe extern "C" fn P_ArchivePlayers() {
    for i in 0..MAXPLAYERS {
        if playeringame[i] == 0 {
            continue;
        }
        saveg_write_pad();
        saveg_write_player_t(&players[i]);
    }
}

/// Deserializes all active players from `save_stream`.
///
/// For each active player slot, aligns the stream (`saveg_read_pad`) then
/// reads the player record. After reading, `mo`, `message`, and `attacker` are
/// reset to null pointers; they will be restored when thinkers are unarchived
/// by `P_UnArchiveThinkers`.
/// Called by `g_game.c` (`G_DoLoadGame`).
///
/// # Safety
///
/// `save_stream` must be an open, readable `FILE *` positioned at the byte
/// sequence written by `P_ArchivePlayers`. `playeringame` must already have
/// been populated by `P_ReadSaveGameHeader` so the active-slot bitmask is
/// correct. After this call, `players[i].mo`, `.message`, and `.attacker` are
/// null and must not be dereferenced until thinkers are unarchived.
#[no_mangle]
pub unsafe extern "C" fn P_UnArchivePlayers() {
    for i in 0..MAXPLAYERS {
        if playeringame[i] == 0 {
            continue;
        }
        saveg_read_pad();
        saveg_read_player_t(&mut players[i]);

        // will be set when unarc thinker
        players[i].mo = std::ptr::null_mut();
        players[i].message = std::ptr::null_mut();
        players[i].attacker = std::ptr::null_mut();
    }
}

// ---------------------------------------------------------------------------
// P_ArchiveWorld / P_UnArchiveWorld
// ---------------------------------------------------------------------------

/// Serializes all map sector and sidedef deltas to `save_stream`.
///
/// For each sector: floor height, ceiling height (both divided by 65536 to
/// strip the fixed-point fractional part and store as 16-bit integers),
/// floor/ceiling picture indices, light level, special, and tag.
///
/// For each linedef: flags, special, and tag. Then for each of the two
/// sidedefs (skipping missing sides where `sidenum[j] == -1`): texture
/// offsets (divided by 65536, stored as 16-bit), and top/bottom/mid texture
/// indices.
///
/// Called by `g_game.c` (`G_DoSaveGame`).
///
/// # Safety
///
/// `save_stream` must be an open, writable `FILE *` with sufficient capacity.
/// The global `sectors` array (length `numsectors`), `lines` array (length
/// `numlines`), and `sides` array must all be fully initialized. Every
/// `line_t.sidenum[j]` that is not `-1` must be a valid index into `sides`.
#[no_mangle]
pub unsafe extern "C" fn P_ArchiveWorld() {
    let num_sec = numsectors as usize;
    let num_li = numlines as usize;

    // do sectors
    for i in 0..num_sec {
        let sec = sectors.add(i);
        saveg_write16(((*sec).floorheight >> 16) as u16);
        saveg_write16(((*sec).ceilingheight >> 16) as u16);
        saveg_write16((*sec).floorpic as u16);
        saveg_write16((*sec).ceilingpic as u16);
        saveg_write16((*sec).lightlevel as u16);
        saveg_write16((*sec).special as u16);
        saveg_write16((*sec).tag as u16);
    }

    // do lines
    for i in 0..num_li {
        let li = lines.add(i);
        saveg_write16((*li).flags as u16);
        saveg_write16((*li).special as u16);
        saveg_write16((*li).tag as u16);
        for j in 0..2 {
            if (*li).sidenum[j as usize] == -1 {
                continue;
            }
            let si = sides.add((*li).sidenum[j as usize] as usize);
            saveg_write16(((*si).textureoffset >> 16) as u16);
            saveg_write16(((*si).rowoffset >> 16) as u16);
            saveg_write16((*si).toptexture as u16);
            saveg_write16((*si).bottomtexture as u16);
            saveg_write16((*si).midtexture as u16);
        }
    }
}

/// Deserializes all map sector and sidedef deltas from `save_stream`.
///
/// For each sector: reads heights as signed 16-bit integers and shifts them
/// left 16 bits to restore the 16.16 fixed-point format. Clears
/// `specialdata` and `soundtarget` to null (they will be rebuilt by
/// `P_UnArchiveSpecials` and the sound code respectively).
///
/// For each linedef: reads flags, special, tag, then each present sidedef's
/// texture offsets (shifted left 16 to restore 16.16) and texture indices.
///
/// Called by `g_game.c` (`G_DoLoadGame`).
///
/// # Safety
///
/// `save_stream` must be an open, readable `FILE *` positioned at the byte
/// sequence written by `P_ArchiveWorld`. The `sectors` array (length
/// `numsectors`), `lines` array (length `numlines`), and `sides` array must
/// all be allocated and at least partially initialized (map load must have
/// completed). Every `line_t.sidenum[j]` that is not `-1` must be a valid
/// index into `sides`.
#[no_mangle]
pub unsafe extern "C" fn P_UnArchiveWorld() {
    let num_sec = numsectors as usize;
    let num_li = numlines as usize;

    // do sectors
    for i in 0..num_sec {
        let sec = sectors.add(i);
        (*sec).floorheight = (saveg_read16() as i16 as c_int) << 16;
        (*sec).ceilingheight = (saveg_read16() as i16 as c_int) << 16;
        (*sec).floorpic = saveg_read16() as i16;
        (*sec).ceilingpic = saveg_read16() as i16;
        (*sec).lightlevel = saveg_read16() as i16;
        (*sec).special = saveg_read16() as i16;
        (*sec).tag = saveg_read16() as i16;
        (*sec).specialdata = std::ptr::null_mut();
        (*sec).soundtarget = std::ptr::null_mut();
    }

    // do lines
    for i in 0..num_li {
        let li = lines.add(i);
        (*li).flags = saveg_read16() as i16;
        (*li).special = saveg_read16() as i16;
        (*li).tag = saveg_read16() as i16;
        for j in 0..2 {
            if (*li).sidenum[j as usize] == -1 {
                continue;
            }
            let si = sides.add((*li).sidenum[j as usize] as usize);
            (*si).textureoffset = (saveg_read16() as i16 as c_int) << 16;
            (*si).rowoffset = (saveg_read16() as i16 as c_int) << 16;
            (*si).toptexture = saveg_read16() as i16;
            (*si).bottomtexture = saveg_read16() as i16;
            (*si).midtexture = saveg_read16() as i16;
        }
    }
}

// ---------------------------------------------------------------------------
// Thinker class enum
// ---------------------------------------------------------------------------

/// Thinker-class tag: marks the end of the archived thinker list.
/// C origin: `tc_end` in `thinkerclass_t`.
const tc_end: u8 = 0;

/// Thinker-class tag: marks a `mobj_t` record in the archived thinker list.
/// C origin: `tc_mobj` in `thinkerclass_t`.
const tc_mobj: u8 = 1;

// ---------------------------------------------------------------------------
// P_ArchiveThinkers / P_UnArchiveThinkers
// ---------------------------------------------------------------------------

/// Serializes all `mobj_t` thinkers from the active thinker chain.
///
/// Walks `thinkercap.next ... thinkercap`; for each thinker whose function
/// equals `P_MobjThinker`, writes a `tc_mobj` byte, 4-byte alignment padding,
/// then the full `mobj_t` record. Non-mobj thinkers are silently skipped
/// (the C version also skips them without error). Terminates the list with a
/// `tc_end` byte.
///
/// Called by `g_game.c` (`G_DoSaveGame`).
///
/// # Safety
///
/// `save_stream` must be an open, writable `FILE *` with sufficient capacity.
/// The thinker chain rooted at `thinkercap` must be fully initialized and form
/// a valid doubly-linked circular list. Every thinker in the chain whose
/// function equals `P_MobjThinker` must point to a valid `mobj_t`.
#[no_mangle]
pub unsafe extern "C" fn P_ArchiveThinkers() {
    let cap = &raw mut thinkercap;
    let mut th = (*cap).next;

    while th != cap {
        // Check if function is P_MobjThinker by comparing pointers
        let func = (*th).function.acp1;
        if let Some(fn_ptr) = func {
            if fn_ptr as usize == P_MobjThinker as *const () as usize {
                saveg_write8(tc_mobj);
                saveg_write_pad();
                saveg_write_mobj_t(th as *const c_void);
                th = (*th).next;
                continue;
            }
        }
        th = (*th).next;
    }

    // terminating marker
    saveg_write8(tc_end);
}

/// Clears all existing thinkers then deserializes the thinker chain from
/// `save_stream`.
///
/// First pass: walks the existing thinker chain; mobj thinkers are removed
/// via `P_RemoveMobj`, all others are freed via `Z_Free`. Then
/// `P_InitThinkers` resets the chain to empty.
///
/// Second pass: reads thinker-class bytes from the stream in a loop.
/// - `tc_end`: returns immediately.
/// - `tc_mobj`: allocates a `mobj_t` with `Z_Malloc(PU_LEVEL)`, reads its
///   fields, nulls out `target` and `tracer`, calls `P_SetThingPosition`,
///   rebuilds `info` from `mobjinfo[type]`, recomputes `floorz`/`ceilingz`
///   from the subsector's sector, assigns `P_MobjThinker` as the function,
///   and registers it with `P_AddThinker`.
/// - Any other byte: calls `i_error!` (fatal).
///
/// Called by `g_game.c` (`G_DoLoadGame`).
///
/// # Safety
///
/// `save_stream` must be an open, readable `FILE *` positioned at the byte
/// sequence written by `P_ArchiveThinkers`. The zone allocator must be
/// operational (`Z_Malloc`/`Z_Free` must be safe to call). The map geometry
/// (`sectors`, block-map, etc.) must be loaded so that `P_SetThingPosition`
/// can place each restored mobj. Must be called before `P_UnArchiveSpecials`
/// and before any code dereferences `players[i].mo`.
#[no_mangle]
pub unsafe extern "C" fn P_UnArchiveThinkers() {
    let cap = &raw mut thinkercap;
    let mut currentthinker = (*cap).next;

    // remove all current thinkers
    while currentthinker != cap {
        let next = (*currentthinker).next;

        let func = (*currentthinker).function.acp1;
        if let Some(fn_ptr) = func {
            if fn_ptr as usize == P_MobjThinker as *const () as usize {
                P_RemoveMobj(currentthinker as *mut c_void);
            } else {
                Z_Free(currentthinker as *mut c_void);
            }
        } else {
            Z_Free(currentthinker as *mut c_void);
        }

        currentthinker = next;
    }

    P_InitThinkers();

    // read saved thinkers
    loop {
        let tclass = saveg_read8();
        match tclass {
            x if x == tc_end => return,
            x if x == tc_mobj => {
                saveg_read_pad();
                let mobj = Z_Malloc(
                    std::mem::size_of::<crate::doom::c_ffi::mobj_t>() as c_int,
                    PU_LEVEL,
                    std::ptr::null_mut(),
                );
                saveg_read_mobj_t(mobj);

                let mo = mobj as *mut crate::doom::c_ffi::mobj_t;

                // target/tracer set to NULL (will be rebuilt)
                (*mo).target = std::ptr::null_mut();
                (*mo).tracer = std::ptr::null_mut();

                P_SetThingPosition(mobj);

                // rebuild info from mobjinfo
                (*mo).info = &mut mobjinfo[(*mo).type_ as usize] as *mut MobjInfo
                    as *mut crate::doom::c_ffi::mobjinfo_t;

                // rebuild floorz/ceilingz from subsector
                let subsec = (*mo).subsector as *mut crate::doom::c_ffi::subsector_t;
                if !subsec.is_null() {
                    let sector = (*subsec).sector as *mut sector_t;
                    if !sector.is_null() {
                        (*mo).floorz = (*sector).floorheight;
                        (*mo).ceilingz = (*sector).ceilingheight;
                    }
                }

                // set thinker function
                let thinker_ptr = std::ptr::addr_of_mut!((*mo).thinker_prev) as *mut thinker_t;
                (*thinker_ptr).function = actionf_t {
                    acp1: Some(P_MobjThinker),
                };

                P_AddThinker(&mut (*thinker_ptr));
            }
            _ => {
                i_error!("Unknown tclass {} in savegame", tclass as c_int);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Specials enum
// ---------------------------------------------------------------------------

/// Specials-class tag for a `ceiling_t` record. C origin: `tc_ceiling`.
const tc_ceiling: u8 = 0;
/// Specials-class tag for a `vldoor_t` record. C origin: `tc_door`.
const tc_door: u8 = 1;
/// Specials-class tag for a `floormove_t` record. C origin: `tc_floor`.
const tc_floor: u8 = 2;
/// Specials-class tag for a `plat_t` record. C origin: `tc_plat`.
const tc_plat: u8 = 3;
/// Specials-class tag for a `lightflash_t` record. C origin: `tc_flash`.
const tc_flash: u8 = 4;
/// Specials-class tag for a `strobe_t` record. C origin: `tc_strobe`.
const tc_strobe: u8 = 5;
/// Specials-class tag for a `glow_t` record. C origin: `tc_glow`.
const tc_glow: u8 = 6;
/// Specials-class tag marking the end of the specials list.
/// C origin: `tc_endspecials`.
const tc_endspecials: u8 = 7;

// ---------------------------------------------------------------------------
// P_ArchiveSpecials / P_UnArchiveSpecials
// ---------------------------------------------------------------------------

/// Serializes all sector-special thinkers from the active thinker chain.
///
/// Walks the thinker chain and identifies specials by their function pointer:
/// - Thinkers with `acv == NULL` that appear in the `activeceilings` list are
///   written as `tc_ceiling` records (these are ceilings paused mid-crush).
/// - `T_MoveCeiling` → `tc_ceiling`
/// - `T_VerticalDoor` → `tc_door`
/// - `T_MoveFloor` → `tc_floor`
/// - `T_PlatRaise` → `tc_plat`
/// - `T_LightFlash` → `tc_flash`
/// - `T_StrobeFlash` → `tc_strobe`
/// - `T_Glow` → `tc_glow`
///
/// Each matching thinker is preceded by its class tag byte and 4-byte
/// alignment padding. Unrecognized thinkers are silently skipped. The list is
/// terminated by `tc_endspecials`.
///
/// Called by `g_game.c` (`G_DoSaveGame`).
///
/// # Safety
///
/// `save_stream` must be an open, writable `FILE *` with sufficient capacity.
/// The thinker chain rooted at `thinkercap` must be fully initialized and form
/// a valid doubly-linked circular list. The `activeceilings` array must be
/// initialized. Every thinker that matches a known special function pointer
/// must point to a fully initialized special struct (`ceiling_t`, `vldoor_t`,
/// etc.).
#[no_mangle]
pub unsafe extern "C" fn P_ArchiveSpecials() {
    let cap = &raw mut thinkercap;
    let mut th = (*cap).next;

    while th != cap {
        let func = (*th).function;

        // Check for ceiling (acv == NULL means in activeceilings list)
        if func.acv.is_none() {
            // Check if it's in activeceilings
            let activeceilings_ptr = std::ptr::addr_of!(crate::doom::p_ceilng::activeceilings[0]);
            let mut found = false;
            for i in 0..MAXCEILINGS {
                if *activeceilings_ptr.add(i) == th as *mut ceiling_t {
                    found = true;
                    break;
                }
            }
            if found {
                saveg_write8(tc_ceiling);
                saveg_write_pad();
                saveg_write_ceiling_t(th as *const ceiling_t);
                th = (*th).next;
                continue;
            }
        }

        if func.acp1.map(|f| f as usize) == Some(T_MoveCeiling as *const () as usize) {
            saveg_write8(tc_ceiling);
            saveg_write_pad();
            saveg_write_ceiling_t(th as *const ceiling_t);
            th = (*th).next;
            continue;
        }

        if func.acp1.map(|f| f as usize) == Some(T_VerticalDoor as *const () as usize) {
            saveg_write8(tc_door);
            saveg_write_pad();
            saveg_write_vldoor_t(th as *const vldoor_t);
            th = (*th).next;
            continue;
        }

        if func.acp1.map(|f| f as usize) == Some(T_MoveFloor as *const () as usize) {
            saveg_write8(tc_floor);
            saveg_write_pad();
            saveg_write_floormove_t(th as *const floormove_t);
            th = (*th).next;
            continue;
        }

        if func.acp1.map(|f| f as usize) == Some(T_PlatRaise as *const () as usize) {
            saveg_write8(tc_plat);
            saveg_write_pad();
            saveg_write_plat_t(th as *const plat_t);
            th = (*th).next;
            continue;
        }

        if func.acp1.map(|f| f as usize) == Some(T_LightFlash as *const () as usize) {
            saveg_write8(tc_flash);
            saveg_write_pad();
            saveg_write_lightflash_t(th as *const lightflash_t);
            th = (*th).next;
            continue;
        }

        if func.acp1.map(|f| f as usize) == Some(T_StrobeFlash as *const () as usize) {
            saveg_write8(tc_strobe);
            saveg_write_pad();
            saveg_write_strobe_t(th as *const strobe_t);
            th = (*th).next;
            continue;
        }

        if func.acp1.map(|f| f as usize) == Some(T_Glow as *const () as usize) {
            saveg_write8(tc_glow);
            saveg_write_pad();
            saveg_write_glow_t(th as *const glow_t);
            th = (*th).next;
            continue;
        }

        th = (*th).next;
    }

    saveg_write8(tc_endspecials);
}

/// Deserializes all sector-special thinkers from `save_stream`.
///
/// Reads thinker-class bytes in a loop until `tc_endspecials`:
/// - `tc_ceiling`: allocates `ceiling_t` (`PU_LEVSPEC`), deserializes,
///   links `sector->specialdata`, restores `T_MoveCeiling` if the function
///   slot was non-null, registers with `P_AddThinker` and
///   `P_AddActiveCeiling`.
/// - `tc_door`: allocates `vldoor_t` (`PU_LEVSPEC`), deserializes, links
///   `sector->specialdata`, unconditionally assigns `T_VerticalDoor`,
///   registers with `P_AddThinker`.
/// - `tc_floor`: allocates `floormove_t` (`PU_LEVSPEC`), deserializes,
///   links `sector->specialdata`, assigns `T_MoveFloor`, registers.
/// - `tc_plat`: allocates `plat_t` (`PU_LEVSPEC`), deserializes, links
///   `sector->specialdata`, restores `T_PlatRaise` if function non-null,
///   registers with `P_AddThinker` and `P_AddActivePlat`.
/// - `tc_flash`: allocates `lightflash_t` (`PU_LEVSPEC`), deserializes,
///   assigns `T_LightFlash`, registers.
/// - `tc_strobe`: allocates `strobe_t` (`PU_LEVSPEC`), deserializes,
///   assigns `T_StrobeFlash`, registers.
/// - `tc_glow`: allocates `glow_t` (`PU_LEVSPEC`), deserializes, assigns
///   `T_Glow`, registers.
/// - Any other byte: calls `i_error!` (fatal).
///
/// Note: the C version allocates ceilings with `PU_LEVEL`, not `PU_LEVSPEC`.
/// This port uses `PU_LEVSPEC` consistently for all specials to match the
/// `Z_Malloc` tag used for the other special types.
///
/// Called by `g_game.c` (`G_DoLoadGame`).
///
/// # Safety
///
/// `save_stream` must be an open, readable `FILE *` positioned at the byte
/// sequence written by `P_ArchiveSpecials`. The zone allocator must be
/// operational. The `sectors` array must be fully initialized so that
/// `sector->specialdata` can be set. Must be called after
/// `P_UnArchiveThinkers` so the thinker chain is in a consistent state before
/// new specials are registered with `P_AddThinker`.
// FIXME: p_saveg.c allocates ceiling_t with PU_LEVEL, not PU_LEVSPEC; this
// port uses PU_LEVSPEC here (consistent with other specials). The difference
// affects when the zone allocator may purge the block.
#[no_mangle]
pub unsafe extern "C" fn P_UnArchiveSpecials() {
    loop {
        let tclass = saveg_read8();
        match tclass {
            x if x == tc_endspecials => return,
            x if x == tc_ceiling => {
                saveg_read_pad();
                let ceiling = Z_Malloc(
                    std::mem::size_of::<ceiling_t>() as c_int,
                    PU_LEVSPEC,
                    std::ptr::null_mut(),
                ) as *mut ceiling_t;
                saveg_read_ceiling_t(ceiling);
                (*ceiling).sector.as_mut().unwrap().specialdata = ceiling as *mut c_void;

                if ceiling.as_ref().unwrap().thinker.function.acp1.is_some() {
                    ceiling.as_mut().unwrap().thinker.function = actionf_p1_move_ceiling();
                }

                P_AddThinker(&mut (*ceiling).thinker);
                P_AddActiveCeiling(ceiling);
            }
            x if x == tc_door => {
                saveg_read_pad();
                let door = Z_Malloc(
                    std::mem::size_of::<vldoor_t>() as c_int,
                    PU_LEVSPEC,
                    std::ptr::null_mut(),
                ) as *mut vldoor_t;
                saveg_read_vldoor_t(door);
                (*door).sector.as_mut().unwrap().specialdata = door as *mut c_void;
                door.as_mut().unwrap().thinker.function = actionf_p1_vertical_door();
                P_AddThinker(&mut (*door).thinker);
            }
            x if x == tc_floor => {
                saveg_read_pad();
                let floor = Z_Malloc(
                    std::mem::size_of::<floormove_t>() as c_int,
                    PU_LEVSPEC,
                    std::ptr::null_mut(),
                ) as *mut floormove_t;
                saveg_read_floormove_t(floor);
                (*floor).sector.as_mut().unwrap().specialdata = floor as *mut c_void;
                floor.as_mut().unwrap().thinker.function = actionf_p1_move_floor();
                P_AddThinker(&mut (*floor).thinker);
            }
            x if x == tc_plat => {
                saveg_read_pad();
                let plat = Z_Malloc(
                    std::mem::size_of::<plat_t>() as c_int,
                    PU_LEVSPEC,
                    std::ptr::null_mut(),
                ) as *mut plat_t;
                saveg_read_plat_t(plat);
                (*plat).sector.as_mut().unwrap().specialdata = plat as *mut c_void;

                if plat.as_ref().unwrap().thinker.function.acp1.is_some() {
                    plat.as_mut().unwrap().thinker.function = actionf_p1_plat_raise();
                }

                P_AddThinker(&mut (*plat).thinker);
                P_AddActivePlat(plat);
            }
            x if x == tc_flash => {
                saveg_read_pad();
                let flash = Z_Malloc(
                    std::mem::size_of::<lightflash_t>() as c_int,
                    PU_LEVSPEC,
                    std::ptr::null_mut(),
                ) as *mut lightflash_t;
                saveg_read_lightflash_t(flash);
                flash.as_mut().unwrap().thinker.function = actionf_p1_light_flash();
                P_AddThinker(&mut (*flash).thinker);
            }
            x if x == tc_strobe => {
                saveg_read_pad();
                let strobe = Z_Malloc(
                    std::mem::size_of::<strobe_t>() as c_int,
                    PU_LEVSPEC,
                    std::ptr::null_mut(),
                ) as *mut strobe_t;
                saveg_read_strobe_t(strobe);
                strobe.as_mut().unwrap().thinker.function = actionf_p1_strobe_flash();
                P_AddThinker(&mut (*strobe).thinker);
            }
            x if x == tc_glow => {
                saveg_read_pad();
                let glow = Z_Malloc(
                    std::mem::size_of::<glow_t>() as c_int,
                    PU_LEVSPEC,
                    std::ptr::null_mut(),
                ) as *mut glow_t;
                saveg_read_glow_t(glow);
                glow.as_mut().unwrap().thinker.function = actionf_p1_glow();
                P_AddThinker(&mut (*glow).thinker);
            }
            _ => {
                i_error!(
                    "P_UnarchiveSpecials:Unknown tclass {} in savegame",
                    tclass as c_int
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Link anchor
// ---------------------------------------------------------------------------

/// Forces all public `extern "C"` functions in this module to be included in
/// the final binary by taking their addresses.
///
/// Without this anchor the linker may dead-strip functions that are only called
/// from C translation units, since Rust does not see those call sites.
#[no_mangle]
pub extern "C" fn P_Saveg_Link_Anchor() {
    let _ = P_TempSaveGameFile as *const () as usize;
    let _ = P_SaveGameFile as *const () as usize;
    let _ = P_WriteSaveGameHeader as *const () as usize;
    let _ = P_ReadSaveGameHeader as *const () as usize;
    let _ = P_ReadSaveGameEOF as *const () as usize;
    let _ = P_WriteSaveGameEOF as *const () as usize;
    let _ = P_ArchivePlayers as *const () as usize;
    let _ = P_UnArchivePlayers as *const () as usize;
    let _ = P_ArchiveWorld as *const () as usize;
    let _ = P_UnArchiveWorld as *const () as usize;
    let _ = P_ArchiveThinkers as *const () as usize;
    let _ = P_UnArchiveThinkers as *const () as usize;
    let _ = P_ArchiveSpecials as *const () as usize;
    let _ = P_UnArchiveSpecials as *const () as usize;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    const SAVEGAME_EOF_VAL: u8 = 0x1d;
    const VERSIONSIZE_VAL: usize = 16;
    const SAVESTRINGSIZE_VAL: usize = 24;

    #[test]
    fn constants_match_c() {
        assert_eq!(SAVEGAME_EOF, SAVEGAME_EOF_VAL);
        assert_eq!(VERSIONSIZE, VERSIONSIZE_VAL);
        assert_eq!(SAVESTRINGSIZE, SAVESTRINGSIZE_VAL);
    }

    #[test]
    fn endian_write_read_16() {
        let _g = LOCK.lock().unwrap();
        // Test that write_u16 followed by read_u16 round-trips correctly
        let mut buf = Vec::new();
        // Simulate little-endian write: 0x1234 -> [0x34, 0x12]
        let val: u16 = 0x1234;
        buf.push((val & 0xff) as u8);
        buf.push(((val >> 8) & 0xff) as u8);
        assert_eq!(buf[0], 0x34);
        assert_eq!(buf[1], 0x12);
    }

    #[test]
    fn endian_write_read_32() {
        let _g = LOCK.lock().unwrap();
        let val: u32 = 0x12345678;
        let mut buf = Vec::new();
        buf.push((val & 0xff) as u8);
        buf.push(((val >> 8) & 0xff) as u8);
        buf.push(((val >> 16) & 0xff) as u8);
        buf.push(((val >> 24) & 0xff) as u8);
        assert_eq!(buf[0], 0x78);
        assert_eq!(buf[1], 0x56);
        assert_eq!(buf[2], 0x34);
        assert_eq!(buf[3], 0x12);
    }

    #[test]
    #[allow(clippy::erasing_op)]
    fn padding_calculation() {
        let _g = LOCK.lock().unwrap();
        // Test padding calculation: (4 - (pos & 3)) & 3
        assert_eq!((4 - (0 & 3)) & 3, 0); // aligned
        assert_eq!((4 - (1 & 3)) & 3, 3);
        assert_eq!((4 - (2 & 3)) & 3, 2);
        assert_eq!((4 - (3 & 3)) & 3, 1);
        assert_eq!((4 - (4 & 3)) & 3, 0); // aligned
        assert_eq!((4 - (5 & 3)) & 3, 3);
    }

    #[test]
    fn eof_marker_value() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(SAVEGAME_EOF, 0x1d);
    }

    #[test]
    fn version_string_length() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(VERSIONSIZE, 16);
        let test_str = "version 110"; // typical version string
        assert!(test_str.len() < VERSIONSIZE);
    }

    #[test]
    fn save_string_size() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(SAVESTRINGSIZE, 24);
    }

    /// Runs `f` with `save_stream` pointed at an in-memory buffer backed by
    /// `data`, then restores the original stream and error globals on exit.
    unsafe fn with_mem_stream<F: FnOnce()>(data: &mut [u8], f: F) {
        let old_stream = save_stream;
        let old_error = savegame_error;
        let old_len = savegamelength;
        save_stream = libc::fmemopen(
            data.as_mut_ptr() as *mut libc::c_void,
            data.len(),
            c"r".as_ptr() as *const libc::c_char,
        );
        savegame_error = 0;
        savegamelength = 0;
        f();
        libc::fclose(save_stream);
        save_stream = old_stream;
        savegame_error = old_error;
        savegamelength = old_len;
    }

    #[test]
    fn saveg_read_thinker_t_zero_fn_ptr_becomes_none() {
        let _g = LOCK.lock().unwrap();
        // 12 bytes: prev(4) + next(4) + fn_ptr(4), all zero
        let mut data = [0u8; 12];
        unsafe {
            let mut th = thinker_t {
                prev: ptr::null_mut(),
                next: ptr::null_mut(),
                function: actionf_t { acp1: None },
            };
            with_mem_stream(&mut data, || {
                saveg_read_thinker_t(&raw mut th as *mut thinker_t);
            });
            assert!(
                th.function.acp1.is_none(),
                "zero function pointer must deserialize as None, not Some(NULL)"
            );
        }
    }

    #[test]
    fn saveg_read_mobj_t_zero_fn_ptr_becomes_none() {
        use crate::doom::c_ffi::mobj_t;
        let _g = LOCK.lock().unwrap();
        // mobj_t is large; we need enough bytes for the full struct read.
        // The thinker (prev+next+fn) is the first 12 bytes.
        // saveg_read_mobj_t reads many fields — pad to 256 zeros.
        let mut data = [0u8; 256];
        unsafe {
            let mut mo = std::mem::MaybeUninit::<mobj_t>::zeroed().assume_init();
            with_mem_stream(&mut data, || {
                saveg_read_mobj_t(&raw mut mo as *mut mobj_t as *mut libc::c_void);
            });
            let thinker_ptr = std::ptr::addr_of!(mo.thinker_prev) as *const thinker_t;
            assert!(
                (*thinker_ptr).function.acp1.is_none(),
                "zero function pointer in mobj thinker must deserialize as None, not Some(NULL)"
            );
        }
    }

    #[test]
    fn save_game_path_not_truncated_for_long_directory() {
        let long_dir = "/very/long/savegame/directory/path/"; // 35 chars — forces path > 31 chars
        let slot = 3i32;
        let alloc_size = long_dir.len() + 32;
        let mut buf = vec![0u8; alloc_size];
        fill_save_filename(&mut buf, long_dir, slot);
        let result = std::ffi::CStr::from_bytes_until_nul(&buf)
            .unwrap()
            .to_str()
            .unwrap();
        let expected = format!("{}{}{}.dsg", long_dir, SAVEGAMENAME, slot);
        assert_eq!(
            result, expected,
            "save path was truncated for long directory"
        );
    }
}

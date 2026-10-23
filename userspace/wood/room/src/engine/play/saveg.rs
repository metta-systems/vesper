//! Rust port of vendor/doomgeneric/p_saveg.c.
//!
//! Implements save-game serialization and deserialization for the entire Doom
//! game state. All active map objects (mobjs), player state, world geometry
//! deltas, thinker chains, and sector specials (ceilings, doors, floors,
//! platforms, lights) are written to or read from the in-memory
//! [`save_stream`], which `g_game` loads from or writes to the save file.
//!
//! The on-disk format is a flat byte stream. Multi-byte integers are always
//! little-endian regardless of host byte order. Sector and state pointers are
//! serialized as array indices; player pointers use a 1-based index (0 means
//! NULL). Thinker function slots stored in saves are raw addresses that are
//! only tested against null on load; the thinker kind is re-established from
//! the record's class (see [`saved_function_word`]).
//!
//! Notable differences from the C source:
//! - `saveg_write8` increments `savegamelength`; the C version does not.
//! - `saveg_read16`/`saveg_write16` operate on `u16`/`u16` rather than C
//!   `short`/`short`, avoiding sign-extension ambiguity.
//! - State indices are bounds-checked on read; out-of-range values set
//!   `savegame_error` and clamp to `states[0]`.
//! - `P_SaveGameFile` and `P_TempSaveGameFile` return owned `String` paths
//!   instead of pointers into function-local static buffers.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use {
    crate::i_error,
    std::{
        ffi::c_void,
        io::{Cursor, Read, Write},
        ptr,
    },
};

use crate::engine::{
    doom::player::{
        players, PlayerMessage, PlayerT, PspdefT, TiccmdT, MAXPLAYERS, NUMAMMO, NUMCARDS,
        NUMPOWERS, NUMPSPRITES, NUMWEAPONS,
    },
    map::{line_t, sector_t, side_t, subsector_t},
    play::{
        ceilng::{ceiling_t, P_AddActiveCeiling, T_MoveCeiling, MAXCEILINGS},
        doors::{vldoor_t, T_VerticalDoor},
        floor::{floormove_t, T_MoveFloor},
        info::{mobjinfo, states, MobjInfo, State, NUMSTATES},
        lights::{
            glow_t, lightflash_t, strobe_t, T_FireFlicker, T_Glow, T_LightFlash, T_StrobeFlash,
        },
        mobj::mobj_t,
        plats::{plat_t, P_AddActivePlat, T_PlatRaise},
        tick::{
            leveltime, thinker_allocation, thinker_t, thinkercap, P_AddThinker, P_InitThinkers,
            ThinkerContainer, ThinkerKind,
        },
    },
    wad::format::mapthing_t,
    zone::{Z_Free, Z_Malloc, PU_LEVEL, PU_LEVSPEC},
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum length (in bytes, including NUL) of the human-readable save-game
/// description string written at the start of every save file.
/// Corresponds to `SAVESTRINGSIZE` in the C source.
const SAVESTRINGSIZE: usize = 24;

/// End-of-file marker byte written at the end of every save game
/// (SAVEGAME_EOF in p_saveg.c = 0x1d).
pub const SAVEGAME_EOF: u8 = 0x1d;
/// Length of the version string embedded in save-game headers
/// (VERSIONSIZE in p_saveg.h = 16).
pub const VERSIONSIZE: usize = 16;

// ---------------------------------------------------------------------------
// Globals
// ---------------------------------------------------------------------------

/// The save game being read or written, held in memory.
///
/// `G_DoLoadGame` fills it with the save file's bytes before unarchiving;
/// `G_DoSaveGame` archives into it and then writes its bytes to the file it
/// opened. Positions in it are file offsets, so padding is computed from
/// [`Cursor::position`] as the C code did with `ftell`.
pub static mut save_stream: Cursor<Vec<u8>> = Cursor::new(Vec::new());

/// The active [`save_stream`].
///
/// # Safety
/// No other reference to `save_stream` may be live.
unsafe fn stream() -> &'static mut Cursor<Vec<u8>> {
    &mut *ptr::addr_of_mut!(save_stream)
}

/// Running byte count of data written to the current save stream.
/// Incremented by `saveg_write8`; read by `g_game.c` after saving completes.
/// C origin: `savegamelength` in `p_saveg.c`.
pub static mut savegamelength: i32 = 0;

/// Non-zero when a read or write error has occurred on `save_stream`.
/// Set to `1` on the first I/O failure; callers check this after
/// archiving/unarchiving to decide whether to accept the save.
/// C origin: `savegame_error` in `p_saveg.c`.
pub static mut savegame_error: i32 = 0;

use crate::engine::{
    doom::main::savegamedir,
    game::G_VanillaVersionCode,
    misc::until_nul,
    play::{
        maputl::P_SetThingPosition,
        mobj::{P_MobjThinker, P_RemoveMobj},
        setup::{lines, numlines, numsectors, sectors, sides},
    },
};

// ---------------------------------------------------------------------------
// Low-level I/O on save_stream.
// ---------------------------------------------------------------------------

/// Reads one byte from `save_stream`.
///
/// Sets `savegame_error = 1` on the first short read; subsequent calls still
/// return `0` but do not double-set the flag. C origin: `saveg_read8`.
unsafe fn saveg_read8() -> u8 {
    let mut result = [0u8];
    let n = stream().read(&mut result).unwrap_or(0);
    if n < 1 && savegame_error == 0 {
        savegame_error = 1;
    }
    result[0]
}

/// Writes one byte to `save_stream` and increments `savegamelength`.
///
/// Sets `savegame_error = 1` on the first short write.
///
/// Note: the C version does not increment `savegamelength` here; this port
/// does so to avoid a separate accounting pass.
/// C origin: `saveg_write8`.
unsafe fn saveg_write8(value: u8) {
    let n = stream().write(&[value]).unwrap_or(0);
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
    let pos = stream().position();
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
    let pos = stream().position();
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
        sector.offset_from(sectors.cast::<sector_t>()) as u32
    }
}

/// Deserializes a sector index from the save stream into a `*mut sector_t`.
///
/// `index` is an offset into the global `sectors` array. No bounds check is
/// performed; callers must trust that the index was written by a valid
/// `saveg_write_sector_ptr` call.
unsafe fn saveg_read_sector_ptr(index: u32) -> *mut sector_t {
    sectors.cast::<sector_t>().add(index as usize)
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
// Thinker function slot
// ---------------------------------------------------------------------------

/// The 32-bit value written into a saved thinker's `function` slot.
///
/// C writes the think-function pointer truncated to 32 bits; the loader only
/// tests it against null (to tell an active ceiling or platform from one in
/// stasis). This port writes the same thing: the address of the function
/// `kind` dispatches to, truncated to 32 bits; `0` for a kind with no think
/// function (C's null); and the truncated `-1` sentinel for a removed thinker.
fn saved_function_word(kind: ThinkerKind) -> u32 {
    let function: *const () = match kind {
        ThinkerKind::None | ThinkerKind::CeilingInStasis | ThinkerKind::PlatInStasis => ptr::null(),
        ThinkerKind::Mobj => P_MobjThinker as *const (),
        ThinkerKind::Ceiling => T_MoveCeiling as *const (),
        ThinkerKind::Door => T_VerticalDoor as *const (),
        ThinkerKind::Floor => T_MoveFloor as *const (),
        ThinkerKind::Plat => T_PlatRaise as *const (),
        ThinkerKind::LightFlash => T_LightFlash as *const (),
        ThinkerKind::Strobe => T_StrobeFlash as *const (),
        ThinkerKind::Glow => T_Glow as *const (),
        ThinkerKind::FireFlicker => T_FireFlicker as *const (),
        ThinkerKind::Removed(_) => return u32::MAX,
    };
    function as usize as u32
}

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
unsafe fn saveg_read_mapthing_t(mt: *mut mapthing_t) {
    let s = &mut *mt;
    s.x.set(saveg_read16() as i16);
    s.y.set(saveg_read16() as i16);
    s.angle.set(saveg_read16() as i16);
    s.kind.set(saveg_read16() as i16);
    s.options.set(saveg_read16() as i16);
}

/// Writes a `mapthing_t` to the save stream from `*mt`.
///
/// Writes five `i16` fields in order: x, y, angle, type, options.
/// C origin: `saveg_write_mapthing_t`.
unsafe fn saveg_write_mapthing_t(mt: *const mapthing_t) {
    let s = &*mt;
    saveg_write16(s.x.get() as u16);
    saveg_write16(s.y.get() as u16);
    saveg_write16(s.angle.get() as u16);
    saveg_write16(s.kind.get() as u16);
    saveg_write16(s.options.get() as u16);
}

//
// thinker_t
//

/// Reads a `thinker_t` header (prev, next, function) from the save stream.
///
/// The `prev` and `next` pointer values stored in the stream are raw addresses
/// from the saving session and are not meaningful in the loading session; they
/// will be overwritten when the thinker is re-linked. The saved function slot
/// is only tested against null, as in C: `0` reads as [`ThinkerKind::None`],
/// anything else as `kind_if_function_set`. The `P_UnArchive*` callers then
/// settle the kind (C overwrites the function the same way).
/// C origin: `saveg_read_thinker_t`.
unsafe fn saveg_read_thinker_t(th: *mut thinker_t, kind_if_function_set: ThinkerKind) {
    let s = &mut *th;
    // Read prev/next as raw pointer indices (will be rebuilt)
    s.prev = saveg_read32() as usize as *mut thinker_t;
    s.next = saveg_read32() as usize as *mut thinker_t;
    s.kind = if saveg_read32() == 0 {
        ThinkerKind::None
    } else {
        kind_if_function_set
    };
}

/// Writes a `thinker_t` header (prev, next, function) to the save stream.
///
/// Pointer fields are written as raw 32-bit addresses; the function slot is
/// written by [`saved_function_word`]. C origin: `saveg_write_thinker_t`.
unsafe fn saveg_write_thinker_t(th: *const thinker_t) {
    let s = &*th;
    saveg_write32(s.prev as u32);
    saveg_write32(s.next as u32);
    saveg_write32(saved_function_word(s.kind));
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
    let mo = mobj as *mut mobj_t;

    // thinker_t
    saveg_read_thinker_t(&raw mut (*mo).thinker, ThinkerKind::Mobj);

    // x, y, z
    (*mo).x = saveg_read32() as i32;
    (*mo).y = saveg_read32() as i32;
    (*mo).z = saveg_read32() as i32;

    // snext, sprev
    (*mo).snext = saveg_read32() as *mut mobj_t;
    (*mo).sprev = saveg_read32() as *mut mobj_t;

    // angle, sprite, frame
    (*mo).angle = saveg_read32();
    (*mo).sprite = saveg_read_enum() as i32;
    (*mo).frame = saveg_read32() as i32;

    // bnext, bprev
    (*mo).bnext = saveg_read32() as *mut mobj_t;
    (*mo).bprev = saveg_read32() as *mut mobj_t;

    // subsector
    (*mo).subsector = saveg_read32() as *mut subsector_t;

    // floorz, ceilingz, radius, height
    (*mo).floorz = saveg_read32() as i32;
    (*mo).ceilingz = saveg_read32() as i32;
    (*mo).radius = saveg_read32() as i32;
    (*mo).height = saveg_read32() as i32;

    // momx, momy, momz
    (*mo).momx = saveg_read32() as i32;
    (*mo).momy = saveg_read32() as i32;
    (*mo).momz = saveg_read32() as i32;

    // validcount
    (*mo).validcount = saveg_read32() as i32;

    // type
    (*mo).kind = saveg_read_enum() as i32;

    // info (raw pointer)
    (*mo).info = saveg_read32() as *mut MobjInfo;

    // tics
    (*mo).tics = saveg_read32() as i32;

    // state (index into states array)
    let state_idx = saveg_read32();
    (*mo).state = saveg_read_state_ptr(state_idx);

    // flags
    (*mo).flags = saveg_read32() as i32;

    // health
    (*mo).health = saveg_read32() as i32;

    // movedir, movecount
    (*mo).movedir = saveg_read32() as i32;
    (*mo).movecount = saveg_read32() as i32;

    // target (raw pointer)
    (*mo).target = saveg_read32() as *mut mobj_t;

    // reactiontime, threshold
    (*mo).reactiontime = saveg_read32() as i32;
    (*mo).threshold = saveg_read32() as i32;

    // player (index + 1)
    let pl = saveg_read32();
    if pl > 0 {
        (*mo).player = saveg_read_player_ptr(pl);
        let player = (*mo).player;
        if !player.is_null() {
            (*player).mo = mo;
        }
    } else {
        (*mo).player = std::ptr::null_mut();
    }

    // lastlook
    (*mo).lastlook = saveg_read32() as i32;

    // spawnpoint
    saveg_read_mapthing_t(std::ptr::addr_of_mut!((*mo).spawnpoint));

    // tracer (raw pointer)
    (*mo).tracer = saveg_read32() as *mut mobj_t;
}

/// Writes a full `mobj_t` record to the save stream from `mobj`.
///
/// Pointers (snext, sprev, bnext, bprev, subsector, target, tracer, info) are
/// written as raw 32-bit addresses; they are not portable across sessions but
/// are reconstructed on load. `state` is written as an index into `states`.
/// `player` is written as a 1-based player index (0 for NULL).
/// C origin: `saveg_write_mobj_t`.
unsafe fn saveg_write_mobj_t(mobj: *const c_void) {
    let mo = mobj as *const mobj_t;

    // thinker_t
    saveg_write_thinker_t(&raw const (*mo).thinker);

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
    saveg_write_enum((*mo).kind as u32);

    // info
    saveg_write32((*mo).info as u32);

    // tics
    saveg_write32((*mo).tics as u32);

    // state (index)
    let state = (*mo).state;
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
    let player = (*mo).player;
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
        saveg_read_state_ptr(state_idx)
    };
    s.tics = saveg_read32() as i32;
    s.sx = saveg_read32() as i32;
    s.sy = saveg_read32() as i32;
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
        saveg_write32(saveg_write_state_ptr(s.state));
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
    s.mo = saveg_read32() as *mut mobj_t;

    // playerstate
    s.playerstate = saveg_read_enum() as i32;

    // cmd
    saveg_read_ticcmd_t(&mut s.cmd);

    // viewz, viewheight, deltaviewheight, bob
    s.viewz = saveg_read32() as i32;
    s.viewheight = saveg_read32() as i32;
    s.deltaviewheight = saveg_read32() as i32;
    s.bob = saveg_read32() as i32;

    // health, armorpoints, armortype
    s.health = saveg_read32() as i32;
    s.armorpoints = saveg_read32() as i32;
    s.armortype = saveg_read32() as i32;

    // powers[NUMPOWERS]
    for i in 0..NUMPOWERS {
        s.powers[i] = saveg_read32() as i32;
    }

    // cards[NUMCARDS]
    for i in 0..NUMCARDS {
        s.cards[i] = saveg_read32() as i32;
    }

    // backpack
    s.backpack = saveg_read32() as i32;

    // frags[MAXPLAYERS]
    for i in 0..MAXPLAYERS {
        s.frags[i] = saveg_read32() as i32;
    }

    // readyweapon, pendingweapon
    s.readyweapon = saveg_read_enum() as i32;
    s.pendingweapon = saveg_read_enum() as i32;

    // weaponowned[NUMWEAPONS]
    for i in 0..NUMWEAPONS {
        s.weaponowned[i] = saveg_read32() as i32;
    }

    // ammo[NUMAMMO]
    for i in 0..NUMAMMO {
        s.ammo[i] = saveg_read32() as i32;
    }

    // maxammo[NUMAMMO]
    for i in 0..NUMAMMO {
        s.maxammo[i] = saveg_read32() as i32;
    }

    // attackdown, usedown
    s.attackdown = saveg_read32() as i32;
    s.usedown = saveg_read32() as i32;

    // cheats, refire
    s.cheats = saveg_read32() as i32;
    s.refire = saveg_read32() as i32;

    // killcount, itemcount, secretcount
    s.killcount = saveg_read32() as i32;
    s.itemcount = saveg_read32() as i32;
    s.secretcount = saveg_read32() as i32;

    // message: a pointer slot in the save format, never restored
    saveg_read32();
    s.message = PlayerMessage::NONE;

    // damagecount, bonuscount
    s.damagecount = saveg_read32() as i32;
    s.bonuscount = saveg_read32() as i32;

    // attacker (raw pointer)
    s.attacker = saveg_read32() as *mut mobj_t;

    // extralight, fixedcolormap, colormap
    s.extralight = saveg_read32() as i32;
    s.fixedcolormap = saveg_read32() as i32;
    s.colormap = saveg_read32() as i32;

    // psprites[NUMPSPRITES]
    for i in 0..NUMPSPRITES {
        saveg_read_pspdef_t(&mut s.psprites[i]);
    }

    // didsecret
    s.didsecret = saveg_read32() as i32;
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
    saveg_write32(0); // message pointer slot
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
    saveg_read_thinker_t(&mut s.thinker, ThinkerKind::Ceiling);
    s.kind = saveg_read_enum() as i32;
    let sector_idx = saveg_read32();
    s.sector = saveg_read_sector_ptr(sector_idx);
    s.bottomheight = saveg_read32() as i32;
    s.topheight = saveg_read32() as i32;
    s.speed = saveg_read32() as i32;
    s.crush = saveg_read32() as i32;
    s.direction = saveg_read32() as i32;
    s.tag = saveg_read32() as i32;
    s.olddirection = saveg_read32() as i32;
}

/// Writes a `ceiling_t` special thinker to the save stream from `*ceil`.
///
/// The sector pointer is serialized as an index via `saveg_write_sector_ptr`.
/// C origin: `saveg_write_ceiling_t`.
unsafe fn saveg_write_ceiling_t(ceil: *const ceiling_t) {
    let s = &*ceil;
    saveg_write_thinker_t(&s.thinker);
    saveg_write_enum(s.kind as u32);
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
    saveg_read_thinker_t(&mut s.thinker, ThinkerKind::Door);
    s.kind = saveg_read_enum() as i32;
    let sector_idx = saveg_read32();
    s.sector = saveg_read_sector_ptr(sector_idx);
    s.topheight = saveg_read32() as i32;
    s.speed = saveg_read32() as i32;
    s.direction = saveg_read32() as i32;
    s.topwait = saveg_read32() as i32;
    s.topcountdown = saveg_read32() as i32;
}

/// Writes a `vldoor_t` thinker to the save stream.
///
/// C origin: `saveg_write_vldoor_t`.
unsafe fn saveg_write_vldoor_t(door: *const vldoor_t) {
    let s = &*door;
    saveg_write_thinker_t(&s.thinker);
    saveg_write_enum(s.kind as u32);
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
    saveg_read_thinker_t(&mut s.thinker, ThinkerKind::Floor);
    s.kind = saveg_read_enum() as i32;
    s.crush = saveg_read32() as i32;
    let sector_idx = saveg_read32();
    s.sector = saveg_read_sector_ptr(sector_idx);
    s.direction = saveg_read32() as i32;
    s.newspecial = saveg_read32() as i32;
    s.texture = saveg_read16() as i16;
    s.floordestheight = saveg_read32() as i32;
    s.speed = saveg_read32() as i32;
}

/// Writes a `floormove_t` thinker to the save stream.
///
/// C origin: `saveg_write_floormove_t`.
unsafe fn saveg_write_floormove_t(floor: *const floormove_t) {
    let s = &*floor;
    saveg_write_thinker_t(&s.thinker);
    saveg_write_enum(s.kind as u32);
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
    saveg_read_thinker_t(&mut s.thinker, ThinkerKind::Plat);
    let sector_idx = saveg_read32();
    s.sector = saveg_read_sector_ptr(sector_idx);
    s.speed = saveg_read32() as i32;
    s.low = saveg_read32() as i32;
    s.high = saveg_read32() as i32;
    s.wait = saveg_read32() as i32;
    s.count = saveg_read32() as i32;
    s.status = saveg_read_enum() as i32;
    s.oldstatus = saveg_read_enum() as i32;
    s.crush = saveg_read32() as i32;
    s.tag = saveg_read32() as i32;
    s.kind = saveg_read_enum() as i32;
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
    saveg_write_enum(s.kind as u32);
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
    saveg_read_thinker_t(&mut s.thinker, ThinkerKind::LightFlash);
    let sector_idx = saveg_read32();
    s.sector = saveg_read_sector_ptr(sector_idx);
    s.count = saveg_read32() as i32;
    s.maxlight = saveg_read32() as i32;
    s.minlight = saveg_read32() as i32;
    s.maxtime = saveg_read32() as i32;
    s.mintime = saveg_read32() as i32;
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
    saveg_read_thinker_t(&mut s.thinker, ThinkerKind::Strobe);
    let sector_idx = saveg_read32();
    s.sector = saveg_read_sector_ptr(sector_idx);
    s.count = saveg_read32() as i32;
    s.minlight = saveg_read32() as i32;
    s.maxlight = saveg_read32() as i32;
    s.darktime = saveg_read32() as i32;
    s.brighttime = saveg_read32() as i32;
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
    saveg_read_thinker_t(&mut s.thinker, ThinkerKind::Glow);
    let sector_idx = saveg_read32();
    s.sector = saveg_read_sector_ptr(sector_idx);
    s.minlight = saveg_read32() as i32;
    s.maxlight = saveg_read32() as i32;
    s.direction = saveg_read32() as i32;
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

/// The save-game filename `"{dir}{SAVEGAMENAME}{slot}.dsg"`.
fn save_filename(dir: &str, slot: i32) -> String {
    format!("{dir}{SAVEGAMENAME}{slot}.dsg")
}

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
/// filename on success.
///
/// Called by `g_game.c` (`G_DoSaveGame`).
///
/// # Safety
///
/// Reads the `savegamedir` static; the caller must not be mutating it.
pub unsafe fn P_TempSaveGameFile() -> String {
    format!("{}temp.dsg", *ptr::addr_of!(savegamedir))
}

/// Returns the path to the save-game file for the given `slot` (0-7).
///
/// Called by `g_game.c` to open the final save file for reading or renaming.
///
/// # Safety
///
/// Reads the `savegamedir` static; the caller must not be mutating it.
pub unsafe fn P_SaveGameFile(slot: i32) -> String {
    save_filename(&*ptr::addr_of!(savegamedir), slot)
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
/// `save_stream` must be the stream being written. The global state
/// `gameskill`, `gameepisode`, `gamemap`, `playeringame`, and `leveltime`
/// must have been set to valid values before this call.
///
/// `description` is read up to its first NUL, so the fixed description
/// buffer can be passed whole.
pub unsafe fn P_WriteSaveGameHeader(description: &[u8]) {
    let desc_bytes = until_nul(description);

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
/// `save_stream` must be positioned at the start of
/// a save file written by `P_WriteSaveGameHeader`. On success, `gameskill`,
/// `gameepisode`, `gamemap`, `playeringame`, and `leveltime` are overwritten
/// with values from the stream.
pub unsafe fn P_ReadSaveGameHeader() -> i32 {
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
    gameskill = saveg_read8() as i32;
    gameepisode = saveg_read8() as i32;
    gamemap = saveg_read8() as i32;

    // Read playeringame
    for i in 0..MAXPLAYERS {
        playeringame[i] = saveg_read8() as i32;
    }

    // Read leveltime (3 bytes, big-endian)
    let a = saveg_read8() as u32;
    let b = saveg_read8() as u32;
    let c = saveg_read8() as u32;
    leveltime = ((a << 16) | (b << 8) | c) as i32;

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
/// `save_stream` must be positioned immediately
/// after the last unarchived byte, i.e. where `P_WriteSaveGameEOF` wrote its
/// marker. Reading past the end of the stream sets `savegame_error`.
pub unsafe fn P_ReadSaveGameEOF() -> i32 {
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
/// `save_stream` must be the stream being written. This must be called after
/// all archive functions have finished writing so that the marker byte appears
/// at the correct position for `P_ReadSaveGameEOF` to validate.
pub unsafe fn P_WriteSaveGameEOF() {
    saveg_write8(SAVEGAME_EOF);
}

use crate::engine::game::{gameepisode, gamemap, gameskill, playeringame};

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
/// `save_stream` must be the stream being written.
/// The `players` array and `playeringame` flags must be fully initialized for
/// all `MAXPLAYERS` slots. Must be called after `P_WriteSaveGameHeader` and
/// before `P_WriteSaveGameEOF`.
pub unsafe fn P_ArchivePlayers() {
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
/// `save_stream` must be positioned at the byte
/// sequence written by `P_ArchivePlayers`. `playeringame` must already have
/// been populated by `P_ReadSaveGameHeader` so the active-slot bitmask is
/// correct. After this call, `players[i].mo`, `.message`, and `.attacker` are
/// null and must not be dereferenced until thinkers are unarchived.
pub unsafe fn P_UnArchivePlayers() {
    for i in 0..MAXPLAYERS {
        if playeringame[i] == 0 {
            continue;
        }
        saveg_read_pad();
        saveg_read_player_t(&mut players[i]);

        // will be set when unarc thinker
        players[i].mo = std::ptr::null_mut();
        players[i].message = PlayerMessage::NONE;
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
/// `save_stream` must be the stream being written.
/// The global `sectors` array (length `numsectors`), `lines` array (length
/// `numlines`), and `sides` array must all be fully initialized. Every
/// `line_t.sidenum[j]` that is not `-1` must be a valid index into `sides`.
pub unsafe fn P_ArchiveWorld() {
    let num_sec = numsectors as usize;
    let num_li = numlines as usize;

    // do sectors
    for i in 0..num_sec {
        let sec = sectors.cast::<sector_t>().add(i);
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
        let li = lines.cast::<line_t>().add(i);
        saveg_write16((*li).flags as u16);
        saveg_write16((*li).special as u16);
        saveg_write16((*li).tag as u16);
        for j in 0..2 {
            if (*li).sidenum[j as usize] == -1 {
                continue;
            }
            let si = sides
                .cast::<side_t>()
                .add((*li).sidenum[j as usize] as usize);
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
/// `save_stream` must be positioned at the byte
/// sequence written by `P_ArchiveWorld`. The `sectors` array (length
/// `numsectors`), `lines` array (length `numlines`), and `sides` array must
/// all be allocated and at least partially initialized (map load must have
/// completed). Every `line_t.sidenum[j]` that is not `-1` must be a valid
/// index into `sides`.
pub unsafe fn P_UnArchiveWorld() {
    let num_sec = numsectors as usize;
    let num_li = numlines as usize;

    // do sectors
    for i in 0..num_sec {
        let sec = sectors.cast::<sector_t>().add(i);
        (*sec).floorheight = (saveg_read16() as i16 as i32) << 16;
        (*sec).ceilingheight = (saveg_read16() as i16 as i32) << 16;
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
        let li = lines.cast::<line_t>().add(i);
        (*li).flags = saveg_read16() as i16;
        (*li).special = saveg_read16() as i16;
        (*li).tag = saveg_read16() as i16;
        for j in 0..2 {
            if (*li).sidenum[j as usize] == -1 {
                continue;
            }
            let si = sides
                .cast::<side_t>()
                .add((*li).sidenum[j as usize] as usize);
            (*si).textureoffset = (saveg_read16() as i16 as i32) << 16;
            (*si).rowoffset = (saveg_read16() as i16 as i32) << 16;
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
/// Walks `thinkercap.next ... thinkercap`; for each thinker of kind
/// [`ThinkerKind::Mobj`], writes a `tc_mobj` byte, 4-byte alignment padding,
/// then the full `mobj_t` record. Non-mobj thinkers are silently skipped
/// (the C version also skips them without error). Terminates the list with a
/// `tc_end` byte.
///
/// Called by `g_game.c` (`G_DoSaveGame`).
///
/// # Safety
///
/// `save_stream` must be the stream being written.
/// The thinker chain rooted at `thinkercap` must be fully initialized and form
/// a valid doubly-linked circular list. Every thinker in the chain of kind
/// [`ThinkerKind::Mobj`] must be embedded in a valid `mobj_t`.
pub unsafe fn P_ArchiveThinkers() {
    let cap = &raw mut thinkercap;
    let mut th = (*cap).next;

    while th != cap {
        if (*th).kind == ThinkerKind::Mobj {
            saveg_write8(tc_mobj);
            saveg_write_pad();
            saveg_write_mobj_t(mobj_t::from_thinker(th) as *const c_void);
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
/// via `P_RemoveMobj`, all others (removed mobjs included) have their owning
/// struct freed via `Z_Free`. Then
/// `P_InitThinkers` resets the chain to empty.
///
/// Second pass: reads thinker-class bytes from the stream in a loop.
/// - `tc_end`: returns immediately.
/// - `tc_mobj`: allocates a `mobj_t` with `Z_Malloc(PU_LEVEL)`, reads its
///   fields, nulls out `target` and `tracer`, calls `P_SetThingPosition`,
///   rebuilds `info` from `mobjinfo[type]`, recomputes `floorz`/`ceilingz`
///   from the subsector's sector, sets the thinker kind to
///   [`ThinkerKind::Mobj`], and registers it with `P_AddThinker`.
/// - Any other byte: calls `i_error!` (fatal).
///
/// Called by `g_game.c` (`G_DoLoadGame`).
///
/// # Safety
///
/// `save_stream` must be positioned at the byte
/// sequence written by `P_ArchiveThinkers`. The zone allocator must be
/// operational (`Z_Malloc`/`Z_Free` must be safe to call). The map geometry
/// (`sectors`, block-map, etc.) must be loaded so that `P_SetThingPosition`
/// can place each restored mobj. Must be called before `P_UnArchiveSpecials`
/// and before any code dereferences `players[i].mo`.
pub unsafe fn P_UnArchiveThinkers() {
    let cap = &raw mut thinkercap;
    let mut currentthinker = (*cap).next;

    // remove all current thinkers
    while currentthinker != cap {
        let next = (*currentthinker).next;

        let kind = (*currentthinker).kind;
        if kind == ThinkerKind::Mobj {
            P_RemoveMobj(mobj_t::from_thinker(currentthinker));
        } else {
            let Some(class) = kind.class() else {
                i_error!("P_UnArchiveThinkers: thinker has no kind");
            };
            Z_Free(thinker_allocation(currentthinker, class));
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
                    std::mem::size_of::<mobj_t>() as i32,
                    PU_LEVEL,
                    std::ptr::null_mut(),
                );
                saveg_read_mobj_t(mobj);

                let mo = mobj as *mut mobj_t;

                // target/tracer set to NULL (will be rebuilt)
                (*mo).target = std::ptr::null_mut();
                (*mo).tracer = std::ptr::null_mut();

                P_SetThingPosition(mobj.cast());

                // rebuild info from mobjinfo
                (*mo).info = &raw mut mobjinfo[(*mo).kind as usize];

                // rebuild floorz/ceilingz from subsector
                let subsec = (*mo).subsector;
                if !subsec.is_null() {
                    let sector = (*subsec).sector;
                    if !sector.is_null() {
                        (*mo).floorz = (*sector).floorheight;
                        (*mo).ceilingz = (*sector).ceilingheight;
                    }
                }

                // set thinker kind
                (*mo).thinker.kind = ThinkerKind::Mobj;

                P_AddThinker(&raw mut (*mo).thinker);
            }
            _ => {
                i_error!("Unknown tclass {} in savegame", tclass as i32);
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
/// Walks the thinker chain and identifies specials by their thinker kind:
/// - Thinkers without a think function (C's `NULL`) that appear in the
///   `activeceilings` list are written as `tc_ceiling` records (these are
///   ceilings paused mid-crush).
/// - `Ceiling` → `tc_ceiling`
/// - `Door` → `tc_door`
/// - `Floor` → `tc_floor`
/// - `Plat` → `tc_plat`
/// - `LightFlash` → `tc_flash`
/// - `Strobe` → `tc_strobe`
/// - `Glow` → `tc_glow`
///
/// Each matching thinker is preceded by its class tag byte and 4-byte
/// alignment padding. Unrecognized thinkers are silently skipped. The list is
/// terminated by `tc_endspecials`.
///
/// Called by `g_game.c` (`G_DoSaveGame`).
///
/// # Safety
///
/// `save_stream` must be the stream being written.
/// The thinker chain rooted at `thinkercap` must be fully initialized and form
/// a valid doubly-linked circular list. The `activeceilings` array must be
/// initialized. Every thinker of a special kind must be embedded in a fully
/// initialized special struct (`ceiling_t`, `vldoor_t`, etc.).
pub unsafe fn P_ArchiveSpecials() {
    let cap = &raw mut thinkercap;
    let mut th = (*cap).next;

    while th != cap {
        match (*th).kind {
            // C's null function: a ceiling in stasis is still saved when it
            // is in `activeceilings`. A platform in stasis is not saved at
            // all (vanilla behaviour).
            ThinkerKind::None | ThinkerKind::CeilingInStasis | ThinkerKind::PlatInStasis => {
                let activeceilings_ptr =
                    std::ptr::addr_of!(crate::engine::play::ceilng::activeceilings[0]);
                let mut found = false;
                for i in 0..MAXCEILINGS {
                    let ceiling = *activeceilings_ptr.add(i);
                    if !ceiling.is_null() && &raw mut (*ceiling).thinker == th {
                        found = true;
                        break;
                    }
                }
                if found {
                    saveg_write8(tc_ceiling);
                    saveg_write_pad();
                    saveg_write_ceiling_t(ceiling_t::from_thinker(th));
                }
            }
            ThinkerKind::Ceiling => {
                saveg_write8(tc_ceiling);
                saveg_write_pad();
                saveg_write_ceiling_t(ceiling_t::from_thinker(th));
            }
            ThinkerKind::Door => {
                saveg_write8(tc_door);
                saveg_write_pad();
                saveg_write_vldoor_t(vldoor_t::from_thinker(th));
            }
            ThinkerKind::Floor => {
                saveg_write8(tc_floor);
                saveg_write_pad();
                saveg_write_floormove_t(floormove_t::from_thinker(th));
            }
            ThinkerKind::Plat => {
                saveg_write8(tc_plat);
                saveg_write_pad();
                saveg_write_plat_t(plat_t::from_thinker(th));
            }
            ThinkerKind::LightFlash => {
                saveg_write8(tc_flash);
                saveg_write_pad();
                saveg_write_lightflash_t(lightflash_t::from_thinker(th));
            }
            ThinkerKind::Strobe => {
                saveg_write8(tc_strobe);
                saveg_write_pad();
                saveg_write_strobe_t(strobe_t::from_thinker(th));
            }
            ThinkerKind::Glow => {
                saveg_write8(tc_glow);
                saveg_write_pad();
                saveg_write_glow_t(glow_t::from_thinker(th));
            }
            // Map objects are saved by `P_ArchiveThinkers`; fire flickers are
            // not saved (vanilla behaviour); removed thinkers are skipped.
            ThinkerKind::Mobj | ThinkerKind::FireFlicker | ThinkerKind::Removed(_) => {}
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
/// `save_stream` must be positioned at the byte
/// sequence written by `P_ArchiveSpecials`. The zone allocator must be
/// operational. The `sectors` array must be fully initialized so that
/// `sector->specialdata` can be set. Must be called after
/// `P_UnArchiveThinkers` so the thinker chain is in a consistent state before
/// new specials are registered with `P_AddThinker`.
// FIXME: p_saveg.c allocates ceiling_t with PU_LEVEL, not PU_LEVSPEC; this
// port uses PU_LEVSPEC here (consistent with other specials). The difference
// affects when the zone allocator may purge the block.
pub unsafe fn P_UnArchiveSpecials() {
    loop {
        let tclass = saveg_read8();
        match tclass {
            x if x == tc_endspecials => return,
            x if x == tc_ceiling => {
                saveg_read_pad();
                let ceiling = Z_Malloc(
                    std::mem::size_of::<ceiling_t>() as i32,
                    PU_LEVSPEC,
                    std::ptr::null_mut(),
                ) as *mut ceiling_t;
                saveg_read_ceiling_t(ceiling);
                (*ceiling).sector.as_mut().unwrap().specialdata = &raw mut (*ceiling).thinker;

                // A set function slot read as `Ceiling`; a null one (C keeps
                // the function null) is a ceiling in stasis.
                if (*ceiling).thinker.kind == ThinkerKind::None {
                    (*ceiling).thinker.kind = ThinkerKind::CeilingInStasis;
                }

                P_AddThinker(&raw mut (*ceiling).thinker);
                P_AddActiveCeiling(ceiling);
            }
            x if x == tc_door => {
                saveg_read_pad();
                let door = Z_Malloc(
                    std::mem::size_of::<vldoor_t>() as i32,
                    PU_LEVSPEC,
                    std::ptr::null_mut(),
                ) as *mut vldoor_t;
                saveg_read_vldoor_t(door);
                (*door).sector.as_mut().unwrap().specialdata = &raw mut (*door).thinker;
                (*door).thinker.kind = ThinkerKind::Door;
                P_AddThinker(&raw mut (*door).thinker);
            }
            x if x == tc_floor => {
                saveg_read_pad();
                let floor = Z_Malloc(
                    std::mem::size_of::<floormove_t>() as i32,
                    PU_LEVSPEC,
                    std::ptr::null_mut(),
                ) as *mut floormove_t;
                saveg_read_floormove_t(floor);
                (*floor).sector.as_mut().unwrap().specialdata = &raw mut (*floor).thinker;
                (*floor).thinker.kind = ThinkerKind::Floor;
                P_AddThinker(&raw mut (*floor).thinker);
            }
            x if x == tc_plat => {
                saveg_read_pad();
                let plat = Z_Malloc(
                    std::mem::size_of::<plat_t>() as i32,
                    PU_LEVSPEC,
                    std::ptr::null_mut(),
                ) as *mut plat_t;
                saveg_read_plat_t(plat);
                (*plat).sector.as_mut().unwrap().specialdata = &raw mut (*plat).thinker;

                // A set function slot read as `Plat`; a null one (C keeps the
                // function null) is a platform in stasis.
                if (*plat).thinker.kind == ThinkerKind::None {
                    (*plat).thinker.kind = ThinkerKind::PlatInStasis;
                }

                P_AddThinker(&raw mut (*plat).thinker);
                P_AddActivePlat(plat);
            }
            x if x == tc_flash => {
                saveg_read_pad();
                let flash = Z_Malloc(
                    std::mem::size_of::<lightflash_t>() as i32,
                    PU_LEVSPEC,
                    std::ptr::null_mut(),
                ) as *mut lightflash_t;
                saveg_read_lightflash_t(flash);
                (*flash).thinker.kind = ThinkerKind::LightFlash;
                P_AddThinker(&raw mut (*flash).thinker);
            }
            x if x == tc_strobe => {
                saveg_read_pad();
                let strobe = Z_Malloc(
                    std::mem::size_of::<strobe_t>() as i32,
                    PU_LEVSPEC,
                    std::ptr::null_mut(),
                ) as *mut strobe_t;
                saveg_read_strobe_t(strobe);
                (*strobe).thinker.kind = ThinkerKind::Strobe;
                P_AddThinker(&raw mut (*strobe).thinker);
            }
            x if x == tc_glow => {
                saveg_read_pad();
                let glow = Z_Malloc(
                    std::mem::size_of::<glow_t>() as i32,
                    PU_LEVSPEC,
                    std::ptr::null_mut(),
                ) as *mut glow_t;
                saveg_read_glow_t(glow);
                (*glow).thinker.kind = ThinkerKind::Glow;
                P_AddThinker(&raw mut (*glow).thinker);
            }
            _ => {
                i_error!(
                    "P_UnarchiveSpecials:Unknown tclass {} in savegame",
                    tclass as i32
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use {super::*, std::sync::Mutex};

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

    /// Runs `f` with `save_stream` reading a copy of `data`, then restores
    /// the original stream and error globals on exit.
    unsafe fn with_mem_stream<F: FnOnce()>(data: &mut [u8], f: F) {
        let old_stream = std::mem::replace(stream(), Cursor::new(data.to_vec()));
        let old_error = savegame_error;
        let old_len = savegamelength;
        savegame_error = 0;
        savegamelength = 0;
        f();
        *stream() = old_stream;
        savegame_error = old_error;
        savegamelength = old_len;
    }

    /// Bytes written to `save_stream` read back in order, padding aligns
    /// to the stream position, and reading past the end sets the error.
    #[test]
    fn save_stream_round_trip() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            with_mem_stream(&mut [], || {
                *stream() = Cursor::default();
                saveg_write8(0xAB);
                saveg_write_pad();
                saveg_write32(0x1234_5678);
                assert_eq!(stream().get_ref(), &[0xAB, 0, 0, 0, 0x78, 0x56, 0x34, 0x12]);
                assert_eq!({ savegamelength }, 8);

                stream().set_position(0);
                assert_eq!(saveg_read8(), 0xAB);
                saveg_read_pad();
                assert_eq!(saveg_read32(), 0x1234_5678);
                assert_eq!({ savegame_error }, 0);
                assert_eq!(saveg_read8(), 0);
                assert_eq!({ savegame_error }, 1);
            });
        }
    }

    #[test]
    fn saveg_read_thinker_t_tests_function_slot_against_null() {
        let _g = LOCK.lock().unwrap();
        // 12 bytes: prev(4) + next(4) + function(4)
        let mut null_function = [0u8; 12];
        let mut set_function = [0u8; 12];
        set_function[8] = 0x40;
        unsafe {
            let mut th = thinker_t {
                prev: ptr::null_mut(),
                next: ptr::null_mut(),
                kind: ThinkerKind::Glow,
            };
            with_mem_stream(&mut null_function, || {
                saveg_read_thinker_t(&raw mut th, ThinkerKind::Ceiling);
            });
            assert_eq!(th.kind, ThinkerKind::None);
            with_mem_stream(&mut set_function, || {
                saveg_read_thinker_t(&raw mut th, ThinkerKind::Ceiling);
            });
            assert_eq!(th.kind, ThinkerKind::Ceiling);
        }
    }

    #[test]
    fn saved_function_word_is_null_only_without_a_think_function() {
        assert_eq!(saved_function_word(ThinkerKind::None), 0);
        assert_eq!(saved_function_word(ThinkerKind::CeilingInStasis), 0);
        assert_eq!(saved_function_word(ThinkerKind::PlatInStasis), 0);
        assert_eq!(
            saved_function_word(ThinkerKind::Ceiling),
            T_MoveCeiling as *const () as usize as u32
        );
        assert_eq!(
            saved_function_word(ThinkerKind::Mobj),
            P_MobjThinker as *const () as usize as u32
        );
        assert_ne!(saved_function_word(ThinkerKind::Plat), 0);
    }

    #[test]
    fn saveg_read_mobj_t_zero_function_slot_reads_as_no_kind() {
        let _g = LOCK.lock().unwrap();
        // mobj_t is large; we need enough bytes for the full struct read.
        // The thinker (prev+next+fn) is the first 12 bytes.
        // saveg_read_mobj_t reads many fields — pad to 256 zeros.
        let mut data = [0u8; 256];
        unsafe {
            let mut mo = std::mem::MaybeUninit::<mobj_t>::zeroed().assume_init();
            mo.thinker.kind = ThinkerKind::Mobj;
            with_mem_stream(&mut data, || {
                saveg_read_mobj_t(&raw mut mo as *mut c_void);
            });
            assert_eq!(mo.thinker.kind, ThinkerKind::None);
        }
    }

    #[test]
    fn save_game_path_not_truncated_for_long_directory() {
        let long_dir = "/very/long/savegame/directory/path/"; // 35 chars — forces path > 31 chars
        let slot = 3i32;
        let result = save_filename(long_dir, slot);
        let expected = format!("{}{}{}.dsg", long_dir, SAVEGAMENAME, slot);
        assert_eq!(
            result, expected,
            "save path was truncated for long directory"
        );
    }
}

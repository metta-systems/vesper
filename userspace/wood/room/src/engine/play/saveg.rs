//! Rust port of vendor/doomgeneric/p_saveg.c.
//!
//! Implements save-game serialization and deserialization for the entire Doom
//! game state. All active map objects (mobjs), player state, world geometry
//! deltas, thinker chains, and sector specials (ceilings, doors, floors,
//! platforms, lights) are written to or read from the in-memory
//! [`SaveGameState::save_stream`], which `g_game` loads from or writes to
//! the save file.
//!
//! The on-disk format is a flat byte stream. Multi-byte integers are always
//! little-endian regardless of host byte order. Sector and state pointers are
//! serialized as array indices; player pointers use a 1-based index (0 means
//! NULL). Thinker function slots stored in saves are raw addresses that are
//! only tested against null on load; the thinker kind is re-established from
//! the record's class (see [`saved_function_word`]).
//!
//! # Format difference from vanilla
//!
//! Room saves are not vanilla saves, and vanilla (doomgeneric) saves do not
//! load in Room:
//! - References to map objects (a map object's `target` and `tracer`, a
//!   player's `mo` and `attacker`) are written in the 32-bit words where C
//!   writes its pointers, but as save ordinals rather than addresses: the
//!   1-based position of the referenced object among the map objects
//!   [`P_ArchiveThinkers`] writes, in thinker-list order, or `0` for none
//!   (see [`mobj_save_ordinals`]). A reference to an object that is not
//!   saved (removed, or freed) is written as `0`. On load the ordinals are
//!   resolved to the recreated objects once all of them are read (see
//!   [`resolve_saved_references`]), so the references survive a save and
//!   load; C writes addresses and loads them as dangling pointers.
//! - The map objects' sector and blockmap links (`snext`, `sprev`, `bnext`,
//!   `bprev`) are written as `0`; `P_SetThingPosition` rebuilds them on
//!   load, as it does in C.
//! - The header's version field holds [`savegame_version`] (`"wood 109"`
//!   for v1.9) instead of vanilla's `"version 109"`, so the loader's version
//!   check rejects a vanilla save rather than misreading its addresses as
//!   ordinals. The field keeps its [`VERSIONSIZE`] bytes; the rest of the
//!   byte layout is vanilla's.
//!
//! Other differences from the C source:
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
        collections::HashMap,
        io::{Cursor, Read, Write},
        ptr,
    },
};

use crate::engine::{
    doom::player::{
        MAXPLAYERS, NUMAMMO, NUMCARDS, NUMPOWERS, NUMPSPRITES, NUMWEAPONS, PlayerMessage, PlayerT,
        PspdefT, TiccmdT,
    },
    game::player as game_player,
    global::Global,
    map::{SectorId, SubsectorId},
    play::{
        ceilng::{CEILINGS, MAXCEILINGS, P_AddActiveCeiling, T_MoveCeiling, ceiling_t},
        doors::{T_VerticalDoor, vldoor_t},
        floor::{T_MoveFloor, floormove_t},
        info::{MobjInfo, NUMSTATES, State, mobjinfo_ptr, state_ptr},
        lights::{
            T_FireFlicker, T_Glow, T_LightFlash, T_StrobeFlash, glow_t, lightflash_t, strobe_t,
        },
        mobj::{MobjId, mobj_t},
        plats::{P_AddActivePlat, T_PlatRaise, plat_t},
        tick::{
            P_AddThinker, P_InitThinkers, TICK, ThinkerId, ThinkerKind, leveltime, new_thinker,
            thinker_t, thinkers,
        },
    },
    wad::format::mapthing_t,
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

/// The save stream and its bookkeeping (`p_saveg.c` globals).
pub struct SaveGameState {
    /// The save game being read or written, held in memory.
    ///
    /// `G_DoLoadGame` fills it with the save file's bytes before unarchiving;
    /// `G_DoSaveGame` archives into it and then writes its bytes to the file it
    /// opened. Positions in it are file offsets, so padding is computed from
    /// [`Cursor::position`] as the C code did with `ftell`.
    pub save_stream: Cursor<Vec<u8>>,
    /// Running byte count of data written to the current save stream.
    /// Incremented by `saveg_write8`; read by `g_game.c` after saving completes.
    /// C origin: `savegamelength` in `p_saveg.c`.
    pub savegamelength: i32,
    /// Non-zero when a read or write error has occurred on `save_stream`.
    /// Set to `1` on the first I/O failure; callers check this after
    /// archiving/unarchiving to decide whether to accept the save.
    /// C origin: `savegame_error` in `p_saveg.c`.
    pub savegame_error: i32,
    /// The `attacker` save ordinal of each player [`P_UnArchivePlayers`]
    /// read (`None` for a slot it did not read), kept until
    /// [`P_UnArchiveThinkers`] has recreated the map objects it refers to.
    saved_attackers: [Option<u32>; MAXPLAYERS],
}

impl SaveGameState {
    /// No save in progress: an empty stream, no bytes written, no error.
    pub const fn new() -> Self {
        Self {
            save_stream: Cursor::new(Vec::new()),
            savegamelength: 0,
            savegame_error: 0,
            saved_attackers: [None; MAXPLAYERS],
        }
    }
}

impl Default for SaveGameState {
    fn default() -> Self {
        Self::new()
    }
}

/// The save stream and its bookkeeping.
pub static SAVEG: Global<SaveGameState> = Global::new(SaveGameState::new());

use crate::engine::{
    doom::main::DOOM_MAIN,
    game::G_VanillaVersionCode,
    misc::until_nul,
    play::{
        maputl::P_SetThingPosition,
        mobj::{P_MobjThinker, P_RemoveMobj},
        setup::LEVEL,
    },
};

// ---------------------------------------------------------------------------
// Low-level I/O on save_stream.
// ---------------------------------------------------------------------------

/// Reads one byte from `save_stream`.
///
/// Sets `savegame_error = 1` on the first short read; subsequent calls still
/// return `0` but do not double-set the flag. C origin: `saveg_read8`.
fn saveg_read8() -> u8 {
    let mut saveg = SAVEG.borrow_mut();
    let mut result = [0u8];
    let n = saveg.save_stream.read(&mut result).unwrap_or(0);
    if n < 1 && saveg.savegame_error == 0 {
        saveg.savegame_error = 1;
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
fn saveg_write8(value: u8) {
    let mut saveg = SAVEG.borrow_mut();
    let n = saveg.save_stream.write(&[value]).unwrap_or(0);
    if n < 1 && saveg.savegame_error == 0 {
        saveg.savegame_error = 1;
    }
    saveg.savegamelength += 1;
}

/// Reads a little-endian 16-bit unsigned integer from `save_stream`.
///
/// Returns the reconstructed value; any read error is recorded in
/// `savegame_error`. C origin: `saveg_read16` (which used `short`; here `u16`
/// avoids sign-extension ambiguity).
fn saveg_read16() -> u16 {
    let a = saveg_read8() as u16;
    let b = saveg_read8() as u16;
    a | (b << 8)
}

/// Writes a little-endian 16-bit unsigned integer to `save_stream`.
///
/// C origin: `saveg_write16`.
fn saveg_write16(value: u16) {
    saveg_write8((value & 0xff) as u8);
    saveg_write8(((value >> 8) & 0xff) as u8);
}

/// Reads a little-endian 32-bit unsigned integer from `save_stream`.
///
/// C origin: `saveg_read32` (which returned `int`; here `u32` to make
/// bit-pattern semantics explicit before callers cast to signed types).
fn saveg_read32() -> u32 {
    let a = saveg_read8() as u32;
    let b = saveg_read8() as u32;
    let c = saveg_read8() as u32;
    let d = saveg_read8() as u32;
    a | (b << 8) | (c << 16) | (d << 24)
}

/// Writes a little-endian 32-bit unsigned integer to `save_stream`.
///
/// C origin: `saveg_write32`.
fn saveg_write32(value: u32) {
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
fn saveg_read_pad() {
    let pos = SAVEG.borrow().save_stream.position();
    let padding = (4 - (pos & 3)) & 3;
    for _ in 0..padding {
        saveg_read8();
    }
}

/// Writes NUL padding bytes to align the stream to the next 4-byte boundary.
///
/// See `saveg_read_pad` for the alignment formula. C origin:
/// `saveg_write_pad`.
fn saveg_write_pad() {
    let pos = SAVEG.borrow().save_stream.position();
    let padding = (4 - (pos & 3)) & 3;
    for _ in 0..padding {
        saveg_write8(0);
    }
}

/// Reads a 32-bit enum value from the stream as a `u32`.
///
/// Enum values are always stored as 32-bit little-endian integers.
/// C origin: the `saveg_read_enum` macro (alias for `saveg_read32`).
fn saveg_read_enum() -> u32 {
    saveg_read32()
}

/// Writes a 32-bit enum value to the stream.
///
/// C origin: the `saveg_write_enum` macro (alias for `saveg_write32`).
fn saveg_write_enum(value: u32) {
    saveg_write32(value);
}

// ---------------------------------------------------------------------------
// Struct serialization helpers
// ---------------------------------------------------------------------------

/// Serializes a sector as its zero-based index into the level's `sectors`
/// array (C: `sector - sectors`).
fn saveg_write_sector_ptr(sector: SectorId) -> u32 {
    sector.index() as u32
}

/// Deserializes a sector index from the save stream.
///
/// `index` is an offset into the level's `sectors` array, as written by
/// `saveg_write_sector_ptr`; it is checked when the sector is used.
fn saveg_read_sector_ptr(index: u32) -> SectorId {
    SectorId::new(index as usize)
}

/// Serializes a `State` pointer as a zero-based index into the `states`
/// table.
///
/// Returns `0` for a null pointer. The C equivalent computes `state - states`;
/// this compares addresses.
fn saveg_write_state_ptr(state: *const State) -> u32 {
    if state.is_null() {
        0
    } else {
        ((state.addr() - state_ptr(0).addr()) / size_of::<State>()) as u32
    }
}

/// Deserializes a state index into a `*mut State`.
///
/// If `index >= NUMSTATES`, sets `savegame_error = 1` and clamps to
/// `states[0]` rather than accessing out-of-bounds memory. The C version
/// performs no such guard (`&states[saveg_read32()]` would silently
/// out-of-bounds).
fn saveg_read_state_ptr(index: u32) -> *mut State {
    if index as usize >= NUMSTATES {
        SAVEG.borrow_mut().savegame_error = 1;
        return state_ptr(0);
    }
    state_ptr(index as usize)
}

/// Serializes a `PlayerT` pointer as a 1-based player index.
///
/// Returns `0` for a null pointer; otherwise returns
/// `(player - players) + 1`. The 1-based encoding reserves `0` to represent
/// NULL. C origin: `str->player - players + 1`; this compares addresses.
fn saveg_write_player_ptr(player: *const PlayerT) -> u32 {
    if player.is_null() {
        0
    } else {
        ((player.addr() - game_player(0).addr()) / size_of::<PlayerT>()) as u32 + 1
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
        game_player(0).add((value - 1) as usize)
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
        ThinkerKind::Removed => return u32::MAX,
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
/// The `prev` and `next` words are list links from the saving session; the
/// thinker list here is [`TICK`]'s ordered list, so they are read and
/// dropped (C overwrites them when it relinks the thinker). The saved
/// function slot is only tested against null, as in C: `0` reads as
/// [`ThinkerKind::None`], anything else as `kind_if_function_set`. The
/// `P_UnArchive*` callers then settle the kind (C overwrites the function
/// the same way).
/// C origin: `saveg_read_thinker_t`.
fn saveg_read_thinker_t(th: &mut thinker_t, kind_if_function_set: ThinkerKind) {
    // prev, next
    saveg_read32();
    saveg_read32();
    th.kind = if saveg_read32() == 0 {
        ThinkerKind::None
    } else {
        kind_if_function_set
    };
}

/// Writes a `thinker_t` header (prev, next, function) to the save stream.
///
/// The thinker list keeps no links in the thinker, so `prev` and `next` are
/// written as `0`; C writes the link addresses, which its loader ignores.
/// The function slot is written by [`saved_function_word`].
/// C origin: `saveg_write_thinker_t`.
fn saveg_write_thinker_t(th: &thinker_t) {
    saveg_write32(0);
    saveg_write32(0);
    saveg_write32(saved_function_word(th.kind));
}

/// The map object `thinker` is, if [`P_ArchiveThinkers`] saves it: a listed
/// map object that is not removed.
fn saved_mobj(thinker: ThinkerId) -> Option<MobjId> {
    match thinker {
        ThinkerId::Mobj(mobj) if thinker.kind() == ThinkerKind::Mobj => Some(mobj),
        _ => None,
    }
}

/// The save ordinal of each map object [`P_ArchiveThinkers`] writes: its
/// 1-based position among the saved map objects, in thinker-list order.
fn mobj_save_ordinals() -> HashMap<MobjId, u32> {
    thinkers().filter_map(saved_mobj).zip(1..).collect()
}

/// The 32-bit word written for a map object reference: the referenced
/// object's save ordinal (see [`mobj_save_ordinals`]), or `0` for none.
///
/// A reference to an object that is not saved (removed and awaiting its
/// free, or a stale handle to a freed one) is written as `0` too, so it
/// loads as `None`.
fn saved_mobj_ordinal(ordinals: &HashMap<MobjId, u32>, mobj: Option<MobjId>) -> u32 {
    mobj.and_then(|mobj| ordinals.get(&mobj).copied())
        .unwrap_or(0)
}

/// The map object recreated for save ordinal `ordinal`, given the objects
/// in the order they were read; `None` for `0` or an ordinal past the end.
fn mobj_for_saved_ordinal(loaded_mobjs: &[MobjId], ordinal: u32) -> Option<MobjId> {
    let position = (ordinal as usize).checked_sub(1)?;
    loaded_mobjs.get(position).copied()
}

/// The save ordinals a map object record refers to, resolved by
/// [`resolve_saved_references`] once every map object is read.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SavedMobjReferences {
    /// Save ordinal of `target`.
    target: u32,
    /// Save ordinal of `tracer`.
    tracer: u32,
}

/// Points the references read as save ordinals at the recreated objects:
/// `target` and `tracer` of each object in `loaded_mobjs` (paired with the
/// ordinals its record held) and the `attacker` of each player
/// [`P_UnArchivePlayers`] read. An ordinal that names no recreated object
/// gives `None`.
///
/// # Safety
///
/// The handles in `loaded_mobjs` must be live map objects.
unsafe fn resolve_saved_references(loaded_mobjs: &[(MobjId, SavedMobjReferences)]) {
    let recreated: Vec<MobjId> = loaded_mobjs.iter().map(|&(mobj, _)| mobj).collect();
    for &(mobj, references) in loaded_mobjs {
        let mo = mobj.ptr();
        (*mo).target = mobj_for_saved_ordinal(&recreated, references.target);
        (*mo).tracer = mobj_for_saved_ordinal(&recreated, references.tracer);
    }

    let saved_attackers = std::mem::take(&mut SAVEG.borrow_mut().saved_attackers);
    for (index, attacker) in saved_attackers.into_iter().enumerate() {
        if let Some(ordinal) = attacker {
            (*game_player(index)).attacker = mobj_for_saved_ordinal(&recreated, ordinal);
        }
    }
}

//
// mobj_t — serialized as C struct via FFI
//

/// Reads a full `mobj_t` record from the save stream into the map object
/// `mobj_id`.
///
/// After reading:
/// - The thinker's saved list links are dropped (see
///   [`saveg_read_thinker_t`]); the caller lists it with `P_AddThinker`.
/// - `snext`, `sprev`, `bnext`, `bprev` are dropped and rebuilt by
///   `P_SetThingPosition`.
/// - `target` and `tracer` are save ordinals; they load as `None` and are
///   returned for [`resolve_saved_references`] to set once every map object
///   is read.
/// - `subsector` is saved as an address; it is skipped and left as
///   subsector 0 until `P_SetThingPosition` finds the real one.
/// - `state` is resolved from a stream index via `saveg_read_state_ptr`.
/// - `player` is a 1-based index; re-resolved and the back-reference
///   `player->mo` is set to `mobj_id`.
/// - `info` is a raw saved pointer (will be overwritten by caller with
///   `&mobjinfo[type]`).
/// C origin: `saveg_read_mobj_t`.
unsafe fn saveg_read_mobj_t(mobj_id: MobjId) -> SavedMobjReferences {
    let mo = mobj_id.ptr();

    // thinker_t
    saveg_read_thinker_t(&mut (*mo).thinker, ThinkerKind::Mobj);

    // x, y, z
    (*mo).x = saveg_read32() as i32;
    (*mo).y = saveg_read32() as i32;
    (*mo).z = saveg_read32() as i32;

    // snext, sprev (rebuilt by `P_SetThingPosition`)
    saveg_read32();
    saveg_read32();
    (*mo).snext = None;
    (*mo).sprev = None;

    // angle, sprite, frame
    (*mo).angle = saveg_read32();
    (*mo).sprite = saveg_read_enum() as i32;
    (*mo).frame = saveg_read32() as i32;

    // bnext, bprev (rebuilt by `P_SetThingPosition`)
    saveg_read32();
    saveg_read32();
    (*mo).bnext = None;
    (*mo).bprev = None;

    // subsector (a saved address, rebuilt by `P_SetThingPosition`)
    saveg_read32();
    (*mo).subsector = SubsectorId::new(0);

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

    // target (a save ordinal, resolved once every map object is read)
    let target = saveg_read32();
    (*mo).target = None;

    // reactiontime, threshold
    (*mo).reactiontime = saveg_read32() as i32;
    (*mo).threshold = saveg_read32() as i32;

    // player (index + 1)
    let pl = saveg_read32();
    if pl > 0 {
        (*mo).player = saveg_read_player_ptr(pl);
        let player = (*mo).player;
        if !player.is_null() {
            (*player).mo = Some(mobj_id);
        }
    } else {
        (*mo).player = std::ptr::null_mut();
    }

    // lastlook
    (*mo).lastlook = saveg_read32() as i32;

    // spawnpoint
    saveg_read_mapthing_t(std::ptr::addr_of_mut!((*mo).spawnpoint));

    // tracer (a save ordinal, resolved once every map object is read)
    let tracer = saveg_read32();
    (*mo).tracer = None;

    SavedMobjReferences { target, tracer }
}

/// Writes a full `mobj_t` record to the save stream from `mobj`.
///
/// `target` and `tracer` are written as save ordinals looked up in
/// `ordinals` (see [`saved_mobj_ordinal`]). `snext`, `sprev`, `bnext` and
/// `bprev` are written as `0`, as the loader rebuilds them. `subsector` and
/// `info` are written as 32-bit addresses, as C writes its pointers; they are
/// not portable across sessions and are reconstructed on load. `state` is
/// written as an index into `states`. `player` is written as a 1-based player
/// index (0 for NULL).
/// C origin: `saveg_write_mobj_t`.
unsafe fn saveg_write_mobj_t(mobj: MobjId, ordinals: &HashMap<MobjId, u32>) {
    let mo = mobj.ptr() as *const mobj_t;

    // thinker_t
    saveg_write_thinker_t(&(*mo).thinker);

    // x, y, z
    saveg_write32((*mo).x as u32);
    saveg_write32((*mo).y as u32);
    saveg_write32((*mo).z as u32);

    // snext, sprev (rebuilt on load)
    saveg_write32(0);
    saveg_write32(0);

    // angle, sprite, frame
    saveg_write32((*mo).angle);
    saveg_write_enum((*mo).sprite as u32);
    saveg_write32((*mo).frame as u32);

    // bnext, bprev (rebuilt on load)
    saveg_write32(0);
    saveg_write32(0);

    // subsector (C writes the pointer: the subsector record's address)
    let subsector_address = (&raw const LEVEL.borrow().subsectors[(*mo).subsector]).addr();
    saveg_write32(subsector_address as u32);

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

    // target (save ordinal)
    saveg_write32(saved_mobj_ordinal(ordinals, (*mo).target));

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

    // tracer (save ordinal)
    saveg_write32(saved_mobj_ordinal(ordinals, (*mo).tracer));
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
/// - `mo` is a save ordinal; it loads as `None` and is set again by
///   `P_UnArchiveThinkers` when the player's map object is read.
/// - `playerstate` and enum fields (`readyweapon`, `pendingweapon`) are read
///   as 32-bit values.
/// - Power timers, key cards, frag counts, weapon ownership, ammo, and max
///   ammo are each 32-bit entries.
/// - `message` is a saved address; it loads as empty.
/// - `attacker` is a save ordinal; it loads as `None` and is returned for
///   [`resolve_saved_references`] to set once every map object is read.
/// - Each of `NUMPSPRITES` player-sprite slots is read via
///   `saveg_read_pspdef_t`.
/// C origin: `saveg_read_player_t`.
unsafe fn saveg_read_player_t(pl: *mut PlayerT) -> u32 {
    let s = &mut *pl;

    // mo (a save ordinal; set when the player's map object is read)
    saveg_read32();
    s.mo = None;

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

    // attacker (a save ordinal, resolved once every map object is read)
    let attacker = saveg_read32();
    s.attacker = None;

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

    attacker
}

/// Writes a full `player_t` record to the save stream from `*pl`.
///
/// `mo` and `attacker` are written as save ordinals looked up in `ordinals`
/// (see [`saved_mobj_ordinal`]); the `message` slot is written as `0`. All
/// other fields mirror `saveg_read_player_t` in order.
/// C origin: `saveg_write_player_t`.
unsafe fn saveg_write_player_t(pl: *const PlayerT, ordinals: &HashMap<MobjId, u32>) {
    let s = &*pl;

    saveg_write32(saved_mobj_ordinal(ordinals, s.mo));
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
    saveg_write32(saved_mobj_ordinal(ordinals, s.attacker));
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
/// The sector index becomes a sector id via `saveg_read_sector_ptr`.
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
/// The sector is serialized as an index via `saveg_write_sector_ptr`.
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
    s.texture_upper_half = 0;
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
pub fn P_TempSaveGameFile() -> String {
    format!("{}temp.dsg", DOOM_MAIN.borrow().savegamedir)
}

/// Returns the path to the save-game file for the given `slot` (0-7).
///
/// Called by `g_game.c` to open the final save file for reading or renaming.
pub fn P_SaveGameFile(slot: i32) -> String {
    save_filename(&DOOM_MAIN.borrow().savegamedir, slot)
}

/// The header's version field for game version code `version_code` (see
/// `G_VanillaVersionCode`): `"wood N"`, NUL-padded to [`VERSIONSIZE`] bytes.
///
/// Vanilla writes `"version N"`. Room's save format differs from vanilla's
/// (map object references are save ordinals, see the module docs), so it
/// carries its own string: the version check in [`P_ReadSaveGameHeader`]
/// then rejects vanilla saves, and vanilla rejects Room's, instead of either
/// misreading the other's references. `N` keeps saves of different game
/// versions apart, as in vanilla.
fn savegame_version(version_code: i32) -> [u8; VERSIONSIZE] {
    let version_string = format!("wood {version_code}");
    let mut version = [0u8; VERSIONSIZE];
    // Keep at least one NUL, as vanilla's strcmp of the field expects.
    let length = version_string.len().min(VERSIONSIZE - 1);
    version[..length].copy_from_slice(&version_string.as_bytes()[..length]);
    version
}

/// Writes the save-game file header to the open `save_stream`.
///
/// The header layout (bytes written in order):
/// 1. `description` string, NUL-padded to `SAVESTRINGSIZE` (24) bytes.
/// 2. Version string [`savegame_version`] (`"wood N"`, vanilla writes
///    `"version N"`), NUL-padded to `VERSIONSIZE` (16) bytes.
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
    for byte in savegame_version(G_VanillaVersionCode()) {
        saveg_write8(byte);
    }

    // Write skill, episode, map
    saveg_write8(gameskill() as u8);
    saveg_write8(gameepisode() as u8);
    saveg_write8(gamemap() as u8);

    // Write playeringame
    for i in 0..MAXPLAYERS {
        saveg_write8(playeringame(i) as u8);
    }

    // Write leveltime (3 bytes, big-endian)
    let lt = leveltime() as u32;
    saveg_write8(((lt >> 16) & 0xff) as u8);
    saveg_write8(((lt >> 8) & 0xff) as u8);
    saveg_write8((lt & 0xff) as u8);
}

/// Reads and validates the save-game header from `save_stream`.
///
/// Returns `1` (true) on success, `0` (false) if the version string is not
/// [`savegame_version`] of `G_VanillaVersionCode()`: a save of another game
/// version, or a vanilla save (whose format Room no longer reads). On
/// success, `gameskill`, `gameepisode`, `gamemap`, `playeringame`, and
/// `leveltime` are updated from the stream.
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
    read_save_game_header(G_VanillaVersionCode())
}

/// [`P_ReadSaveGameHeader`] for game version code `version_code`.
fn read_save_game_header(version_code: i32) -> i32 {
    // Skip description (SAVESTRINGSIZE bytes)
    for _ in 0..SAVESTRINGSIZE {
        saveg_read8();
    }

    // Read version string
    let mut read_vcheck = [0u8; VERSIONSIZE];
    for byte in &mut read_vcheck {
        *byte = saveg_read8();
    }

    // Compare version (the whole NUL-padded field)
    if read_vcheck != savegame_version(version_code) {
        return 0; // bad version
    }

    // Read skill, episode, map
    let gameskill = saveg_read8() as i32;
    let gameepisode = saveg_read8() as i32;
    let gamemap = saveg_read8() as i32;
    {
        let mut game = GAME.borrow_mut();
        game.gameskill = gameskill;
        game.gameepisode = gameepisode;
        game.gamemap = gamemap;
    }

    // Read playeringame
    for i in 0..MAXPLAYERS {
        let in_game = saveg_read8() as i32;
        GAME.borrow_mut().playeringame[i] = in_game;
    }

    // Read leveltime (3 bytes, big-endian)
    let a = saveg_read8() as u32;
    let b = saveg_read8() as u32;
    let c = saveg_read8() as u32;
    TICK.borrow_mut().leveltime = ((a << 16) | (b << 8) | c) as i32;

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
    if value == SAVEGAME_EOF { 1 } else { 0 }
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

use crate::engine::game::{GAME, gameepisode, gamemap, gameskill, playeringame};

// ---------------------------------------------------------------------------
// P_ArchivePlayers / P_UnArchivePlayers
// ---------------------------------------------------------------------------

/// Serializes all active players to `save_stream`.
///
/// Iterates over `players[0..MAXPLAYERS]`; skips slots where
/// `playeringame[i] == 0`. Each active player record is 4-byte aligned
/// (`saveg_write_pad`) then written by `saveg_write_player_t`, with its map
/// object references as save ordinals in the current thinker list, the one
/// `P_ArchiveThinkers` then writes.
/// Called by `g_game.c` (`G_DoSaveGame`).
///
/// # Safety
///
/// `save_stream` must be the stream being written.
/// The `players` array and `playeringame` flags must be fully initialized for
/// all `MAXPLAYERS` slots. Must be called after `P_WriteSaveGameHeader` and
/// before `P_WriteSaveGameEOF`.
pub unsafe fn P_ArchivePlayers() {
    let ordinals = mobj_save_ordinals();
    for i in 0..MAXPLAYERS {
        if playeringame(i) == 0 {
            continue;
        }
        saveg_write_pad();
        saveg_write_player_t(&*game_player(i), &ordinals);
    }
}

/// Deserializes all active players from `save_stream`.
///
/// For each active player slot, aligns the stream (`saveg_read_pad`) then
/// reads the player record. After reading, `mo`, `message`, and `attacker` are
/// empty; `mo` is restored when `P_UnArchiveThinkers` reads the player's map
/// object, and `attacker` once it has recreated every map object (the save
/// ordinal is kept in [`SaveGameState`] until then; C loads a dangling
/// pointer).
/// Called by `g_game.c` (`G_DoLoadGame`).
///
/// # Safety
///
/// `save_stream` must be positioned at the byte
/// sequence written by `P_ArchivePlayers`. `playeringame` must already have
/// been populated by `P_ReadSaveGameHeader` so the active-slot bitmask is
/// correct. After this call, `players[i].mo`, `.message`, and `.attacker` are
/// empty until thinkers are unarchived.
pub unsafe fn P_UnArchivePlayers() {
    let mut saved_attackers = [None; MAXPLAYERS];
    for i in 0..MAXPLAYERS {
        if playeringame(i) == 0 {
            continue;
        }
        saveg_read_pad();
        let player = &mut *game_player(i);
        saved_attackers[i] = Some(saveg_read_player_t(player));

        // will be set when unarc thinker
        player.mo = None;
        player.message = PlayerMessage::NONE;
        player.attacker = None;
    }
    SAVEG.borrow_mut().saved_attackers = saved_attackers;
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
/// Called by `g_game.c` (`G_DoSaveGame`), with `save_stream` being the
/// stream written and a level loaded.
pub fn P_ArchiveWorld() {
    let level = LEVEL.borrow();

    // do sectors
    for sec in &level.sectors[..level.numsectors as usize] {
        saveg_write16((sec.floorheight >> 16) as u16);
        saveg_write16((sec.ceilingheight >> 16) as u16);
        saveg_write16(sec.floorpic as u16);
        saveg_write16(sec.ceilingpic as u16);
        saveg_write16(sec.lightlevel as u16);
        saveg_write16(sec.special as u16);
        saveg_write16(sec.tag as u16);
    }

    // do lines
    for li in &level.lines {
        saveg_write16(li.flags as u16);
        saveg_write16(li.special as u16);
        saveg_write16(li.tag as u16);
        for j in 0..2 {
            if li.sidenum[j] == -1 {
                continue;
            }
            let si = &level.sides[li.sidenum[j] as usize];
            saveg_write16((si.textureoffset >> 16) as u16);
            saveg_write16((si.rowoffset >> 16) as u16);
            saveg_write16(si.toptexture as u16);
            saveg_write16(si.bottomtexture as u16);
            saveg_write16(si.midtexture as u16);
        }
    }
}

/// Deserializes all map sector and sidedef deltas from `save_stream`.
///
/// For each sector: reads heights as signed 16-bit integers and shifts them
/// left 16 bits to restore the 16.16 fixed-point format. Clears
/// `specialdata` and `soundtarget` to `None` (they will be rebuilt by
/// `P_UnArchiveSpecials` and the sound code respectively).
///
/// For each linedef: reads flags, special, tag, then each present sidedef's
/// texture offsets (shifted left 16 to restore 16.16) and texture indices.
///
/// Called by `g_game.c` (`G_DoLoadGame`), with `save_stream` positioned at
/// the byte sequence written by `P_ArchiveWorld` and the saved level loaded.
pub fn P_UnArchiveWorld() {
    let mut level = LEVEL.borrow_mut();
    let level = &mut *level;
    let numsectors = level.numsectors as usize;

    // do sectors
    for sec in &mut level.sectors[..numsectors] {
        sec.floorheight = (saveg_read16() as i16 as i32) << 16;
        sec.ceilingheight = (saveg_read16() as i16 as i32) << 16;
        sec.floorpic = saveg_read16() as i16;
        sec.ceilingpic = saveg_read16() as i16;
        sec.lightlevel = saveg_read16() as i16;
        sec.special = saveg_read16() as i16;
        sec.tag = saveg_read16() as i16;
        sec.specialdata = None;
        sec.soundtarget = None;
    }

    // do lines
    for li in &mut level.lines {
        li.flags = saveg_read16() as i16;
        li.special = saveg_read16() as i16;
        li.tag = saveg_read16() as i16;
        for j in 0..2 {
            if li.sidenum[j] == -1 {
                continue;
            }
            let si = &mut level.sides[li.sidenum[j] as usize];
            si.textureoffset = (saveg_read16() as i16 as i32) << 16;
            si.rowoffset = (saveg_read16() as i16 as i32) << 16;
            si.toptexture = saveg_read16() as i16;
            si.bottomtexture = saveg_read16() as i16;
            si.midtexture = saveg_read16() as i16;
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

/// Serializes all `mobj_t` thinkers from the thinker list.
///
/// Walks the list in run order; for each [`ThinkerId::Mobj`] of kind
/// [`ThinkerKind::Mobj`], writes a `tc_mobj` byte, 4-byte alignment padding,
/// then the full `mobj_t` record. Other thinkers are silently skipped (the C
/// version also skips them without error). Terminates the list with a
/// `tc_end` byte. The `n`th record written is the map object with save
/// ordinal `n` (see [`mobj_save_ordinals`]).
///
/// Called by `g_game.c` (`G_DoSaveGame`).
///
/// # Safety
///
/// `save_stream` must be the stream being written, and a level must be
/// loaded (each map object's subsector is written by address).
pub unsafe fn P_ArchiveThinkers() {
    let ordinals = mobj_save_ordinals();
    for mobj in thinkers().filter_map(saved_mobj) {
        saveg_write8(tc_mobj);
        saveg_write_pad();
        saveg_write_mobj_t(mobj, &ordinals);
    }

    // terminating marker
    saveg_write8(tc_end);
}

/// Clears all existing thinkers then deserializes the thinker list from
/// `save_stream`.
///
/// First pass: walks the existing thinker list; map objects whose kind is
/// [`ThinkerKind::Mobj`] are removed via `P_RemoveMobj` (unlinking them from
/// the map and their sounds), the others are dropped. Then `P_InitThinkers`
/// empties the list and frees every thinker (C frees the non-mobjs with
/// `Z_Free` and leaves the removed mobjs to the level's zone purge).
///
/// Second pass: reads thinker-class bytes from the stream in a loop.
/// - `tc_end`: resolves the references saved as save ordinals (the map
///   objects' `target` and `tracer`, the players' `attacker`) to the
///   recreated objects with [`resolve_saved_references`], and returns.
/// - `tc_mobj`: puts a zeroed `mobj_t` in the mobj arena, reads its fields
///   (`target` and `tracer` stay `None` until `tc_end`), calls
///   `P_SetThingPosition`,
///   rebuilds `info` from `mobjinfo[type]`, recomputes `floorz`/`ceilingz`
///   from the subsector's sector, sets the thinker kind to
///   [`ThinkerKind::Mobj`], and registers it with `P_AddThinker`.
/// - Any other byte: calls `i_error!` (fatal).
///
/// Called by `g_game.c` (`G_DoLoadGame`).
///
/// # Safety
///
/// `save_stream` must be positioned at the byte sequence written by
/// `P_ArchiveThinkers`. The map geometry (`sectors`, block-map, etc.) must
/// be loaded so that `P_SetThingPosition` can place each restored mobj.
/// Must be called before `P_UnArchiveSpecials` and before any code reads
/// `players[i].mo`.
pub unsafe fn P_UnArchiveThinkers() {
    // remove all current thinkers
    for thinker in thinkers() {
        if let ThinkerId::Mobj(mobj) = thinker
            && thinker.kind() == ThinkerKind::Mobj
        {
            P_RemoveMobj(mobj);
        }
    }

    P_InitThinkers();

    // read saved thinkers, the n-th map object read having save ordinal n
    let mut loaded_mobjs = Vec::new();
    loop {
        let tclass = saveg_read8();
        match tclass {
            x if x == tc_end => {
                resolve_saved_references(&loaded_mobjs);
                return;
            }
            x if x == tc_mobj => {
                saveg_read_pad();
                let mobj_id = new_thinker::<mobj_t>();
                let references = saveg_read_mobj_t(mobj_id);
                loaded_mobjs.push((mobj_id, references));

                let mo = mobj_id.ptr();

                P_SetThingPosition(mobj_id);

                // rebuild info from mobjinfo
                (*mo).info = mobjinfo_ptr((*mo).kind as usize);

                // rebuild floorz/ceilingz from subsector
                {
                    let level = LEVEL.borrow();
                    let sector = level.subsector_sector((*mo).subsector);
                    (*mo).floorz = sector.floorheight;
                    (*mo).ceilingz = sector.ceilingheight;
                }

                // set thinker kind
                (*mo).thinker.kind = ThinkerKind::Mobj;

                P_AddThinker(ThinkerId::Mobj(mobj_id));
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

/// Serializes all sector-special thinkers from the thinker list.
///
/// Walks the thinker list and identifies specials by their thinker kind:
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
/// `save_stream` must be the stream being written, and every listed special
/// must belong to the loaded level.
pub unsafe fn P_ArchiveSpecials() {
    for thinker in thinkers() {
        match (thinker, thinker.kind()) {
            // C's null function: a ceiling in stasis is still saved when it
            // is in `activeceilings`. A platform in stasis is not saved at
            // all (vanilla behaviour).
            (ThinkerId::Ceiling(ceiling), ThinkerKind::None | ThinkerKind::CeilingInStasis) => {
                let activeceilings = CEILINGS.borrow().activeceilings;
                if activeceilings
                    .iter()
                    .take(MAXCEILINGS)
                    .any(|&active| active == Some(ceiling))
                {
                    saveg_write8(tc_ceiling);
                    saveg_write_pad();
                    saveg_write_ceiling_t(ceiling.ptr());
                }
            }
            (ThinkerId::Ceiling(ceiling), ThinkerKind::Ceiling) => {
                saveg_write8(tc_ceiling);
                saveg_write_pad();
                saveg_write_ceiling_t(ceiling.ptr());
            }
            (ThinkerId::Door(door), ThinkerKind::Door) => {
                saveg_write8(tc_door);
                saveg_write_pad();
                saveg_write_vldoor_t(door.ptr());
            }
            (ThinkerId::Floor(floor), ThinkerKind::Floor) => {
                saveg_write8(tc_floor);
                saveg_write_pad();
                saveg_write_floormove_t(floor.ptr());
            }
            (ThinkerId::Plat(plat), ThinkerKind::Plat) => {
                saveg_write8(tc_plat);
                saveg_write_pad();
                saveg_write_plat_t(plat.ptr());
            }
            (ThinkerId::LightFlash(flash), ThinkerKind::LightFlash) => {
                saveg_write8(tc_flash);
                saveg_write_pad();
                saveg_write_lightflash_t(flash.ptr());
            }
            (ThinkerId::Strobe(strobe), ThinkerKind::Strobe) => {
                saveg_write8(tc_strobe);
                saveg_write_pad();
                saveg_write_strobe_t(strobe.ptr());
            }
            (ThinkerId::Glow(glow), ThinkerKind::Glow) => {
                saveg_write8(tc_glow);
                saveg_write_pad();
                saveg_write_glow_t(glow.ptr());
            }
            // Map objects are saved by `P_ArchiveThinkers`; fire flickers and
            // platforms in stasis are not saved (vanilla behaviour); removed
            // thinkers are skipped.
            _ => {}
        }
    }

    saveg_write8(tc_endspecials);
}

/// Deserializes all sector-special thinkers from `save_stream`.
///
/// Reads thinker-class bytes in a loop until `tc_endspecials`. Each record
/// is read into a zeroed thinker put in its type's arena:
/// - `tc_ceiling`: `ceiling_t`; links `sector->specialdata`, restores
///   `T_MoveCeiling` if the function slot was non-null, registers with
///   `P_AddThinker` and `P_AddActiveCeiling`.
/// - `tc_door`: `vldoor_t`; links `sector->specialdata`, unconditionally
///   assigns `T_VerticalDoor`, registers with `P_AddThinker`.
/// - `tc_floor`: `floormove_t`; links `sector->specialdata`, assigns
///   `T_MoveFloor`, registers.
/// - `tc_plat`: `plat_t`; links `sector->specialdata`, restores
///   `T_PlatRaise` if function non-null, registers with `P_AddThinker` and
///   `P_AddActivePlat`.
/// - `tc_flash`: `lightflash_t`; assigns `T_LightFlash`, registers.
/// - `tc_strobe`: `strobe_t`; assigns `T_StrobeFlash`, registers.
/// - `tc_glow`: `glow_t`; assigns `T_Glow`, registers.
/// - Any other byte: calls `i_error!` (fatal).
///
/// Called by `g_game.c` (`G_DoLoadGame`).
///
/// # Safety
///
/// `save_stream` must be positioned at the byte
/// sequence written by `P_ArchiveSpecials`. The `sectors` array must be
/// fully initialized so that `sector->specialdata` can be set. Must be
/// called after `P_UnArchiveThinkers` so the thinker list is in a
/// consistent state before new specials are registered with `P_AddThinker`.
pub unsafe fn P_UnArchiveSpecials() {
    loop {
        let tclass = saveg_read8();
        match tclass {
            x if x == tc_endspecials => return,
            x if x == tc_ceiling => {
                saveg_read_pad();
                let ceiling_id = new_thinker::<ceiling_t>();
                let ceiling = ceiling_id.ptr();
                saveg_read_ceiling_t(ceiling);
                LEVEL.borrow_mut().sectors[(*ceiling).sector].specialdata =
                    Some(ThinkerId::Ceiling(ceiling_id));

                // A set function slot read as `Ceiling`; a null one (C keeps
                // the function null) is a ceiling in stasis.
                if (*ceiling).thinker.kind == ThinkerKind::None {
                    (*ceiling).thinker.kind = ThinkerKind::CeilingInStasis;
                }

                P_AddThinker(ThinkerId::Ceiling(ceiling_id));
                P_AddActiveCeiling(ceiling_id);
            }
            x if x == tc_door => {
                saveg_read_pad();
                let door_id = new_thinker::<vldoor_t>();
                let door = door_id.ptr();
                saveg_read_vldoor_t(door);
                LEVEL.borrow_mut().sectors[(*door).sector].specialdata =
                    Some(ThinkerId::Door(door_id));
                (*door).thinker.kind = ThinkerKind::Door;
                P_AddThinker(ThinkerId::Door(door_id));
            }
            x if x == tc_floor => {
                saveg_read_pad();
                let floor_id = new_thinker::<floormove_t>();
                let floor = floor_id.ptr();
                saveg_read_floormove_t(floor);
                LEVEL.borrow_mut().sectors[(*floor).sector].specialdata =
                    Some(ThinkerId::Floor(floor_id));
                (*floor).thinker.kind = ThinkerKind::Floor;
                P_AddThinker(ThinkerId::Floor(floor_id));
            }
            x if x == tc_plat => {
                saveg_read_pad();
                let plat_id = new_thinker::<plat_t>();
                let plat = plat_id.ptr();
                saveg_read_plat_t(plat);
                LEVEL.borrow_mut().sectors[(*plat).sector].specialdata =
                    Some(ThinkerId::Plat(plat_id));

                // A set function slot read as `Plat`; a null one (C keeps the
                // function null) is a platform in stasis.
                if (*plat).thinker.kind == ThinkerKind::None {
                    (*plat).thinker.kind = ThinkerKind::PlatInStasis;
                }

                P_AddThinker(ThinkerId::Plat(plat_id));
                P_AddActivePlat(plat_id);
            }
            x if x == tc_flash => {
                saveg_read_pad();
                let flash_id = new_thinker::<lightflash_t>();
                let flash = flash_id.ptr();
                saveg_read_lightflash_t(flash);
                (*flash).thinker.kind = ThinkerKind::LightFlash;
                P_AddThinker(ThinkerId::LightFlash(flash_id));
            }
            x if x == tc_strobe => {
                saveg_read_pad();
                let strobe_id = new_thinker::<strobe_t>();
                let strobe = strobe_id.ptr();
                saveg_read_strobe_t(strobe);
                (*strobe).thinker.kind = ThinkerKind::Strobe;
                P_AddThinker(ThinkerId::Strobe(strobe_id));
            }
            x if x == tc_glow => {
                saveg_read_pad();
                let glow_id = new_thinker::<glow_t>();
                let glow = glow_id.ptr();
                saveg_read_glow_t(glow);
                (*glow).thinker.kind = ThinkerKind::Glow;
                P_AddThinker(ThinkerId::Glow(glow_id));
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
    use {
        super::*,
        crate::engine::play::{
            info::{MF_NOBLOCKMAP, MF_NOSECTOR},
            tick::P_RemoveThinker,
        },
        std::sync::Mutex,
    };

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
        // The version string keeps a terminating NUL in the field.
        assert_eq!(savegame_version(109)[VERSIONSIZE - 1], 0);
        assert_eq!(savegame_version(i32::MIN)[VERSIONSIZE - 1], 0);
    }

    #[test]
    fn save_string_size() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(SAVESTRINGSIZE, 24);
    }

    /// Runs `f` with `save_stream` reading a copy of `data`, then restores
    /// the original stream and error globals on exit.
    fn with_mem_stream<F: FnOnce()>(data: &mut [u8], f: F) {
        let old_state = std::mem::replace(
            &mut *SAVEG.borrow_mut(),
            SaveGameState {
                save_stream: Cursor::new(data.to_vec()),
                ..SaveGameState::new()
            },
        );
        f();
        *SAVEG.borrow_mut() = old_state;
    }

    /// Bytes written to `save_stream` read back in order, padding aligns
    /// to the stream position, and reading past the end sets the error.
    #[test]
    fn save_stream_round_trip() {
        let _g = LOCK.lock().unwrap();
        with_mem_stream(&mut [], || {
            saveg_write8(0xAB);
            saveg_write_pad();
            saveg_write32(0x1234_5678);
            {
                let saveg = SAVEG.borrow();
                assert_eq!(
                    saveg.save_stream.get_ref(),
                    &[0xAB, 0, 0, 0, 0x78, 0x56, 0x34, 0x12]
                );
                assert_eq!(saveg.savegamelength, 8);
            }

            SAVEG.borrow_mut().save_stream.set_position(0);
            assert_eq!(saveg_read8(), 0xAB);
            saveg_read_pad();
            assert_eq!(saveg_read32(), 0x1234_5678);
            assert_eq!(SAVEG.borrow().savegame_error, 0);
            assert_eq!(saveg_read8(), 0);
            assert_eq!(SAVEG.borrow().savegame_error, 1);
        });
    }

    #[test]
    fn saveg_read_thinker_t_tests_function_slot_against_null() {
        let _g = LOCK.lock().unwrap();
        // 12 bytes: prev(4) + next(4) + function(4)
        let mut null_function = [0u8; 12];
        let mut set_function = [0u8; 12];
        set_function[8] = 0x40;
        let mut th = thinker_t {
            kind: ThinkerKind::Glow,
        };
        with_mem_stream(&mut null_function, || {
            saveg_read_thinker_t(&mut th, ThinkerKind::Ceiling);
        });
        assert_eq!(th.kind, ThinkerKind::None);
        with_mem_stream(&mut set_function, || {
            saveg_read_thinker_t(&mut th, ThinkerKind::Ceiling);
        });
        assert_eq!(th.kind, ThinkerKind::Ceiling);
    }

    /// The thinker header keeps C's layout: two zero link words, then the
    /// function word.
    #[test]
    fn saveg_write_thinker_t_writes_zero_links_and_the_function_word() {
        let _g = LOCK.lock().unwrap();
        with_mem_stream(&mut [], || {
            saveg_write_thinker_t(&thinker_t {
                kind: ThinkerKind::None,
            });
            assert_eq!(SAVEG.borrow().save_stream.get_ref(), &[0u8; 12]);
        });
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
        // Non-zero words in snext, sprev, bnext, bprev, target and tracer
        // (byte offsets in the record); the function slot stays zero.
        for offset in [24, 28, 44, 48, 120, 150] {
            data[offset..offset + 4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        }
        let _thinker_guard = crate::engine::play::tick::THINKER_TEST_LOCK.lock().unwrap();
        let mobj = new_thinker::<mobj_t>();
        unsafe {
            let mo = mobj.ptr();
            (*mo).thinker.kind = ThinkerKind::Mobj;
            let mut references = None;
            with_mem_stream(&mut data, || {
                references = Some(saveg_read_mobj_t(mobj));
            });
            assert_eq!((*mo).thinker.kind, ThinkerKind::None);
            // The links load empty; target and tracer come back as save
            // ordinals for `resolve_saved_references`.
            assert_eq!(
                references,
                Some(SavedMobjReferences {
                    target: 0x1234_5678,
                    tracer: 0x1234_5678,
                })
            );
            assert_eq!((*mo).target, None);
            assert_eq!((*mo).tracer, None);
            assert_eq!((*mo).snext, None);
            assert_eq!((*mo).bprev, None);
        }
    }

    /// A listed map object with `health` as its tag, for telling the
    /// recreated objects apart.
    fn add_mobj(health: i32) -> MobjId {
        let mobj = new_thinker::<mobj_t>();
        let mo = unsafe { &mut *mobj.ptr() };
        mo.health = health;
        // Kept out of the sector and blockmap lists, which the test level
        // does not have.
        mo.flags = MF_NOSECTOR | MF_NOBLOCKMAP;
        mo.thinker.kind = ThinkerKind::Mobj;
        P_AddThinker(ThinkerId::Mobj(mobj));
        mobj
    }

    /// The listed map object tagged with `health` (see [`add_mobj`]).
    fn mobj_with_health(health: i32) -> MobjId {
        thinkers()
            .find_map(|thinker| match thinker {
                ThinkerId::Mobj(mobj) if unsafe { (*mobj.ptr()).health } == health => Some(mobj),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no map object with health {health}"))
    }

    /// Save ordinals number the saved map objects from 1 in list order and
    /// skip removed ones; references to unsaved objects write 0, and
    /// ordinals that name no loaded object resolve to `None`.
    #[test]
    fn save_ordinals_follow_the_thinker_list() {
        let _g = LOCK.lock().unwrap();
        let _thinker_guard = crate::engine::play::tick::THINKER_TEST_LOCK.lock().unwrap();
        P_InitThinkers();
        let first = add_mobj(1);
        let removed = add_mobj(2);
        let second = add_mobj(3);
        P_RemoveThinker(unsafe { &mut (*removed.ptr()).thinker });
        let unlisted = new_thinker::<mobj_t>();

        let ordinals = mobj_save_ordinals();
        assert_eq!(saved_mobj_ordinal(&ordinals, Some(first)), 1);
        assert_eq!(saved_mobj_ordinal(&ordinals, Some(second)), 2);
        assert_eq!(saved_mobj_ordinal(&ordinals, Some(removed)), 0);
        assert_eq!(saved_mobj_ordinal(&ordinals, Some(unlisted)), 0);
        assert_eq!(saved_mobj_ordinal(&ordinals, None), 0);

        let loaded_mobjs = [second, first];
        assert_eq!(mobj_for_saved_ordinal(&loaded_mobjs, 0), None);
        assert_eq!(mobj_for_saved_ordinal(&loaded_mobjs, 1), Some(second));
        assert_eq!(mobj_for_saved_ordinal(&loaded_mobjs, 2), Some(first));
        assert_eq!(mobj_for_saved_ordinal(&loaded_mobjs, 3), None);
        assert_eq!(mobj_for_saved_ordinal(&loaded_mobjs, u32::MAX), None);
        P_InitThinkers();
    }

    /// Map object references survive a save and load: `target`, `tracer`
    /// and a player's `attacker` point at the recreated objects, including
    /// a forward reference and a self-reference; references to a removed
    /// object and to one not in the thinker list load as `None`.
    #[test]
    fn mobj_references_survive_save_and_load() {
        use crate::engine::{
            map::{sector_t, subsector_t},
            play::setup::{LEVEL_TEST_LOCK, LevelState},
        };

        let _g = LOCK.lock().unwrap();
        let _level_guard = LEVEL_TEST_LOCK.lock().unwrap();
        let _thinker_guard = crate::engine::play::tick::THINKER_TEST_LOCK.lock().unwrap();

        // One sector, one subsector and no nodes: every point is in it.
        let level = LevelState {
            numsectors: 1,
            sectors: vec![sector_t::ZERO],
            subsectors: vec![subsector_t {
                sector: SectorId::new(0),
                numlines: 0,
                firstline: 0,
            }],
            ..LevelState::new()
        };
        let saved_level = std::mem::replace(&mut *LEVEL.borrow_mut(), level);
        let saved_player = unsafe { *game_player(0) };
        let saved_playeringame = GAME.borrow().playeringame;

        P_InitThinkers();
        let first = add_mobj(1);
        let second = add_mobj(2);
        let removed = add_mobj(3);
        let third = add_mobj(4);
        let unlisted = new_thinker::<mobj_t>();
        unsafe {
            P_RemoveThinker(&mut (*removed.ptr()).thinker);
            (*first.ptr()).target = Some(third);
            (*first.ptr()).tracer = Some(first);
            (*first.ptr()).player = game_player(0);
            (*second.ptr()).target = Some(removed);
            (*second.ptr()).tracer = Some(unlisted);
            (*third.ptr()).target = Some(first);

            GAME.borrow_mut().playeringame = [1, 0, 0, 0];
            *game_player(0) = std::mem::zeroed();
            (*game_player(0)).mo = Some(first);
            (*game_player(0)).attacker = Some(third);
        }

        with_mem_stream(&mut [], || unsafe {
            P_ArchivePlayers();
            P_ArchiveThinkers();

            *game_player(0) = std::mem::zeroed();
            P_InitThinkers();
            SAVEG.borrow_mut().save_stream.set_position(0);

            P_UnArchivePlayers();
            P_UnArchiveThinkers();
            assert_eq!(SAVEG.borrow().savegame_error, 0);
        });

        let healths: Vec<i32> = thinkers()
            .map(|thinker| match thinker {
                ThinkerId::Mobj(mobj) => unsafe { (*mobj.ptr()).health },
                _ => panic!("{thinker:?} is not a map object"),
            })
            .collect();
        assert_eq!(
            healths,
            [1, 2, 4],
            "saved in list order, removed one left out"
        );
        let (first, second, third) = (
            mobj_with_health(1),
            mobj_with_health(2),
            mobj_with_health(4),
        );
        unsafe {
            assert_eq!((*first.ptr()).target, Some(third));
            assert_eq!((*first.ptr()).tracer, Some(first));
            assert_eq!(
                (*second.ptr()).target,
                None,
                "the removed object is not saved"
            );
            assert_eq!(
                (*second.ptr()).tracer,
                None,
                "the unlisted object is not saved"
            );
            assert_eq!((*third.ptr()).target, Some(first));
            assert_eq!((*third.ptr()).tracer, None);
            assert_eq!((*game_player(0)).mo, Some(first));
            assert_eq!((*game_player(0)).attacker, Some(third));
        }
        assert_eq!(SAVEG.borrow().saved_attackers, [None; MAXPLAYERS]);

        P_InitThinkers();
        unsafe { *game_player(0) = saved_player };
        GAME.borrow_mut().playeringame = saved_playeringame;
        *LEVEL.borrow_mut() = saved_level;
    }

    /// A save-game header with `version` in its version field.
    fn header_with_version(version: &[u8]) -> Vec<u8> {
        let mut header = vec![0u8; SAVESTRINGSIZE + VERSIONSIZE + 3 + MAXPLAYERS + 3];
        header[SAVESTRINGSIZE..][..version.len()].copy_from_slice(version);
        header
    }

    /// Room's version string differs from vanilla's: a vanilla header is
    /// rejected, Room's own is accepted.
    #[test]
    fn header_with_vanilla_version_is_rejected() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(&savegame_version(109), b"wood 109\0\0\0\0\0\0\0\0");

        let mut vanilla = header_with_version(b"version 109");
        with_mem_stream(&mut vanilla, || {
            assert_eq!(read_save_game_header(109), 0);
        });

        let mut other_game_version = header_with_version(&savegame_version(106));
        with_mem_stream(&mut other_game_version, || {
            assert_eq!(read_save_game_header(109), 0);
        });

        let saved_playeringame = GAME.borrow().playeringame;
        let saved_leveltime = leveltime();
        let mut room = header_with_version(&savegame_version(109));
        with_mem_stream(&mut room, || {
            assert_eq!(read_save_game_header(109), 1);
        });
        GAME.borrow_mut().playeringame = saved_playeringame;
        TICK.borrow_mut().leveltime = saved_leveltime;
    }

    /// The vanilla saves in `.savegame` carry `"version 109"` and are
    /// rejected by the version check.
    #[test]
    fn vanilla_fixture_saves_are_rejected() {
        let _g = LOCK.lock().unwrap();
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".savegame");
        for slot in 0..4 {
            let path = directory.join(format!("{SAVEGAMENAME}{slot}.dsg"));
            let mut contents =
                std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_eq!(
                &contents[SAVESTRINGSIZE..SAVESTRINGSIZE + VERSIONSIZE],
                b"version 109\0\0\0\0\0"
            );
            with_mem_stream(&mut contents, || {
                assert_eq!(read_save_game_header(109), 0);
            });
        }
    }

    /// The address-based index helpers give the same indices as the C
    /// pointer subtraction and round-trip through the read helpers.
    #[test]
    fn pointer_indices_round_trip() {
        assert_eq!(saveg_write_state_ptr(std::ptr::null()), 0);
        assert_eq!(saveg_write_state_ptr(state_ptr(5)), 5);
        assert_eq!(saveg_read_state_ptr(5), state_ptr(5));

        assert_eq!(saveg_write_player_ptr(std::ptr::null()), 0);
        assert_eq!(saveg_write_player_ptr(game_player(0)), 1);
        assert_eq!(saveg_write_player_ptr(game_player(2)), 3);
        assert_eq!(unsafe { saveg_read_player_ptr(3) }, game_player(2));

        assert_eq!(saveg_write_sector_ptr(SectorId::new(0)), 0);
        assert_eq!(saveg_write_sector_ptr(SectorId::new(17)), 17);
        assert_eq!(saveg_read_sector_ptr(17), SectorId::new(17));
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

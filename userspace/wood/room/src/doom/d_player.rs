//! Mirror of `d_player.h` types for FFI with remaining C code.
//!
//! Provides `PlayerT`, `TiccmdT`, `PspdefT` and the C globals `players` / `consoleplayer`.
//! Also implements `M_Menu_SetPlayerMessage` (previously in `m_menu_shim.c`).
//!
//! In the original C code `player_t`, `ticcmd_t`, and `pspdef_t` are spread
//! across `d_player.h`, `d_ticcmd.h`, and `p_pspr.h`.  They are collected
//! here for convenience because they form a tight cluster needed for FFI.
//!
//! Rust differences from C:
//! - Boolean fields (`cards`, `backpack`, `weaponowned`, `didsecret`) that are
//!   `boolean` in C are stored as `c_int` here to keep the struct layout
//!   identical to the C ABI on x86-64 Linux (where `boolean = unsigned int =
//!   4 bytes`).  Using a Rust `bool` would break layout because `bool` is 1
//!   byte.
//! - `TiccmdT` adds an explicit `_pad: [u8; 2]` after `arti` to match the
//!   C compiler's padding and reach the expected struct size.
//! - Opaque C types `mobj_t` and `state_t` are represented as empty enums,
//!   which is the idiomatic Rust way to express "pointer-only, never
//!   constructed" foreign types.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::c_char;
use std::os::raw::c_int;

/// Number of power-up slots in `PlayerT::powers`.  Matches `NUMPOWERS` in `doomdef.h`.
pub const NUMPOWERS: usize = 6;

/// Number of key card slots in `PlayerT::cards`.  Matches `NUMCARDS` in `doomdef.h`.
pub const NUMCARDS: usize = 6;

/// Number of weapon slots in `PlayerT::weaponowned`.  Matches `NUMWEAPONS` in `doomdef.h`.
pub const NUMWEAPONS: usize = 9;

/// Number of ammo type slots in `PlayerT::ammo` / `maxammo`.  Matches `NUMAMMO` in `doomdef.h`.
pub const NUMAMMO: usize = 4;

/// Number of player-sprite (weapon overlay) slots in `PlayerT::psprites`.  Matches `NUMPSPRITES` in `p_pspr.h`.
pub const NUMPSPRITES: usize = 2;

/// Maximum number of simultaneously connected players.  Matches `MAXPLAYERS` in `doomdef.h`.
pub const MAXPLAYERS: usize = 4;

/// No-clip cheat flag: player passes through walls.  Matches `CF_NOCLIP` in `d_player.h`.
// Cheat flags (d_player.h)
pub const CF_NOCLIP: c_int = 1;

/// God-mode cheat flag: player takes no damage.  Matches `CF_GODMODE` in `d_player.h`.
pub const CF_GODMODE: c_int = 2;

/// No-momentum debug flag: player cannot move.  Matches `CF_NOMOMENTUM` in `d_player.h`.
pub const CF_NOMOMENTUM: c_int = 4;

/// Opaque handle for the moving-object (`mobj_t`) C type.
///
/// `mobj_t` is defined in `p_mobj.h` and is too large and complex to port
/// fully yet.  Using an empty enum prevents accidental construction while
/// still allowing `*mut mobj_t` pointers to cross the FFI boundary.
pub enum mobj_t {}

/// Opaque handle for the animation-state (`state_t`) C type.
///
/// `state_t` is defined in `info.h` (the mobjinfo table).  As with `mobj_t`,
/// represented as an empty enum so only pointers to it are valid at the
/// Rust call sites.
pub enum state_t {}

/// Per-tic command packet: the inputs sampled from one player for one game tic.
///
/// Corresponds to `ticcmd_t` in `d_ticcmd.h`.  One `TiccmdT` is built each
/// tic by `G_BuildTiccmd` and stored in `PlayerT::cmd`.  In a network game,
/// these packets are also transmitted to peers so every machine runs the same
/// simulation.
///
/// Field semantics (from `d_ticcmd.h`):
/// - `forwardmove`: signed forward/back movement, scaled by 2048 inside the sim.
/// - `sidemove`: signed strafe movement, scaled by 2048.
/// - `angleturn`: signed yaw delta, shifted left by 16 when applied.
/// - `chatchar`: character typed for chat, or 0.
/// - `buttons`: bitfield of `BT_*` action flags (fire, use, weapon change).
/// - `consistancy`: checksum byte used in network games to detect desync.
/// - `buttons2`: Strife-specific secondary button bitfield (`BT2_*`).
/// - `inventory`: Strife-specific inventory item index.
/// - `lookfly`: Heretic/Hexen look-up/down/center.
/// - `arti`: Heretic/Hexen artifact type to use.
///
/// The two padding bytes after `arti` are not present in the C struct but are
/// required here to reach the ABI size on x86-64 Linux.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TiccmdT {
    /// Signed forward/backward movement unit; multiply by 2048 to get fixed-point speed.
    pub forwardmove: i8,
    /// Signed strafe movement unit; multiply by 2048 to get fixed-point speed.
    pub sidemove: i8,
    /// Signed yaw turn delta; shift left 16 to get a `angle_t` increment.
    pub angleturn: i16,
    /// Chat character typed this tic, or 0 if none.
    pub chatchar: u8,
    /// `BT_*` button bitfield for fire, use, weapon-change, and special actions.
    pub buttons: u8,
    /// Network consistency check byte; compared across peers to detect desyncs.
    pub consistancy: u8,
    /// Strife-specific `BT2_*` secondary button bitfield (look, jump, inventory).
    pub buttons2: u8,
    /// Strife-specific inventory item index to use this tic.
    pub inventory: c_int,
    /// Heretic/Hexen look-up/down/center command byte.
    pub lookfly: u8,
    /// Heretic/Hexen artifact (`artitype_t`) to activate this tic.
    pub arti: u8,
    /// Explicit padding to match C ABI struct size on x86-64 Linux.
    _pad: [u8; 2],
}

/// Player-sprite definition: the on-screen weapon/hand overlay animation state.
///
/// Corresponds to `pspdef_t` in `p_pspr.h`.  Each player has
/// [`NUMPSPRITES`] slots (typically `ps_weapon` and `ps_flash`), stored in
/// `PlayerT::psprites`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PspdefT {
    /// Pointer to the current animation state for this sprite overlay.
    pub state: *mut state_t,
    /// Remaining tics in the current animation state; 0 means advance to next.
    pub tics: c_int,
    /// Screen-space X position of the sprite, in fixed-point pixels.
    pub sx: c_int,
    /// Screen-space Y position of the sprite, in fixed-point pixels.
    pub sy: c_int,
}

/// Complete per-player state, updated every tic by the game logic.
///
/// Corresponds to `player_t` in `d_player.h`.  One `PlayerT` exists for each
/// active player (up to [`MAXPLAYERS`]); the local player is always
/// `players[consoleplayer]`.  The struct is large (328 bytes on x86-64) and
/// encapsulates everything the engine needs to simulate, render, and save a
/// single player's participation in a game session.
///
/// Layout is verified at compile time by the tests below; the expected values
/// were derived from a `layout_probe.c` run on x86-64 Linux.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PlayerT {
    /// Pointer to the player's map object (position, velocity, health during play).
    pub mo: *mut mobj_t,
    /// Current lifecycle state: alive (`PST_LIVE`), dead (`PST_DEAD`), or respawning (`PST_REBORN`).
    pub playerstate: c_int,
    /// The tic command built for this player this tic.
    pub cmd: TiccmdT,
    /// Camera height above the floor in fixed-point units (`fixed_t`).
    pub viewz: c_int,
    /// Base eye height above the floor; normally 41 units.
    pub viewheight: c_int,
    /// Per-tic adjustment to `viewheight` for landing/bobbing smoothness.
    pub deltaviewheight: c_int,
    /// Bounded total momentum magnitude used to drive weapon and view bob.
    pub bob: c_int,
    /// Player health between levels (during a level, `mo->health` is authoritative).
    pub health: c_int,
    /// Armor point total; 0 = no armor.
    pub armorpoints: c_int,
    /// Armor type: 0 = none, 1 = green armor (33%), 2 = blue armor (50%).
    pub armortype: c_int,
    /// Active power-up tic counters (`pw_*` index); invulnerability/invisibility count down.
    pub powers: [c_int; NUMPOWERS],
    /// Key card / skull key possession flags (`it_*` index); non-zero means owned.
    pub cards: [c_int; NUMCARDS],
    /// Non-zero if the player has picked up a backpack (doubles max ammo).
    pub backpack: c_int,
    /// Per-player frag counts indexed by player number.
    pub frags: [c_int; MAXPLAYERS],
    /// Currently active weapon (`weapontype_t` value).
    pub readyweapon: c_int,
    /// Weapon the player has requested to switch to; `wp_nochange` if none.
    pub pendingweapon: c_int,
    /// Weapon ownership flags; non-zero at index `i` means weapon `i` is owned.
    pub weaponowned: [c_int; NUMWEAPONS],
    /// Current ammo count per ammo type (`am_*` index).
    pub ammo: [c_int; NUMAMMO],
    /// Maximum ammo capacity per ammo type; doubled after picking up a backpack.
    pub maxammo: [c_int; NUMAMMO],
    /// Non-zero if the attack button was held last tic (prevents auto-fire restart).
    pub attackdown: c_int,
    /// Non-zero if the use button was held last tic (prevents repeated use on hold).
    pub usedown: c_int,
    /// Bitmask of active cheat flags (`CF_NOCLIP`, `CF_GODMODE`, `CF_NOMOMENTUM`).
    pub cheats: c_int,
    /// Refiring counter; incremented while the fire button is held; accuracy decreases while non-zero.
    pub refire: c_int,
    /// Total kills this level (used by intermission screen).
    pub killcount: c_int,
    /// Total items picked up this level (used by intermission screen).
    pub itemcount: c_int,
    /// Total secrets found this level (used by intermission screen).
    pub secretcount: c_int,
    /// Pointer to the current HUD hint message string, or null if none.
    pub message: *mut c_char,
    /// Tic counter for red damage flash; decrements toward 0 each tic.
    pub damagecount: c_int,
    /// Tic counter for yellow bonus flash; decrements toward 0 each tic.
    pub bonuscount: c_int,
    /// Pointer to the `mobj_t` that last damaged this player; null for environment damage.
    pub attacker: *mut mobj_t,
    /// Extra lighting bonus for gun flashes (added to sector light level during render).
    pub extralight: c_int,
    /// If non-zero, overrides the colormap used to render the player's view (e.g., `REDCOLORMAP` for pain).
    pub fixedcolormap: c_int,
    /// Player skin color shift index (0-3), used by the renderer to select the palette range.
    pub colormap: c_int,
    /// Weapon and muzzle-flash overlay sprites drawn on top of the 3-D view.
    pub psprites: [PspdefT; NUMPSPRITES],
    /// Non-zero if this player has completed a secret level this episode.
    pub didsecret: c_int,
}

extern "C" {
    /// Global array of player state structs; defined in C (`g_game.c`).
    ///
    /// Index 0 is the local player when `consoleplayer == 0`.  Slots
    /// `[1..MAXPLAYERS-1]` are used in network games.
    pub static mut players: [PlayerT; MAXPLAYERS];

    /// Index into `players` for the player whose view is shown on the local screen.
    ///
    /// Defined in C (`g_game.c`).  Always 0 for single-player games; may differ
    /// in split-screen or network configurations.
    pub static mut consoleplayer: c_int;
}

/// Set the HUD hint message for the console player.
///
/// This function replaces the tiny `m_menu_shim.c` that previously bridged
/// the C `M_Menu` code to the player struct.  It writes `msg` into
/// `players[consoleplayer].message` so the HUD renderer can display it.
///
/// Called from C (`m_menu.c`) when a menu action produces a status message
/// (e.g., "Gamma correction OFF").
///
/// # Safety
/// - `msg` must be a valid pointer to a NUL-terminated C string that remains
///   valid at least until the next game tic clears the message field.
/// - `consoleplayer` must be in the range `[0, MAXPLAYERS)` before this
///   function is called (guaranteed by the engine's startup sequence).
#[no_mangle]
pub unsafe extern "C" fn M_Menu_SetPlayerMessage(msg: *const c_char) {
    (*std::ptr::addr_of_mut!(players[0]).offset(consoleplayer as isize)).message =
        msg as *mut c_char;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Expected layout of `player_t` as emitted by layout_probe.c
    /// on x86_64 Linux.  Kept in-source so tests don't depend on the
    /// linker pulling symbols out of a static archive.
    const PLAYER_T_SIZEOF: usize = 328;
    const PLAYER_T_MESSAGE_OFFSET: usize = 232;

    #[test]
    fn player_t_size_matches_c() {
        assert_eq!(
            std::mem::size_of::<PlayerT>(),
            PLAYER_T_SIZEOF,
            "PlayerT size mismatch: Rust={}, expected={}",
            std::mem::size_of::<PlayerT>(),
            PLAYER_T_SIZEOF
        );
    }

    #[test]
    fn player_t_message_offset_matches_c() {
        assert_eq!(
            std::mem::offset_of!(PlayerT, message),
            PLAYER_T_MESSAGE_OFFSET,
            "PlayerT.message offset mismatch: Rust={}, expected={}",
            std::mem::offset_of!(PlayerT, message),
            PLAYER_T_MESSAGE_OFFSET
        );
    }
}

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
//!   `boolean` in C are stored as `i32` here, like the C `boolean`
//!   (`unsigned int`).
//! - `TiccmdT` adds an explicit `_pad: [u8; 2]` after `arti` to match the
//!   C compiler's padding and reach the expected struct size.
//! - `PspdefT::state` points through the opaque `state_t` from
//!   `play::info`, an empty enum: the idiomatic Rust way to express
//!   "pointer-only, never constructed" foreign types.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::engine::play::{info::state_t, mobj::mobj_t};

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
pub const CF_NOCLIP: i32 = 1;

/// God-mode cheat flag: player takes no damage.  Matches `CF_GODMODE` in `d_player.h`.
pub const CF_GODMODE: i32 = 2;

/// No-momentum debug flag: player cannot move.  Matches `CF_NOMOMENTUM` in `d_player.h`.
pub const CF_NOMOMENTUM: i32 = 4;

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
    pub inventory: i32,
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
#[derive(Clone, Copy, Debug)]
pub struct PspdefT {
    /// Pointer to the current animation state for this sprite overlay.
    pub state: *mut state_t,
    /// Remaining tics in the current animation state; 0 means advance to next.
    pub tics: i32,
    /// Screen-space X position of the sprite, in fixed-point pixels.
    pub sx: i32,
    /// Screen-space Y position of the sprite, in fixed-point pixels.
    pub sy: i32,
}

/// Complete per-player state, updated every tic by the game logic.
///
/// Corresponds to `player_t` in `d_player.h`.  One `PlayerT` exists for each
/// active player (up to [`MAXPLAYERS`]); the local player is always
/// `players[consoleplayer]`.  The struct encapsulates everything the engine
/// needs to simulate, render, and save a single player's participation in a
/// game session. It is `Copy` and valid when zeroed: `G_PlayerReborn` resets
/// it with `mem::zeroed()`.
#[derive(Clone, Copy, Debug)]
pub struct PlayerT {
    /// Pointer to the player's map object (position, velocity, health during play).
    pub mo: *mut mobj_t,
    /// Current lifecycle state: alive (`PST_LIVE`), dead (`PST_DEAD`), or respawning (`PST_REBORN`).
    pub playerstate: i32,
    /// The tic command built for this player this tic.
    pub cmd: TiccmdT,
    /// Camera height above the floor in fixed-point units (`fixed_t`).
    pub viewz: i32,
    /// Base eye height above the floor; normally 41 units.
    pub viewheight: i32,
    /// Per-tic adjustment to `viewheight` for landing/bobbing smoothness.
    pub deltaviewheight: i32,
    /// Bounded total momentum magnitude used to drive weapon and view bob.
    pub bob: i32,
    /// Player health between levels (during a level, `mo->health` is authoritative).
    pub health: i32,
    /// Armor point total; 0 = no armor.
    pub armorpoints: i32,
    /// Armor type: 0 = none, 1 = green armor (33%), 2 = blue armor (50%).
    pub armortype: i32,
    /// Active power-up tic counters (`pw_*` index); invulnerability/invisibility count down.
    pub powers: [i32; NUMPOWERS],
    /// Key card / skull key possession flags (`it_*` index); non-zero means owned.
    pub cards: [i32; NUMCARDS],
    /// Non-zero if the player has picked up a backpack (doubles max ammo).
    pub backpack: i32,
    /// Per-player frag counts indexed by player number.
    pub frags: [i32; MAXPLAYERS],
    /// Currently active weapon (`weapontype_t` value).
    pub readyweapon: i32,
    /// Weapon the player has requested to switch to; `wp_nochange` if none.
    pub pendingweapon: i32,
    /// Weapon ownership flags; non-zero at index `i` means weapon `i` is owned.
    pub weaponowned: [i32; NUMWEAPONS],
    /// Current ammo count per ammo type (`am_*` index).
    pub ammo: [i32; NUMAMMO],
    /// Maximum ammo capacity per ammo type; doubled after picking up a backpack.
    pub maxammo: [i32; NUMAMMO],
    /// Non-zero if the attack button was held last tic (prevents auto-fire restart).
    pub attackdown: i32,
    /// Non-zero if the use button was held last tic (prevents repeated use on hold).
    pub usedown: i32,
    /// Bitmask of active cheat flags (`CF_NOCLIP`, `CF_GODMODE`, `CF_NOMOMENTUM`).
    pub cheats: i32,
    /// Refiring counter; incremented while the fire button is held; accuracy decreases while non-zero.
    pub refire: i32,
    /// Total kills this level (used by intermission screen).
    pub killcount: i32,
    /// Total items picked up this level (used by intermission screen).
    pub itemcount: i32,
    /// Total secrets found this level (used by intermission screen).
    pub secretcount: i32,
    /// The current HUD hint message; empty if there is none.
    pub message: PlayerMessage,
    /// Tic counter for red damage flash; decrements toward 0 each tic.
    pub damagecount: i32,
    /// Tic counter for yellow bonus flash; decrements toward 0 each tic.
    pub bonuscount: i32,
    /// Pointer to the `mobj_t` that last damaged this player; null for environment damage.
    pub attacker: *mut mobj_t,
    /// Extra lighting bonus for gun flashes (added to sector light level during render).
    pub extralight: i32,
    /// If non-zero, overrides the colormap used to render the player's view (e.g., `REDCOLORMAP` for pain).
    pub fixedcolormap: i32,
    /// Player skin color shift index (0-3), used by the renderer to select the palette range.
    pub colormap: i32,
    /// Weapon and muzzle-flash overlay sprites drawn on top of the 3-D view.
    pub psprites: [PspdefT; NUMPSPRITES],
    /// Non-zero if this player has completed a secret level this episode.
    pub didsecret: i32,
}

/// The player state array (`players`) and the index of the player whose view
/// is shown locally (`consoleplayer`), defined in `g_game`.
pub use crate::engine::game::{consoleplayer, players};

/// Set the HUD hint message for the console player.
///
/// This function replaces the tiny `m_menu_shim.c` that previously bridged
/// the C `M_Menu` code to the player struct.  It writes `msg` into
/// `players[consoleplayer].message` so the HUD renderer can display it.
///
/// Called from `m_menu` when a menu action produces a status message
/// (e.g., "Gamma correction OFF").
///
/// # Safety
/// `consoleplayer` must be in the range `[0, MAXPLAYERS)` before this
/// function is called (guaranteed by the engine's startup sequence).
pub unsafe fn M_Menu_SetPlayerMessage(msg: &str) {
    (*std::ptr::addr_of_mut!(players[0]).offset(consoleplayer as isize)).message =
        PlayerMessage::new(msg);
}

/// A HUD hint message, stored inline so [`PlayerT`] stays `Copy` and a
/// zeroed player has no message.
///
/// Holds up to [`PlayerMessage::CAPACITY`] bytes, one HUD line (C
/// `HU_MAXLINELENGTH`); longer text is truncated, as the HUD line would
/// truncate it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerMessage {
    /// Number of bytes of `bytes` in use; 0 means no message.
    length: u8,
    /// The message text.
    bytes: [u8; PlayerMessage::CAPACITY],
}

impl PlayerMessage {
    /// Longest message kept, in bytes.
    pub const CAPACITY: usize = 80;

    /// No message.
    pub const NONE: Self = Self {
        length: 0,
        bytes: [0; Self::CAPACITY],
    };

    /// A message holding `text`, truncated to [`Self::CAPACITY`] bytes.
    pub fn new(text: impl AsRef<[u8]>) -> Self {
        let text = text.as_ref();
        let length = text.len().min(Self::CAPACITY);
        let mut message = Self::NONE;
        message.bytes[..length].copy_from_slice(&text[..length]);
        message.length = length as u8;
        message
    }

    /// The message text; empty if there is no message.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.length)]
    }

    /// Whether there is no message.
    pub fn is_none(&self) -> bool {
        self.length == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A zeroed message (as in a zeroed `PlayerT`) is no message.
    #[test]
    fn zeroed_message_is_none() {
        let message: PlayerMessage = unsafe { std::mem::zeroed() };
        assert!(message.is_none());
        assert_eq!(message, PlayerMessage::NONE);
    }

    /// Text is kept up to one HUD line and truncated beyond it.
    #[test]
    fn message_keeps_one_hud_line() {
        assert_eq!(PlayerMessage::new("game saved.").as_bytes(), b"game saved.");
        let long = [b'x'; 100];
        assert_eq!(
            PlayerMessage::new(long).as_bytes(),
            &long[..PlayerMessage::CAPACITY]
        );
        assert!(PlayerMessage::new("").is_none());
    }
}

//! Player types from `d_player.h`.
//!
//! Provides `PlayerT`, `TiccmdT`, `PspdefT`, the `PlayerId` that names a
//! slot of the C global `players`, and `consoleplayer`.
//! Also implements `M_Menu_SetPlayerMessage`.
//!
//! In the original C code `player_t`, `ticcmd_t`, and `pspdef_t` are spread
//! across `d_player.h`, `d_ticcmd.h`, and `p_pspr.h`.  They are collected
//! here because they form a tight cluster.
//!
//! Rust differences from C:
//! - Boolean fields (`cards`, `backpack`, `weaponowned`, `didsecret`) that are
//!   `boolean` in C are stored as `i32` here, like the C `boolean`
//!   (`unsigned int`).

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::engine::play::{info::State, mobj::MobjId};

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
}

/// Player-sprite definition: the on-screen weapon/hand overlay animation state.
///
/// Corresponds to `pspdef_t` in `p_pspr.h`.  Each player has
/// [`NUMPSPRITES`] slots (typically `ps_weapon` and `ps_flash`), stored in
/// `PlayerT::psprites`.
#[derive(Clone, Copy, Debug)]
pub struct PspdefT {
    /// Pointer to the current animation state for this sprite overlay.
    pub state: *mut State,
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
    /// The player's map object (position, velocity, health during play);
    /// `None` before the player is first spawned. [`PlayerT::mo`] reads it.
    pub mo: Option<MobjId>,
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
    /// The map object that last damaged this player; `None` for environment damage.
    pub attacker: Option<MobjId>,
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

impl PlayerT {
    /// The player's map object, which every player in a running level has
    /// (C dereferences `player->mo` without a check).
    ///
    /// # Panics
    ///
    /// If the player has not been spawned.
    #[track_caller]
    pub fn mo(&self) -> MobjId {
        self.mo.expect("player has no map object")
    }
}

/// The index of the player whose view is shown locally (`consoleplayer`),
/// defined in `g_game`.
pub use crate::engine::game::consoleplayer;

/// Names one of the [`MAXPLAYERS`] slots in `players` (`g_game`); C passes
/// and stores `&players[i]` where Rust passes and stores a `PlayerId`.
///
/// The index is checked when the id is made, so every id names a slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlayerId(u8);

impl PlayerId {
    /// The id of slot `index`.
    ///
    /// # Panics
    ///
    /// If `index` is not below [`MAXPLAYERS`].
    #[track_caller]
    pub fn new(index: usize) -> Self {
        assert!(index < MAXPLAYERS, "player index {index} out of range");
        Self(index as u8)
    }

    /// The console player's id (C `&players[consoleplayer]`).
    #[track_caller]
    pub fn console() -> Self {
        Self::new(consoleplayer() as usize)
    }

    /// Every slot's id, in slot order.
    pub fn all() -> impl Iterator<Item = Self> {
        (0..MAXPLAYERS).map(Self::new)
    }

    /// The slot index (C `player - players`).
    pub fn index(self) -> usize {
        usize::from(self.0)
    }

    /// Pointer to the slot, stable for the whole program.
    ///
    /// For code that keeps the player across calls back into play code,
    /// which may reach the same slot (weapon actions, `P_PlayerThink`,
    /// damage): such code reads and writes through this pointer for its
    /// whole body, as C does through `player_t *`. Every use of it is a
    /// place the move to explicit world borrows still has to rework.
    pub fn ptr(self) -> *mut PlayerT {
        use crate::engine::game::PLAYERS;
        // SAFETY: in-bounds projection on the stable state pointer.
        unsafe { PLAYERS.as_ptr().cast::<PlayerT>().add(self.index()) }
    }

    /// Run `f` on the slot for reading.
    ///
    /// `f` must not reach play code; the slot stays borrowed while it runs.
    ///
    /// # Panics
    ///
    /// In debug builds, if the players are mutably borrowed.
    #[track_caller]
    pub fn read<R>(self, f: impl FnOnce(&PlayerT) -> R) -> R {
        f(&crate::engine::game::PLAYERS.borrow()[self.index()])
    }

    /// Run `f` on the slot for writing.
    ///
    /// `f` must not reach play code; the slot stays borrowed while it runs.
    ///
    /// # Panics
    ///
    /// In debug builds, if the players are borrowed.
    #[track_caller]
    pub fn write<R>(self, f: impl FnOnce(&mut PlayerT) -> R) -> R {
        f(&mut crate::engine::game::PLAYERS.borrow_mut()[self.index()])
    }

    /// The player's map object (C `player->mo`).
    ///
    /// # Panics
    ///
    /// If the player has not been spawned.
    #[track_caller]
    pub fn mo(self) -> MobjId {
        self.read(PlayerT::mo)
    }

    /// Set the player's HUD hint message (C `player->message = text`).
    pub fn set_message(self, text: impl AsRef<[u8]>) {
        let message = PlayerMessage::new(text);
        self.write(|player| player.message = message);
    }
}

/// Set the HUD hint message for the console player.
///
/// This function replaces the tiny `m_menu_shim.c` that previously bridged
/// the C `M_Menu` code to the player struct.  It writes `msg` into
/// `players[consoleplayer].message` so the HUD renderer can display it.
///
/// Called from `m_menu` when a menu action produces a status message
/// (e.g., "Gamma correction OFF").
pub fn M_Menu_SetPlayerMessage(msg: &str) {
    PlayerId::console().set_message(msg);
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

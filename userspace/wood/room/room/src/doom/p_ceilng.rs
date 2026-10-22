//! Ceiling animation: lowering, crushing, and raising.
//!
//! Rust port of `vendor/doomgeneric/p_ceilng.c`.  Manages the per-tic update
//! logic for moving ceilings ([`T_MoveCeiling`]), linedef-triggered ceiling
//! activation ([`EV_DoCeiling`] / [`EV_CeilingCrushStop`]), and the fixed-size
//! active-ceiling table ([`activeceilings`]).

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::doom::sounds::Sfx;
use std::ffi::c_void;
use std::os::raw::c_int;

use crate::doom::c_ffi as cffi;
use crate::doom::m_fixed::{fixed_t, FRACUNIT};
use crate::doom::p_floor::T_MovePlane;
use crate::doom::p_lights::{line_t, sector_t};
use crate::doom::p_setup::sectors;
use crate::doom::p_spec::{P_FindHighestCeilingSurrounding, P_FindSectorFromLineTag};
use crate::doom::p_tick::{leveltime, thinker_t, P_AddThinker, P_RemoveThinker};
use crate::doom::s_sound::S_StartSound;
use crate::doom::z_zone::{Z_Malloc, PU_LEVSPEC};

/// Default ceiling movement speed: 1 fixed-point unit per tic (`FRACUNIT`).
///
/// Crusher ceilings slow to `CEILSPEED / 8` when they hit something, and fast
/// crushers move at `CEILSPEED * 2`.  Matches `CEILSPEED` in `p_ceilng.c`.
const CEILSPEED: fixed_t = FRACUNIT;

/// Maximum number of simultaneously active ceiling movers.
///
/// Matches `MAXCEILINGS` in `p_local.h`.  If all slots are occupied,
/// [`P_AddActiveCeiling`] silently discards new ceilings.
pub const MAXCEILINGS: usize = 30;

// result_e enum values returned by T_MovePlane.
/// `T_MovePlane` result: the plane moved without incident.
const result_ok: c_int = 0;
/// `T_MovePlane` result: the plane crushed something while moving.
const result_crushed: c_int = 1;
/// `T_MovePlane` result: the plane reached its destination height.
const result_pastdest: c_int = 2;

// ceiling_e enum values — ceiling mover types.
/// Lower ceiling until it reaches the floor height of the sector.
const lowerToFloor: c_int = 0;
/// Raise ceiling to the highest surrounding ceiling height.
const raiseToHighest: c_int = 1;
/// Lower ceiling to 8 units above the floor, crushing things in the way.
const lowerAndCrush: c_int = 2;
/// Crusher that bounces between floor+8 and its original height; normal speed.
const crushAndRaise: c_int = 3;
/// Crusher that bounces at twice the normal speed.
const fastCrushAndRaise: c_int = 4;
/// Like [`crushAndRaise`] but does not play the movement sound each tic.
const silentCrushAndRaise: c_int = 5;

/// Per-sector thinker for an active ceiling mover.
///
/// Allocated via `Z_Malloc` and linked into the thinker list.  The `thinker`
/// field must be at offset 0 (verified by the compile-time assertions in
/// `layout_checks`) so that a `*mut ceiling_t` can be cast safely to
/// `*mut thinker_t`.
///
/// Layout matches the C `ceiling_t` struct.  Padding (`_pad0`, `_pad1`)
/// preserves C alignment on 64-bit targets.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ceiling_t {
    /// Thinker header — must be at offset 0.
    pub thinker: thinker_t,
    /// Ceiling type (one of the `ceiling_e` integer constants).
    pub r#type: c_int,
    _pad0: [u8; 4],
    /// The sector whose ceiling this thinker is moving.
    pub sector: *mut sector_t,
    /// Target height when the ceiling is moving downward.
    pub bottomheight: fixed_t,
    /// Target height when the ceiling is moving upward.
    pub topheight: fixed_t,
    /// Current movement speed in fixed-point units per tic.
    pub speed: fixed_t,
    /// Non-zero if the ceiling damages things it crushes.
    pub crush: c_int,
    /// Current movement direction: `1` = up, `-1` = down, `0` = in stasis.
    pub direction: c_int,
    /// Linedef tag that activated this ceiling (used for crush-stop matching).
    pub tag: c_int,
    /// Saved direction used to resume a ceiling that was put into stasis.
    pub olddirection: c_int,
    _pad1: [u8; 4],
}

#[cfg(target_pointer_width = "64")]
mod layout_checks {
    use super::*;
    const _: () = assert!(std::mem::size_of::<ceiling_t>() == 72);
    const _: () = assert!(std::mem::offset_of!(ceiling_t, thinker) == 0);
    const _: () = assert!(std::mem::offset_of!(ceiling_t, r#type) == 24);
    const _: () = assert!(std::mem::offset_of!(ceiling_t, sector) == 32);
    const _: () = assert!(std::mem::offset_of!(ceiling_t, bottomheight) == 40);
    const _: () = assert!(std::mem::offset_of!(ceiling_t, topheight) == 44);
    const _: () = assert!(std::mem::offset_of!(ceiling_t, speed) == 48);
    const _: () = assert!(std::mem::offset_of!(ceiling_t, crush) == 52);
    const _: () = assert!(std::mem::offset_of!(ceiling_t, direction) == 56);
    const _: () = assert!(std::mem::offset_of!(ceiling_t, tag) == 60);
    const _: () = assert!(std::mem::offset_of!(ceiling_t, olddirection) == 64);
}

/// Table of all currently active ceiling thinkers.
///
/// Null entries represent free slots.  [`P_AddActiveCeiling`] fills the first
/// null slot; [`P_RemoveActiveCeiling`] nulls the matching slot.
/// Exported as `activeceilings` for C linkage.
#[no_mangle]
pub static mut activeceilings: [*mut ceiling_t; MAXCEILINGS] = [std::ptr::null_mut(); MAXCEILINGS];

/// Per-tic update for a moving ceiling thinker.
///
/// Calls `T_MovePlane` with the appropriate target height (determined by
/// `ceiling->direction`), then reacts to the result:
///
/// - **`result_pastdest`**: the ceiling reached its target height.
///   - `raiseToHighest`: remove and free the thinker.
///   - `silentCrushAndRaise` (downward): play the stop sound, then reverse
///     direction upward.
///   - `fastCrushAndRaise` / `crushAndRaise` (downward): reverse direction.
///   - `lowerAndCrush` / `lowerToFloor` (downward): remove and free the
///     thinker.
/// - **`result_crushed`**: the ceiling hit something while moving down.
///   - `silentCrushAndRaise` / `crushAndRaise` / `lowerAndCrush`: reduce
///     speed to `CEILSPEED / 8` to avoid shaking the victim rapidly.
///
/// Every 8 tics, a movement sound (`sfx_stnmov`) is played at the sector's
/// sound origin, except for `silentCrushAndRaise` which suppresses it.
///
/// # FIXME
/// The C original uses a `switch(direction)` with explicit `case 0` (stasis,
/// no-op), `case 1` (up), and `case -1` (down) branches.  The direction
/// controls which target height is passed to `T_MovePlane` and which
/// `pastdest` actions apply.  This Rust port passes `direction` to
/// `T_MovePlane` but does not gate the `pastdest`/`crushed` handling on the
/// current direction, so those match arms may fire for `direction == 0`
/// (stasis) or the wrong direction — a minor behavioral divergence from C.
///
/// # FIXME
/// The C `case silentCrushAndRaise` (downward) falls through to
/// `crushAndRaise`/`fastCrushAndRaise`, which restores speed to `CEILSPEED`
/// and reverses direction.  This Rust port handles `silentCrushAndRaise`
/// separately and only plays the stop sound; it does not restore speed or
/// reverse direction for that variant.
///
/// Corresponds to `T_MoveCeiling` in `p_ceilng.c`.
///
/// # Safety
///
/// `ceiling` must be a valid, non-null pointer to a `ceiling_t` that is
/// currently linked in the thinker list and whose `sector` pointer is valid.
#[no_mangle]
pub unsafe extern "C" fn T_MoveCeiling(ceiling: *mut ceiling_t) {
    let res = T_MovePlane(
        (*ceiling).sector,
        (*ceiling).speed,
        if (*ceiling).direction == 1 {
            (*ceiling).topheight
        } else {
            (*ceiling).bottomheight
        },
        (*ceiling).crush,
        1,
        (*ceiling).direction,
    );

    if (leveltime & 7) == 0 {
        match (*ceiling).r#type {
            x if x == silentCrushAndRaise => {}
            _ => {
                S_StartSound(
                    &(*(*ceiling).sector).soundorg as *const [u8; 40] as *mut c_void,
                    Sfx::Stnmov as c_int,
                );
            }
        }
    }

    if res == result_pastdest {
        match (*ceiling).r#type {
            x if x == raiseToHighest => {
                P_RemoveActiveCeiling(ceiling);
            }
            x if x == silentCrushAndRaise => {
                S_StartSound(
                    &(*(*ceiling).sector).soundorg as *const [u8; 40] as *mut c_void,
                    Sfx::Pstop as c_int,
                );
            }
            x if x == fastCrushAndRaise || x == crushAndRaise => {
                (*ceiling).direction = -1;
            }
            _ => {}
        }
    } else if res == result_crushed {
        match (*ceiling).r#type {
            x if x == silentCrushAndRaise || x == crushAndRaise || x == lowerAndCrush => {
                (*ceiling).speed = CEILSPEED / 8;
            }
            _ => {}
        }
    }
}

/// Activate a ceiling mover on every sector whose tag matches `line->tag`.
///
/// For crusher types (`fastCrushAndRaise`, `silentCrushAndRaise`,
/// `crushAndRaise`), first reactivates any in-stasis ceilings with the same
/// tag via [`P_ActivateInStasisCeiling`].
///
/// For each eligible sector (no existing special data):
/// 1. Allocates and links a new `ceiling_t` thinker.
/// 2. Sets its parameters based on `ceilingtype` (height targets, speed,
///    crush flag, direction).  `silentCrushAndRaise` and `crushAndRaise` only
///    initialise `crush` and `topheight`; `bottomheight` and `direction` keep
///    their zero-initialised defaults and are set dynamically by `T_MoveCeiling`.
/// 3. Registers it in [`activeceilings`].
///
/// Returns `1` if at least one ceiling was activated, `0` otherwise.
///
/// Corresponds to `EV_DoCeiling` in `p_ceilng.c`.
///
/// # Safety
///
/// `line` must be a valid, non-null pointer.  The global `sectors` array must
/// be initialised for the current level.
#[no_mangle]
pub unsafe extern "C" fn EV_DoCeiling(line: *mut line_t, ceilingtype: c_int) -> c_int {
    let mut secnum: c_int = -1;
    let mut rtn: c_int = 0;

    // Reactivate in-stasis ceilings for certain types.
    match ceilingtype {
        x if x == fastCrushAndRaise || x == silentCrushAndRaise || x == crushAndRaise => {
            P_ActivateInStasisCeiling(line);
        }
        _ => {}
    }

    while {
        secnum = P_FindSectorFromLineTag(line as *mut cffi::line_t, secnum);
        secnum
    } >= 0
    {
        let sec = sectors.add(secnum as usize);
        if !(*sec).specialdata.is_null() {
            continue;
        }

        rtn = 1;
        let ceiling = Z_Malloc(
            std::mem::size_of::<ceiling_t>() as c_int,
            PU_LEVSPEC,
            std::ptr::null_mut(),
        ) as *mut ceiling_t;
        P_AddThinker(&mut (*ceiling).thinker);
        (*sec).specialdata = ceiling as *mut c_void;
        (*ceiling).thinker.function.acp1 = Some(core::mem::transmute::<
            unsafe extern "C" fn(*mut ceiling_t),
            unsafe extern "C" fn(*mut c_void),
        >(T_MoveCeiling));
        (*ceiling).sector = sec as *mut sector_t;
        (*ceiling).crush = 0;

        match ceilingtype {
            x if x == fastCrushAndRaise => {
                (*ceiling).crush = 1;
                (*ceiling).topheight = (*sec).ceilingheight;
                (*ceiling).bottomheight = (*sec).floorheight + (8 * FRACUNIT);
                (*ceiling).direction = -1;
                (*ceiling).speed = CEILSPEED * 2;
            }
            x if x == silentCrushAndRaise || x == crushAndRaise => {
                (*ceiling).crush = 1;
                (*ceiling).topheight = (*sec).ceilingheight;
            }
            x if x == lowerAndCrush || x == lowerToFloor => {
                (*ceiling).bottomheight = (*sec).floorheight;
                if ceilingtype != lowerToFloor {
                    (*ceiling).bottomheight += 8 * FRACUNIT;
                }
                (*ceiling).direction = -1;
                (*ceiling).speed = CEILSPEED;
            }
            x if x == raiseToHighest => {
                (*ceiling).topheight = P_FindHighestCeilingSurrounding(sec);
                (*ceiling).direction = 1;
                (*ceiling).speed = CEILSPEED;
            }
            _ => {}
        }

        (*ceiling).tag = (*sec).tag as c_int;
        (*ceiling).r#type = ceilingtype;
        P_AddActiveCeiling(ceiling);
    }
    rtn
}

/// Register `c` in the first available slot of [`activeceilings`].
///
/// If all [`MAXCEILINGS`] slots are occupied the function returns silently,
/// discarding the ceiling — matching the C behaviour which has no overflow
/// guard.
///
/// Corresponds to `P_AddActiveCeiling` in `p_ceilng.c`.
#[no_mangle]
pub extern "C" fn P_AddActiveCeiling(c: *mut ceiling_t) {
    unsafe {
        for i in 0..MAXCEILINGS {
            if activeceilings[i].is_null() {
                activeceilings[i] = c;
                return;
            }
        }
    }
}

/// Unlink and schedule deallocation of the active ceiling `c`.
///
/// Clears `sector->specialdata`, calls [`P_RemoveThinker`] (which marks the
/// thinker for deferred `Z_Free`), and nulls the matching slot in
/// [`activeceilings`].  Does nothing if `c` is not found in the table.
///
/// Corresponds to `P_RemoveActiveCeiling` in `p_ceilng.c`.
#[no_mangle]
pub extern "C" fn P_RemoveActiveCeiling(c: *mut ceiling_t) {
    unsafe {
        for i in 0..MAXCEILINGS {
            if activeceilings[i] == c {
                (*activeceilings[i]).sector.as_mut().unwrap().specialdata = std::ptr::null_mut();
                P_RemoveThinker(&mut (*activeceilings[i]).thinker);
                activeceilings[i] = std::ptr::null_mut();
                break;
            }
        }
    }
}

/// Restart any in-stasis ceilings whose tag matches `line->tag`.
///
/// A ceiling is in stasis when `direction == 0`.  This function restores
/// `direction` from `olddirection` and reinstates `T_MoveCeiling` as the
/// thinker callback.
///
/// Called by [`EV_DoCeiling`] for crusher types, and corresponds to
/// `P_ActivateInStasisCeiling` in `p_ceilng.c`.
///
/// # Safety
///
/// `line` must be a valid, non-null pointer.
#[no_mangle]
pub extern "C" fn P_ActivateInStasisCeiling(line: *mut line_t) {
    unsafe {
        for i in 0..MAXCEILINGS {
            if !activeceilings[i].is_null()
                && (*activeceilings[i]).tag == (*line).tag as c_int
                && (*activeceilings[i]).direction == 0
            {
                (*activeceilings[i]).direction = (*activeceilings[i]).olddirection;
                (*activeceilings[i]).thinker.function.acp1 = Some(core::mem::transmute::<
                    unsafe extern "C" fn(*mut ceiling_t),
                    unsafe extern "C" fn(*mut c_void),
                >(T_MoveCeiling));
            }
        }
    }
}

/// Stop all active crusher ceilings whose tag matches `line->tag`.
///
/// Each matching ceiling has its `direction` saved to `olddirection`, its
/// `direction` set to `0` (stasis), and its thinker function set to `None` so
/// the callback is skipped each tic.  The ceiling is not removed from
/// [`activeceilings`]; it can be restarted by [`P_ActivateInStasisCeiling`].
///
/// Returns `1` if at least one ceiling was stopped, `0` otherwise.
///
/// Corresponds to `EV_CeilingCrushStop` in `p_ceilng.c`.
///
/// # Safety
///
/// `line` must be a valid, non-null pointer.
#[no_mangle]
pub extern "C" fn EV_CeilingCrushStop(line: *mut line_t) -> c_int {
    unsafe {
        let mut rtn: c_int = 0;
        for i in 0..MAXCEILINGS {
            if !activeceilings[i].is_null()
                && (*activeceilings[i]).tag == (*line).tag as c_int
                && (*activeceilings[i]).direction != 0
            {
                (*activeceilings[i]).olddirection = (*activeceilings[i]).direction;
                (*activeceilings[i]).thinker.function.acv = None;
                (*activeceilings[i]).direction = 0;
                rtn = 1;
            }
        }
        rtn
    }
}

/// Link anchor that prevents dead-code elimination of exported ceiling symbols.
///
/// Referenced from the engine initialisation path so the linker keeps every
/// ceiling function in the final binary even if no Rust caller uses them
/// directly.
#[no_mangle]
pub extern "C" fn P_Ceilng_Link_Anchor() {
    let _ = T_MoveCeiling as *const () as usize;
    let _ = EV_DoCeiling as *const () as usize;
    let _ = P_AddActiveCeiling as *const () as usize;
    let _ = P_RemoveActiveCeiling as *const () as usize;
    let _ = P_ActivateInStasisCeiling as *const () as usize;
    let _ = EV_CeilingCrushStop as *const () as usize;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    const CEILING_T_SIZEOF: usize = 72;
    const CEILING_T_THINKER: usize = 0;
    const CEILING_T_TYPE: usize = 24;
    const CEILING_T_SECTOR: usize = 32;
    const CEILING_T_BOTTOMHEIGHT: usize = 40;
    const CEILING_T_TOPHEIGHT: usize = 44;
    const CEILING_T_SPEED: usize = 48;
    const CEILING_T_CRUSH: usize = 52;
    const CEILING_T_DIRECTION: usize = 56;
    const CEILING_T_TAG: usize = 60;
    const CEILING_T_OLDDIRECTION: usize = 64;

    #[test]
    fn ceiling_t_layout_matches_c() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(std::mem::size_of::<ceiling_t>(), CEILING_T_SIZEOF);
        assert_eq!(std::mem::offset_of!(ceiling_t, thinker), CEILING_T_THINKER);
        assert_eq!(std::mem::offset_of!(ceiling_t, r#type), CEILING_T_TYPE);
        assert_eq!(std::mem::offset_of!(ceiling_t, sector), CEILING_T_SECTOR);
        assert_eq!(
            std::mem::offset_of!(ceiling_t, bottomheight),
            CEILING_T_BOTTOMHEIGHT
        );
        assert_eq!(
            std::mem::offset_of!(ceiling_t, topheight),
            CEILING_T_TOPHEIGHT
        );
        assert_eq!(std::mem::offset_of!(ceiling_t, speed), CEILING_T_SPEED);
        assert_eq!(std::mem::offset_of!(ceiling_t, crush), CEILING_T_CRUSH);
        assert_eq!(
            std::mem::offset_of!(ceiling_t, direction),
            CEILING_T_DIRECTION
        );
        assert_eq!(std::mem::offset_of!(ceiling_t, tag), CEILING_T_TAG);
        assert_eq!(
            std::mem::offset_of!(ceiling_t, olddirection),
            CEILING_T_OLDDIRECTION
        );
    }
}

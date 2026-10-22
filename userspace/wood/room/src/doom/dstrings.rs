//! Rust port of vendor/doomgeneric/dstrings.c.
//!
//! Globally defined quit messages, one table for Doom 1 and one for Doom 2.
//!
//! In C, `doom1_endmsg` and `doom2_endmsg` are `char *[]` arrays of mutable
//! pointers to string literals.  In Rust, string literals are `'static`
//! read-only data, so the array element type cannot be `*mut c_char`.
//! Instead, a thin `repr(transparent)` wrapper [`Ptr`] around `*const c_char`
//! is used; `unsafe impl Sync` is required because raw pointers are not
//! `Sync` by default, even though the pointers here only ever point at
//! immutable `'static` data.
//!
//! C string literals (`c"..."`) provide static NUL-terminated pointers without
//! heap allocation.
//!
//! The engine selects a message from these tables at random when the player
//! attempts to quit, using `gamemission` to pick the correct table
//! (`doom1_endmsg` for Doom 1 / Chex Quest, `doom2_endmsg` for Doom 2 /
//! TNT / Plutonia).

#![allow(non_upper_case_globals, non_snake_case)]

use std::ffi::c_char;

/// A `*const c_char` newtype that is safe to store in a `static`.
///
/// `*const c_char` does not implement `Sync`, so it cannot appear in a
/// `static` without a wrapper.  [`Ptr`] is `repr(transparent)`, guaranteeing
/// ABI compatibility with `*const c_char` for C interop: the C side can read
/// the table entries as plain `char *` values.
// `*const c_char` does not implement `Sync`, so wrap it.
// `repr(transparent)` guarantees ABI compatibility with `*const c_char` for C interop.
#[repr(transparent)]
pub struct Ptr(pub *const c_char);
unsafe impl Sync for Ptr {}

impl Ptr {
    /// Returns the inner raw pointer as `*const c_char`.
    pub fn as_ptr(&self) -> *const c_char {
        self.0
    }
}

/// Eight randomised quit messages shown when a Doom 1 player tries to exit.
///
/// Corresponds to `char *doom1_endmsg[]` in `dstrings.c`.  The game picks
/// one at random (via `M_Random`) when the quit dialog is opened in a Doom 1
/// game session.  The `#[no_mangle]` export allows C code in `m_misc.c` (or
/// equivalent) to access the table by its original symbol name.
#[no_mangle]
pub static doom1_endmsg: [Ptr; 8] = [
    Ptr(c"are you sure you want to\nquit this great game?".as_ptr()),
    Ptr(c"please don't leave, there's more\ndemons to toast!".as_ptr()),
    Ptr(c"let's beat it -- this is turning\ninto a bloodbath!".as_ptr()),
    Ptr(c"i wouldn't leave if i were you.\ndos is much worse.".as_ptr()),
    Ptr(c"you're trying to say you like dos\nbetter than me, right?".as_ptr()),
    Ptr(c"don't leave yet -- there's a\ndemon around that corner!".as_ptr()),
    Ptr(c"ya know, next time you come in here\ni'm gonna toast ya.".as_ptr()),
    Ptr(c"go ahead and leave. see if i care.".as_ptr()),
];

/// Eight randomised quit messages shown when a Doom 2 player tries to exit.
///
/// Corresponds to `char *doom2_endmsg[]` in `dstrings.c`.  Functionally
/// identical to [`doom1_endmsg`] but with Doom-II-themed text.  Used when
/// `gamemission` is `doom2`, `pack_tnt`, or `pack_plut`.
#[no_mangle]
pub static doom2_endmsg: [Ptr; 8] = [
    Ptr(c"are you sure you want to\nquit this great game?".as_ptr()),
    Ptr(c"you want to quit?\nthen, thou hast lost an eighth!".as_ptr()),
    Ptr(c"don't go now, there's a \ndimensional shambler waiting\nat the dos prompt!".as_ptr()),
    Ptr(c"get outta here and go back\nto your boring programs.".as_ptr()),
    Ptr(c"if i were your boss, i'd \n deathmatch ya in a minute!".as_ptr()),
    Ptr(c"look, bud. you leave now\nand you forfeit your body count!".as_ptr()),
    Ptr(c"just leave. when you come\nback, i'll be waiting with a bat.".as_ptr()),
    Ptr(c"you're lucky i don't smack\nyou for thinking about leaving.".as_ptr()),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doom1_table_length() {
        assert_eq!(doom1_endmsg.len(), 8);
    }

    #[test]
    fn doom2_table_length() {
        assert_eq!(doom2_endmsg.len(), 8);
    }

    #[test]
    fn doom1_first_entry_roundtrips() {
        use std::ffi::CStr;
        let s = unsafe { CStr::from_ptr(doom1_endmsg[0].0) };
        assert_eq!(
            s.to_str().unwrap(),
            "are you sure you want to\nquit this great game?"
        );
    }

    #[test]
    fn doom2_first_entry_roundtrips() {
        use std::ffi::CStr;
        let s = unsafe { CStr::from_ptr(doom2_endmsg[0].0) };
        assert_eq!(
            s.to_str().unwrap(),
            "are you sure you want to\nquit this great game?"
        );
    }

    /// Every entry in both tables must be NUL-terminated and non-empty.
    #[test]
    fn all_doom1_entries_are_valid_c_strings() {
        use std::ffi::CStr;
        for entry in doom1_endmsg.iter() {
            let s = unsafe { CStr::from_ptr(entry.0) };
            let text = s.to_str().expect("doom1_endmsg entry is invalid UTF-8");
            assert!(!text.is_empty(), "doom1_endmsg entry must not be empty");
        }
    }

    #[test]
    fn all_doom2_entries_are_valid_c_strings() {
        use std::ffi::CStr;
        for entry in doom2_endmsg.iter() {
            let s = unsafe { CStr::from_ptr(entry.0) };
            let text = s.to_str().expect("doom2_endmsg entry is invalid UTF-8");
            assert!(!text.is_empty(), "doom2_endmsg entry must not be empty");
        }
    }

    /// The two tables are independent: same index may differ between them.
    #[test]
    fn doom1_and_doom2_differ_at_index_1() {
        use std::ffi::CStr;
        let s1 = unsafe { CStr::from_ptr(doom1_endmsg[1].0) }
            .to_str()
            .unwrap();
        let s2 = unsafe { CStr::from_ptr(doom2_endmsg[1].0) }
            .to_str()
            .unwrap();
        assert_ne!(
            s1, s2,
            "index 1 messages should differ between doom1 and doom2"
        );
    }

    /// The last entry in each table is accessible.
    #[test]
    fn last_entries_are_accessible() {
        use std::ffi::CStr;
        let s1 = unsafe { CStr::from_ptr(doom1_endmsg[7].0) };
        let s2 = unsafe { CStr::from_ptr(doom2_endmsg[7].0) };
        assert!(!s1.to_str().unwrap().is_empty());
        assert!(!s2.to_str().unwrap().is_empty());
    }
}

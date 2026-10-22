//! Rust port of vendor/doomgeneric/dstrings.c.
//!
//! Globally defined quit messages, one table for Doom 1 and one for Doom 2.
//!
//! The engine selects a message from these tables at random when the player
//! attempts to quit, using `gamemission` to pick the correct table
//! (`doom1_endmsg` for Doom 1 / Chex Quest, `doom2_endmsg` for Doom 2 /
//! TNT / Plutonia).

#![allow(non_upper_case_globals, non_snake_case)]

/// Eight randomised quit messages shown when a Doom 1 player tries to exit.
///
/// Corresponds to `char *doom1_endmsg[]` in `dstrings.c`.  The game picks
/// one at random (via `M_Random`) when the quit dialog is opened in a Doom 1
/// game session.
pub const doom1_endmsg: [&str; 8] = [
    "are you sure you want to\nquit this great game?",
    "please don't leave, there's more\ndemons to toast!",
    "let's beat it -- this is turning\ninto a bloodbath!",
    "i wouldn't leave if i were you.\ndos is much worse.",
    "you're trying to say you like dos\nbetter than me, right?",
    "don't leave yet -- there's a\ndemon around that corner!",
    "ya know, next time you come in here\ni'm gonna toast ya.",
    "go ahead and leave. see if i care.",
];

/// Eight randomised quit messages shown when a Doom 2 player tries to exit.
///
/// Corresponds to `char *doom2_endmsg[]` in `dstrings.c`.  Functionally
/// identical to [`doom1_endmsg`] but with Doom-II-themed text.  Used when
/// `gamemission` is `doom2`, `pack_tnt`, or `pack_plut`.
pub const doom2_endmsg: [&str; 8] = [
    "are you sure you want to\nquit this great game?",
    "you want to quit?\nthen, thou hast lost an eighth!",
    "don't go now, there's a \ndimensional shambler waiting\nat the dos prompt!",
    "get outta here and go back\nto your boring programs.",
    "if i were your boss, i'd \n deathmatch ya in a minute!",
    "look, bud. you leave now\nand you forfeit your body count!",
    "just leave. when you come\nback, i'll be waiting with a bat.",
    "you're lucky i don't smack\nyou for thinking about leaving.",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doom1_first_entry() {
        assert_eq!(
            doom1_endmsg[0],
            "are you sure you want to\nquit this great game?"
        );
    }

    #[test]
    fn doom2_first_entry() {
        assert_eq!(
            doom2_endmsg[0],
            "are you sure you want to\nquit this great game?"
        );
    }

    /// Every entry in both tables is non-empty.
    #[test]
    fn all_entries_are_non_empty() {
        for entry in doom1_endmsg.iter().chain(doom2_endmsg.iter()) {
            assert!(!entry.is_empty());
        }
    }

    /// The two tables are independent: same index may differ between them.
    #[test]
    fn doom1_and_doom2_differ_at_index_1() {
        assert_ne!(
            doom1_endmsg[1], doom2_endmsg[1],
            "index 1 messages should differ between doom1 and doom2"
        );
    }
}

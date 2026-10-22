//! Rust port of the `DEH_String` hook from Chocolate Doom's `deh_str.c`.
//!
//! DeHackEd patches can replace engine strings and lump names at runtime.
//! This port does not load DeHackEd patches, so the hook returns its
//! argument unchanged; call sites keep it so the substitution point stays
//! visible when patch support is added.

#![allow(non_snake_case)]

/// Return the DeHackEd replacement for `s`, which is `s` itself in this build.
#[inline(always)]
pub fn DEH_String<T>(s: T) -> T {
    s
}

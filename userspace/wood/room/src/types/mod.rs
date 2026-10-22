//! Shared types used across the codebase.
//!
//! This module provides Rust-native wrappers and type aliases for C ABI types
//! that appear at the FFI boundary between Rust and the Doom C source.  Keeping
//! them here — rather than inside individual port modules — means the types are
//! available to any module without creating circular imports through `doom::`.
//!
//! # Types
//!
//! | Type | C origin | Notes |
//! |------|----------|-------|
//! | [`Boolean`] | `typedef unsigned int boolean` (`doomtype.h`) | Three-state: `FALSE`, `TRUE`, `UNDEF` |

mod doom_bool;

pub use doom_bool::Boolean;

//! A strongly-typed wrapper for Doom's C `boolean` (`typedef unsigned int boolean`).
//!
//! Doom's `boolean` is `unsigned int` in C, but behaves like an integer:
//! any non-zero value is truthy, and `0xFFFFFFFF` is used as an "undefined"
//! sentinel by the enum variant `undef`.  This module models all three states
//! explicitly and provides safe conversion helpers.

use core::ffi::c_uint;
use core::fmt;
use core::ops::Not;

/// FFI-safe boolean matching Doom's `typedef unsigned int boolean`.
///
/// The `#[repr(transparent)]` attribute guarantees the same ABI as a bare
/// `c_uint`, so values can cross the Rust/C boundary without any casting.
#[repr(transparent)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Boolean(c_uint);

/// Constants, predicates, and canonicalisation helpers for [`Boolean`].
impl Boolean {
    /// The canonical false value (`0`).
    pub const FALSE: Self = Self(0);
    /// The canonical true value (`1`).
    pub const TRUE: Self = Self(1);
    /// The "undefined" sentinel (`0xFFFFFFFF`), matching the C enum `undef`.
    pub const UNDEF: Self = Self(c_uint::MAX);

    /// Construct a `Boolean` from a raw `c_uint` without any conversion.
    #[inline]
    pub const fn from_raw(value: c_uint) -> Self {
        Self(value)
    }

    /// Return the underlying raw `c_uint` value.
    #[inline]
    pub const fn raw(self) -> c_uint {
        self.0
    }

    /// C `if (x)` semantics: any non-zero value is truthy.
    #[inline]
    pub const fn is_truthy(self) -> bool {
        self.0 != 0
    }

    /// Domain-level true: non-zero and not the `UNDEF` sentinel.
    #[inline]
    pub const fn is_true(self) -> bool {
        self.0 != 0 && self.0 != c_uint::MAX
    }

    /// Returns `true` when the value is exactly `0`.
    #[inline]
    pub const fn is_false(self) -> bool {
        self.0 == 0
    }

    /// Returns `true` when the value is the `UNDEF` sentinel (`0xFFFFFFFF`).
    #[inline]
    pub const fn is_undef(self) -> bool {
        self.0 == c_uint::MAX
    }

    /// Returns `true` when the value is one of `FALSE`, `TRUE`, or `UNDEF`.
    #[inline]
    pub const fn is_canonical(self) -> bool {
        self.0 == 0 || self.0 == 1 || self.0 == c_uint::MAX
    }

    /// Collapse any truthy value to `TRUE`; falsy to `FALSE`.
    /// Matches C's `!!x` double-negation idiom.
    #[inline]
    pub const fn canonical_truthy(self) -> Self {
        if self.is_truthy() {
            Self::TRUE
        } else {
            Self::FALSE
        }
    }

    /// Collapse any defined true value to `TRUE`; everything else to `FALSE`.
    /// `UNDEF` maps to `FALSE`.
    #[inline]
    pub const fn canonical_defined(self) -> Self {
        if self.is_true() {
            Self::TRUE
        } else {
            Self::FALSE
        }
    }
}

/// Lifts a native Rust `bool` to a [`Boolean`], using the canonical
/// `0`/`1` representation.
impl From<bool> for Boolean {
    /// `true` becomes [`Boolean::TRUE`]; `false` becomes [`Boolean::FALSE`].
    #[inline]
    fn from(value: bool) -> Self {
        if value {
            Self::TRUE
        } else {
            Self::FALSE
        }
    }
}

/// Reduces a [`Boolean`] to a native Rust `bool` using domain-level truth
/// (the `UNDEF` sentinel is treated as `false`).
impl From<Boolean> for bool {
    /// Converts using domain-level truth: `UNDEF` and `FALSE` both map to
    /// `false`; any other non-zero value maps to `true`.
    #[inline]
    fn from(value: Boolean) -> bool {
        value.is_true()
    }
}

/// `!` operator with C semantics: any truthy value becomes [`Boolean::FALSE`].
impl Not for Boolean {
    /// `!x` produces another `Boolean`.
    type Output = Boolean;

    /// Follows C's `!x` semantics: any truthy value becomes `FALSE`,
    /// and `FALSE` becomes `TRUE`.
    #[inline]
    fn not(self) -> Self::Output {
        if self.is_truthy() {
            Boolean::FALSE
        } else {
            Boolean::TRUE
        }
    }
}

/// Debug formatting that prints `Boolean::FALSE`, `Boolean::TRUE`,
/// `Boolean::UNDEF`, or `Boolean(<raw>)` for unusual values.
impl fmt::Debug for Boolean {
    /// Render the boolean using the canonical-constant name when possible,
    /// falling back to `Boolean(value)` for non-canonical bit patterns.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::FALSE => f.write_str("Boolean::FALSE"),
            Self::TRUE => f.write_str("Boolean::TRUE"),
            Self::UNDEF => f.write_str("Boolean::UNDEF"),
            Self(value) => write!(f, "Boolean({value})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn false_is_zero() {
        assert_eq!(Boolean::FALSE.raw(), 0);
        assert!(Boolean::FALSE.is_false());
        assert!(!Boolean::FALSE.is_truthy());
    }

    #[test]
    fn true_is_one() {
        assert_eq!(Boolean::TRUE.raw(), 1);
        assert!(Boolean::TRUE.is_truthy());
        assert!(!Boolean::TRUE.is_false());
    }

    #[test]
    fn undef_is_max() {
        assert_eq!(Boolean::UNDEF.raw(), c_uint::MAX);
        assert!(Boolean::UNDEF.is_undef());
        assert!(Boolean::UNDEF.is_truthy());
        assert!(!Boolean::UNDEF.is_true());
    }

    #[test]
    fn from_bool() {
        assert_eq!(Boolean::from(true), Boolean::TRUE);
        assert_eq!(Boolean::from(false), Boolean::FALSE);
    }

    #[test]
    fn into_bool() {
        assert!(bool::from(Boolean::TRUE));
        assert!(!bool::from(Boolean::FALSE));
        assert!(!bool::from(Boolean::UNDEF));
    }

    #[test]
    fn not_operator() {
        assert_eq!(!Boolean::FALSE, Boolean::TRUE);
        assert_eq!(!Boolean::TRUE, Boolean::FALSE);
        assert_eq!(!Boolean::UNDEF, Boolean::FALSE);
    }

    #[test]
    fn canonical_truthy() {
        assert_eq!(Boolean::from_raw(42).canonical_truthy(), Boolean::TRUE);
        assert_eq!(Boolean::FALSE.canonical_truthy(), Boolean::FALSE);
        assert_eq!(Boolean::UNDEF.canonical_truthy(), Boolean::TRUE);
    }

    #[test]
    fn canonical_defined() {
        assert_eq!(Boolean::from_raw(42).canonical_defined(), Boolean::TRUE);
        assert_eq!(Boolean::FALSE.canonical_defined(), Boolean::FALSE);
        assert_eq!(Boolean::UNDEF.canonical_defined(), Boolean::FALSE);
    }

    #[test]
    fn size_matches_c_uint() {
        assert_eq!(
            std::mem::size_of::<Boolean>(),
            std::mem::size_of::<c_uint>()
        );
    }

    #[test]
    fn debug_format() {
        assert_eq!(format!("{:?}", Boolean::FALSE), "Boolean::FALSE");
        assert_eq!(format!("{:?}", Boolean::TRUE), "Boolean::TRUE");
        assert_eq!(format!("{:?}", Boolean::UNDEF), "Boolean::UNDEF");
        assert_eq!(format!("{:?}", Boolean::from_raw(42)), "Boolean(42)");
    }
}

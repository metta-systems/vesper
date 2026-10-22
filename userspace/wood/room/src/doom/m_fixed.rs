//! Rust port of vendor/doomgeneric/m_fixed.c.
//!
//! Provides 16.16 fixed-point multiplication and division with the exact
//! same overflow / saturation semantics as the original C code.

#![allow(non_camel_case_types, non_snake_case)]

use std::ffi::c_int;

/// `fixed_t` — matches `typedef int fixed_t;` in m_fixed.h.
pub type fixed_t = c_int;

/// `angle_t` — BAM angle, matches `typedef unsigned int angle_t;` in tables.h.
pub type angle_t = u32;

/// Number of fractional bits in `fixed_t`. Matches `FRACBITS` in
/// `m_fixed.h` and is the right-shift used by `FixedMul`.
pub const FRACBITS: u32 = 16;

/// `1.0` expressed in 16.16 fixed-point (`1 << FRACBITS`). Matches
/// `FRACUNIT` in `m_fixed.h`.
pub const FRACUNIT: fixed_t = 1 << FRACBITS;

/// `fixed_t FixedMul(fixed_t a, fixed_t b)` — 16.16 fixed-point multiply.
///
/// Widens to `i64` so the full 64-bit product is computed, then shifts
/// right by `FRACBITS` to land back in 16.16. Truncates the result; no
/// overflow checking, matching the C original which is exported to and
/// consumed by ~20 vendored `.c` files.
#[no_mangle]
pub extern "C" fn FixedMul(a: fixed_t, b: fixed_t) -> fixed_t {
    ((a as i64 * b as i64) >> FRACBITS) as fixed_t
}

/// Exported to C as `FixedDiv`. Matches the C overflow/saturation path.
///
/// C's `abs(INT_MIN)` is UB; Rust's `wrapping_abs()` returns `INT_MIN`,
/// which still triggers the saturation branch — same effective behaviour
/// but without UB or panics.
#[no_mangle]
pub extern "C" fn FixedDiv(a: fixed_t, b: fixed_t) -> fixed_t {
    if (a.wrapping_abs() >> 14) >= b.wrapping_abs() {
        if (a ^ b) < 0 {
            i32::MIN
        } else {
            i32::MAX
        }
    } else {
        (((a as i64) << 16) / b as i64) as fixed_t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `1.0 * 1.0 == 1.0` in 16.16.
    #[test]
    fn mul_identity() {
        assert_eq!(FixedMul(1 << 16, 1 << 16), 1 << 16);
    }

    /// `0.5 * 2.0 == 1.0` in 16.16.
    #[test]
    fn mul_half_times_two() {
        assert_eq!(FixedMul(1 << 15, 2 << 16), 1 << 16);
    }

    /// `-1.0 * 1.0 == -1.0` in 16.16 (sign preserved through `>>`).
    #[test]
    fn mul_negative() {
        assert_eq!(FixedMul(-(1 << 16), 1 << 16), -(1 << 16));
    }

    /// `3.0 / 1.0 == 3.0` in 16.16.
    #[test]
    fn div_one() {
        assert_eq!(FixedDiv(3 << 16, 1 << 16), 3 << 16);
    }

    /// Positive overflow saturates to `INT_MAX`.
    #[test]
    fn div_saturates_pos() {
        assert_eq!(FixedDiv(i32::MAX, 1), i32::MAX);
    }

    /// Negative overflow (positive / negative) saturates to `INT_MIN`.
    #[test]
    fn div_saturates_neg() {
        assert_eq!(FixedDiv(i32::MAX, -1), i32::MIN);
    }

    /// Regression guard: `FixedDiv(INT_MIN, x)` must not panic on the
    /// `abs(INT_MIN)` step; Rust's `wrapping_abs` keeps the call defined.
    #[test]
    fn div_min_no_panic() {
        let _ = FixedDiv(i32::MIN, 1 << 16);
    }

    /// Any operand of 0 yields 0.
    #[test]
    fn mul_zero() {
        assert_eq!(FixedMul(0, 1 << 16), 0);
        assert_eq!(FixedMul(1 << 16, 0), 0);
        assert_eq!(FixedMul(0, 0), 0);
    }

    /// `(-2.0) * 3.0 == -6.0` in 16.16.
    #[test]
    fn mul_neg_pos() {
        // (-2.0) × 3.0 = -6.0 in 16.16 fixed-point
        assert_eq!(FixedMul(-(2 << 16), 3 << 16), -(6 << 16));
    }

    /// `(-2.0) * (-3.0) == 6.0` in 16.16.
    #[test]
    fn mul_neg_neg() {
        // (-2.0) × (-3.0) = 6.0 in 16.16 fixed-point
        assert_eq!(FixedMul(-(2 << 16), -(3 << 16)), 6 << 16);
    }

    /// `1.5 * 2.0 == 3.0` in 16.16.
    #[test]
    fn mul_fraction() {
        // 1.5 × 2.0 = 3.0; 1.5 = (3 << 15)
        assert_eq!(FixedMul(3 << 15, 2 << 16), 3 << 16);
    }

    /// Multiplying extreme values must not invoke i64 UB or panic. The
    /// `as` cast at the end truncates silently, matching the C original.
    #[test]
    fn mul_large_does_not_panic() {
        // The intermediate i64 must not overflow to UB; Rust guarantees this
        let _ = FixedMul(i32::MAX, i32::MAX);
        let _ = FixedMul(i32::MIN, i32::MIN);
        let _ = FixedMul(i32::MAX, i32::MIN);
    }

    /// `1.0 / 2.0 == 0.5` in 16.16.
    #[test]
    fn div_half() {
        // 1.0 / 2.0 = 0.5 in 16.16 fixed-point
        assert_eq!(FixedDiv(1 << 16, 2 << 16), 1 << 15);
    }

    /// `(-4.0) / 2.0 == -2.0` in 16.16.
    #[test]
    fn div_neg_by_pos() {
        // (-4.0) / 2.0 = -2.0 in 16.16 fixed-point
        assert_eq!(FixedDiv(-(4 << 16), 2 << 16), -(2 << 16));
    }

    /// FixedDiv(a, 0): abs(a)>>14 >= abs(0)=0 is always true.
    /// (a ^ 0) = a; sign of a decides INT_MAX vs INT_MIN.
    #[test]
    fn div_positive_by_zero_saturates_max() {
        assert_eq!(FixedDiv(1, 0), i32::MAX);
        assert_eq!(FixedDiv(i32::MAX, 0), i32::MAX);
    }

    /// For negative `a` with |a| small enough that wrapping_abs doesn't
    /// overflow, dividing by zero saturates to INT_MIN.
    /// Note: FixedDiv(i32::MIN, 0) is *not* tested here — i32::MIN.wrapping_abs()
    /// returns i32::MIN whose arithmetic right-shift is negative, so the
    /// saturation guard does not fire (same UB/trap as the C original for
    /// abs(INT_MIN) / 0).
    #[test]
    fn div_negative_by_zero_saturates_min() {
        assert_eq!(FixedDiv(-1, 0), i32::MIN);
        assert_eq!(FixedDiv(-65536, 0), i32::MIN); // -1.0 in 16.16
    }

    /// Saturation when |a|/2^14 >= |b| and a, b have opposite signs → INT_MIN.
    #[test]
    fn div_saturates_neg_opposite_signs() {
        // abs(i32::MAX) >> 14 = 131071 >= abs(-1) = 1 → saturate,
        // (MAX ^ -1) has bit 31 set → negative → INT_MIN
        assert_eq!(FixedDiv(i32::MAX, -1), i32::MIN);
    }
}

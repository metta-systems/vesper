//! Tests for `i_scale.c` — screen-scaling mode descriptors.
//!
//! `i_scale.c` defines 15 `screen_mode_t` instances that describe how to
//! upscale the 320×200 game framebuffer to larger display resolutions:
//!
//!   * **scale**   — integer pixel doubling (1×–5×)
//!   * **stretch** — vertical 4:3 correction (320×200 → 320×240 base)
//!   * **squash**  — horizontal 4:3 correction (320×200 → 256×200 base)
//!
//! The `screen_mode_t` struct carries `width`, `height`, a `DrawScreen`
//! function pointer, and a `poor_quality` flag; none of these require the
//! display to be initialized.
//!
//! These tests verify:
//!   * Screen constants (`c_ffi::SCREENWIDTH_4_3`, `c_ffi::SCREENHEIGHT_4_3`)
//!   * `screen_mode_t` struct layout (must match the C layout exactly)
//!   * Dimensions of every named mode
//!   * `poor_quality` flag for the lowest-resolution modes
//!   * `draw_screen` function pointer is non-null (the mode is usable)

#![allow(non_snake_case)]

use crate::doom::c_ffi;
use crate::doom::i_video::{SCREENHEIGHT, SCREENWIDTH};

// ---------------------------------------------------------------------------
// Screen dimension constants
// ---------------------------------------------------------------------------

/// `c_ffi::SCREENWIDTH_4_3 = 256` — the horizontal resolution used by the squash
/// modes.  The "correct" value should be ~266, but vanilla used 256 because
/// it recycled the same blend tables as the stretch modes.
#[test]
fn screenwidth_4_3_is_256() {
    assert_eq!(c_ffi::SCREENWIDTH_4_3, 256);
}

/// `c_ffi::SCREENHEIGHT_4_3 = 240` — the vertical resolution used by the stretch
/// modes to achieve a 4:3 aspect ratio from the 320×200 source.
#[test]
fn screenheight_4_3_is_240() {
    assert_eq!(c_ffi::SCREENHEIGHT_4_3, 240);
}

// ---------------------------------------------------------------------------
// screen_mode_t struct layout
// ---------------------------------------------------------------------------

/// On 64-bit Linux the struct layout is:
///   +0  width         (int,    4 bytes)
///   +4  height        (int,    4 bytes)
///   +8  InitMode      (ptr,    8 bytes)
///   +16 DrawScreen    (ptr,    8 bytes)
///   +24 poor_quality  (int,    4 bytes)
///   +28 [4 bytes tail-padding to align struct to 8]
/// Total: 32 bytes.
#[test]
fn screen_mode_t_size_is_32() {
    assert_eq!(std::mem::size_of::<c_ffi::screen_mode_t>(), 32);
}

/// `screen_mode_t` alignment is 8 — the largest member is a pointer
/// (`InitMode`/`DrawScreen`) which on 64-bit Linux is 8-byte aligned.
#[test]
fn screen_mode_t_align_is_8() {
    assert_eq!(std::mem::align_of::<c_ffi::screen_mode_t>(), 8);
}

// ---------------------------------------------------------------------------
// Scale modes (direct pixel doubling)
// ---------------------------------------------------------------------------

/// 1× scale: output is 320×200 — exactly the same as the source buffer.
#[test]
fn mode_scale_1x_is_320x200() {
    unsafe {
        assert_eq!(c_ffi::mode_scale_1x.width, 320, "mode_scale_1x.width");
        assert_eq!(c_ffi::mode_scale_1x.height, 200, "mode_scale_1x.height");
    }
}

/// 2× scale: each source pixel becomes a 2×2 block → 640×400.
#[test]
fn mode_scale_2x_is_640x400() {
    unsafe {
        assert_eq!(c_ffi::mode_scale_2x.width, 640, "mode_scale_2x.width");
        assert_eq!(c_ffi::mode_scale_2x.height, 400, "mode_scale_2x.height");
    }
}

/// 3× scale: 960×600.
#[test]
fn mode_scale_3x_is_960x600() {
    unsafe {
        assert_eq!(c_ffi::mode_scale_3x.width, 960, "mode_scale_3x.width");
        assert_eq!(c_ffi::mode_scale_3x.height, 600, "mode_scale_3x.height");
    }
}

/// 4× scale: 1280×800.
#[test]
fn mode_scale_4x_is_1280x800() {
    unsafe {
        assert_eq!(c_ffi::mode_scale_4x.width, 1280, "mode_scale_4x.width");
        assert_eq!(c_ffi::mode_scale_4x.height, 800, "mode_scale_4x.height");
    }
}

/// 5× scale: 1600×1000.
#[test]
fn mode_scale_5x_is_1600x1000() {
    unsafe {
        assert_eq!(c_ffi::mode_scale_5x.width, 1600, "mode_scale_5x.width");
        assert_eq!(c_ffi::mode_scale_5x.height, 1000, "mode_scale_5x.height");
    }
}

/// All scale-mode widths are `N × SCREENWIDTH` for N in 1..=5.
#[test]
fn scale_mode_widths_are_multiples_of_screenwidth() {
    unsafe {
        let modes = [
            (1, c_ffi::mode_scale_1x.width),
            (2, c_ffi::mode_scale_2x.width),
            (3, c_ffi::mode_scale_3x.width),
            (4, c_ffi::mode_scale_4x.width),
            (5, c_ffi::mode_scale_5x.width),
        ];
        for (n, w) in modes {
            assert_eq!(w, n * SCREENWIDTH, "mode_scale_{n}x width mismatch");
        }
    }
}

/// All scale-mode heights are `N × SCREENHEIGHT` for N in 1..=5.
#[test]
fn scale_mode_heights_are_multiples_of_screenheight() {
    unsafe {
        let modes = [
            (1, c_ffi::mode_scale_1x.height),
            (2, c_ffi::mode_scale_2x.height),
            (3, c_ffi::mode_scale_3x.height),
            (4, c_ffi::mode_scale_4x.height),
            (5, c_ffi::mode_scale_5x.height),
        ];
        for (n, h) in modes {
            assert_eq!(h, n * SCREENHEIGHT, "mode_scale_{n}x height mismatch");
        }
    }
}

/// None of the scale modes are marked as `poor_quality`.
/// Scale modes do pure pixel-doubling with no blending, so visual quality
/// degrades only at very low resolutions — which is not the case here.
#[test]
fn scale_modes_not_poor_quality() {
    unsafe {
        assert_eq!(
            c_ffi::mode_scale_1x.poor_quality,
            0,
            "mode_scale_1x.poor_quality"
        );
        assert_eq!(
            c_ffi::mode_scale_2x.poor_quality,
            0,
            "mode_scale_2x.poor_quality"
        );
        assert_eq!(
            c_ffi::mode_scale_3x.poor_quality,
            0,
            "mode_scale_3x.poor_quality"
        );
        assert_eq!(
            c_ffi::mode_scale_4x.poor_quality,
            0,
            "mode_scale_4x.poor_quality"
        );
        assert_eq!(
            c_ffi::mode_scale_5x.poor_quality,
            0,
            "mode_scale_5x.poor_quality"
        );
    }
}

/// All scale modes must have a non-null `draw_screen` function pointer.
#[test]
fn scale_modes_have_draw_fn() {
    unsafe {
        assert!(
            c_ffi::mode_scale_1x.draw_screen.is_some(),
            "mode_scale_1x.draw_screen should not be null"
        );
        assert!(
            c_ffi::mode_scale_2x.draw_screen.is_some(),
            "mode_scale_2x.draw_screen should not be null"
        );
        assert!(
            c_ffi::mode_scale_5x.draw_screen.is_some(),
            "mode_scale_5x.draw_screen should not be null"
        );
    }
}

// ---------------------------------------------------------------------------
// Stretch modes (4:3 vertical correction)
// ---------------------------------------------------------------------------

/// 1× stretch: 320×240 — adds 40 extra lines to correct 320×200 to 4:3.
/// This is marked `poor_quality` because at this small size every pixel must
/// be interpolated with its neighbour.
#[test]
fn mode_stretch_1x_is_320x240() {
    unsafe {
        assert_eq!(c_ffi::mode_stretch_1x.width, 320, "mode_stretch_1x.width");
        assert_eq!(c_ffi::mode_stretch_1x.height, 240, "mode_stretch_1x.height");
    }
}

/// The 1× stretch mode is flagged as poor quality (blended interpolation
/// is visible at such low resolution).
#[test]
fn mode_stretch_1x_is_poor_quality() {
    unsafe {
        assert_ne!(
            c_ffi::mode_stretch_1x.poor_quality,
            0,
            "mode_stretch_1x.poor_quality should be true"
        );
    }
}

/// 2× stretch: 640×480.  Original pixels appear twice in both axes, so
/// this mode is *not* poor quality.
#[test]
fn mode_stretch_2x_is_640x480() {
    unsafe {
        assert_eq!(c_ffi::mode_stretch_2x.width, 640, "mode_stretch_2x.width");
        assert_eq!(c_ffi::mode_stretch_2x.height, 480, "mode_stretch_2x.height");
        assert_eq!(
            c_ffi::mode_stretch_2x.poor_quality,
            0,
            "mode_stretch_2x.poor_quality"
        );
    }
}

/// 3× stretch: 960×720.
#[test]
fn mode_stretch_3x_is_960x720() {
    unsafe {
        assert_eq!(c_ffi::mode_stretch_3x.width, 960);
        assert_eq!(c_ffi::mode_stretch_3x.height, 720);
    }
}

/// 4× stretch: 1280×960.
#[test]
fn mode_stretch_4x_is_1280x960() {
    unsafe {
        assert_eq!(c_ffi::mode_stretch_4x.width, 1280);
        assert_eq!(c_ffi::mode_stretch_4x.height, 960);
    }
}

/// 5× stretch: 1600×1200.
#[test]
fn mode_stretch_5x_is_1600x1200() {
    unsafe {
        assert_eq!(c_ffi::mode_stretch_5x.width, 1600);
        assert_eq!(c_ffi::mode_stretch_5x.height, 1200);
    }
}

/// Stretch-mode widths are `N × SCREENWIDTH` for N in 1..=5.
#[test]
fn stretch_mode_widths_are_n_times_screenwidth() {
    unsafe {
        let modes = [
            (1, c_ffi::mode_stretch_1x.width),
            (2, c_ffi::mode_stretch_2x.width),
            (3, c_ffi::mode_stretch_3x.width),
            (4, c_ffi::mode_stretch_4x.width),
            (5, c_ffi::mode_stretch_5x.width),
        ];
        for (n, w) in modes {
            assert_eq!(w, n * SCREENWIDTH, "mode_stretch_{n}x width mismatch");
        }
    }
}

/// Stretch-mode heights are `N × c_ffi::SCREENHEIGHT_4_3` for N in 1..=5.
#[test]
fn stretch_mode_heights_are_n_times_screenheight_4_3() {
    unsafe {
        let modes = [
            (1, c_ffi::mode_stretch_1x.height),
            (2, c_ffi::mode_stretch_2x.height),
            (3, c_ffi::mode_stretch_3x.height),
            (4, c_ffi::mode_stretch_4x.height),
            (5, c_ffi::mode_stretch_5x.height),
        ];
        for (n, h) in modes {
            assert_eq!(
                h,
                n * c_ffi::SCREENHEIGHT_4_3,
                "mode_stretch_{n}x height mismatch"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Squash modes (4:3 horizontal correction)
// ---------------------------------------------------------------------------

/// 1× squash: 256×200.  Each block of 5 source pixels is blended into 4
/// destination pixels, shrinking the width from 320 to 256.  This is poor
/// quality because some source pixels are entirely discarded.
#[test]
fn mode_squash_1x_is_256x200() {
    unsafe {
        assert_eq!(c_ffi::mode_squash_1x.width, 256, "mode_squash_1x.width");
        assert_eq!(c_ffi::mode_squash_1x.height, 200, "mode_squash_1x.height");
    }
}

/// The 1× squash mode is poor quality.
#[test]
fn mode_squash_1x_is_poor_quality() {
    unsafe {
        assert_ne!(
            c_ffi::mode_squash_1x.poor_quality,
            0,
            "mode_squash_1x.poor_quality should be true"
        );
    }
}

/// 2× squash: 512×400.
#[test]
fn mode_squash_2x_is_512x400() {
    unsafe {
        assert_eq!(c_ffi::mode_squash_2x.width, 512);
        assert_eq!(c_ffi::mode_squash_2x.height, 400);
    }
}

/// 3× squash: 800×600.  This is a quirk of the original C code: rather than
/// using the formula `c_ffi::SCREENWIDTH_4_3 × 3 = 768`, vanilla used the nearest
/// standard monitor resolution (800×600).
#[test]
fn mode_squash_3x_is_800x600() {
    unsafe {
        assert_eq!(c_ffi::mode_squash_3x.width, 800);
        assert_eq!(c_ffi::mode_squash_3x.height, 600);
    }
}

/// 4× squash: 1024×800.
#[test]
fn mode_squash_4x_is_1024x800() {
    unsafe {
        assert_eq!(c_ffi::mode_squash_4x.width, 1024);
        assert_eq!(c_ffi::mode_squash_4x.height, 800);
    }
}

/// 5× squash: 1280×1000.
#[test]
fn mode_squash_5x_is_1280x1000() {
    unsafe {
        assert_eq!(c_ffi::mode_squash_5x.width, 1280);
        assert_eq!(c_ffi::mode_squash_5x.height, 1000);
    }
}

/// Squash-mode widths are `N × c_ffi::SCREENWIDTH_4_3` for N in 1, 2, 4, 5.
/// Mode 3× is special-cased to 800 (a standard monitor resolution) rather
/// than the formula value of 768 — this is an original C quirk.
#[test]
fn squash_mode_widths_are_n_times_screenwidth_4_3() {
    unsafe {
        let modes = [
            (1, c_ffi::mode_squash_1x.width),
            (2, c_ffi::mode_squash_2x.width),
            // 3x is deliberately skipped — it uses 800 instead of 768
            (4, c_ffi::mode_squash_4x.width),
            (5, c_ffi::mode_squash_5x.width),
        ];
        for (n, w) in modes {
            assert_eq!(
                w,
                n * c_ffi::SCREENWIDTH_4_3,
                "mode_squash_{n}x width mismatch"
            );
        }
        // The 3x quirk: 800 ≠ 3 × 256
        assert_eq!(
            c_ffi::mode_squash_3x.width,
            800,
            "mode_squash_3x.width quirk"
        );
        assert_ne!(
            c_ffi::mode_squash_3x.width,
            3 * c_ffi::SCREENWIDTH_4_3,
            "mode_squash_3x should NOT follow the formula"
        );
    }
}

/// Squash-mode heights are `N × SCREENHEIGHT` for N in 1..=5.
#[test]
fn squash_mode_heights_are_n_times_screenheight() {
    unsafe {
        let modes = [
            (1, c_ffi::mode_squash_1x.height),
            (2, c_ffi::mode_squash_2x.height),
            (3, c_ffi::mode_squash_3x.height),
            (4, c_ffi::mode_squash_4x.height),
            (5, c_ffi::mode_squash_5x.height),
        ];
        for (n, h) in modes {
            assert_eq!(h, n * SCREENHEIGHT, "mode_squash_{n}x height mismatch");
        }
    }
}

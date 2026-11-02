//! Generates the synthesiser's lookup tables into `$OUT_DIR/tables.rs`.
//!
//! The library itself never touches floating point (Vesper builds soft-float
//! and traps FP/SIMD); the exponentials, logarithms and the pan law it needs
//! are computed here, on the build host, and baked in as integer constants.

#![expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "every table value is rounded and non-negative, far below u64::MAX"
)]

use std::{env, fmt::Write as _, fs, path::Path};

/// `round(2^(cents / 1200) * 2^30)` for `cents` in `0..1200`: one octave of
/// pitch (and time) ratios in Q2.30.
fn cent_ratios() -> Vec<u64> {
    (0..1200)
        .map(|cents| {
            (2_f64.powf(f64::from(cents) / 1200.0) * f64::from(1_u32 << 30)).round() as u64
        })
        .collect()
}

/// `round(65536 * 10^(-centibels / 200))` for `centibels` in `0..=960`: the
/// linear amplitude of an attenuation, in Q16.
fn attenuation_gains() -> Vec<u64> {
    (0..=960)
        .map(|centibels| (65536.0 * 10_f64.powf(-f64::from(centibels) / 200.0)).round() as u64)
        .collect()
}

/// The SoundFont's default concave curve for velocity and the volume and
/// expression controllers, as attenuation in centibels: an amplitude of
/// `(value / 127)^2`, so `400 * log10(127 / value)`, 960 for `value` 0.
fn concave_attenuations() -> Vec<u64> {
    (0..128)
        .map(|value| {
            if value == 0 {
                960
            } else {
                (400.0 * (127.0 / f64::from(value)).log10())
                    .round()
                    .min(960.0) as u64
            }
        })
        .collect()
}

/// Constant-power pan law in Q15: `round(32768 * cos(position / 1000 * pi/2))`
/// for `position` in `0..=1000` (0 is hard left). The left gain of SoundFont
/// pan `p` (-500 to 500) is entry `p + 500`, the right gain entry `500 - p`.
fn pan_gains() -> Vec<u64> {
    (0..=1000)
        .map(|position| {
            (32768.0 * (f64::from(position) / 1000.0 * std::f64::consts::FRAC_PI_2).cos()).round()
                as u64
        })
        .collect()
}

fn emit(out: &mut String, doc: &str, name: &str, element: &str, values: &[u64]) {
    writeln!(out, "/// {doc}").unwrap();
    writeln!(
        out,
        "pub(crate) static {name}: [{element}; {}] = [",
        values.len()
    )
    .unwrap();
    for chunk in values.chunks(12) {
        let line: Vec<String> = chunk.iter().map(u64::to_string).collect();
        writeln!(out, "    {},", line.join(", ")).unwrap();
    }
    writeln!(out, "];").unwrap();
}

fn main() {
    let mut out = String::new();
    emit(
        &mut out,
        "`2^(cents / 1200)` in Q2.30 for one octave of cents.",
        "CENT_RATIOS_Q30",
        "u32",
        &cent_ratios(),
    );
    emit(
        &mut out,
        "Linear amplitude in Q16 of an attenuation of 0 to 960 centibels.",
        "ATTENUATION_GAINS_Q16",
        "u32",
        &attenuation_gains(),
    );
    emit(
        &mut out,
        "Attenuation in centibels of the concave velocity/volume curve, by value.",
        "CONCAVE_CENTIBELS",
        "u16",
        &concave_attenuations(),
    );
    emit(
        &mut out,
        "Constant-power pan gains in Q15, by pan position 0 (left) to 1000 (right).",
        "PAN_GAINS_Q15",
        "u16",
        &pan_gains(),
    );
    let path = Path::new(&env::var("OUT_DIR").unwrap()).join("tables.rs");
    fs::write(path, out).unwrap();
    println!("cargo::rerun-if-changed=build.rs");
}

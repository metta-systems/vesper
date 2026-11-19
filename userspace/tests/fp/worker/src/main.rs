#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! An FP/SIMD component. It is built for the hard-float target (it is on the
//! Justfile's `fp_components` list), so the compiler uses FP/SIMD registers
//! freely, and image-build's FP scan flags it, so it is loaded into an
//! `AddressSpace` that allows FP/SIMD.

use libuser::semihosting as semi;

libuser::entry!(main);

/// A floating-point computation the compiler cannot fold away: the sum of
/// `1 / k²` for `k` in `1..=terms`, which approaches `π² / 6`.
#[inline(never)]
fn basel_sum(terms: u32) -> f64 {
    let mut sum = 0.0_f64;
    for k in 1..=terms {
        let k = f64::from(core::hint::black_box(k));
        sum += 1.0 / (k * k);
    }
    sum
}

fn main(argument: u64) -> ! {
    let terms = u32::try_from(argument).unwrap_or(u32::MAX).max(1);
    let sum = basel_sum(terms);
    semi::println!("fp-worker: basel sum {sum}");
    core::hint::black_box(sum);
    loop {
        core::hint::spin_loop();
    }
}

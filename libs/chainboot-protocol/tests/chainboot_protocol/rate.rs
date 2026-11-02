use vesper_chainboot_protocol::*;

const MHZ_48: u32 = 48_000_000;
const MHZ_3: u32 = 3_000_000;
const MHZ_4: u32 = 4_000_000;

#[test]
fn fast_rates_at_48_mhz_are_accepted() {
    assert_eq!(validate_rate(MHZ_48, 2_000_000), Ok((1, 32)));
    assert_eq!(validate_rate(MHZ_48, 1_500_000), Ok((2, 0)));
    // 921600 comes out at 923076, 0.16% fast.
    assert_eq!(validate_rate(MHZ_48, 921_600), Ok((3, 16)));
    assert_eq!(actual_rate(MHZ_48, 3, 16), 923_076);
    // The fastest the PL011 goes: the clock over sixteen.
    assert_eq!(validate_rate(MHZ_48, 3_000_000), Ok((1, 0)));
    assert_eq!(validate_rate(MHZ_48, BOOT_BAUD_RATE), Ok((26, 3)));
}

#[test]
fn exact_divisors_make_the_rate_exactly() {
    assert_eq!(actual_rate(MHZ_48, 1, 32), 2_000_000);
    assert_eq!(actual_rate(MHZ_48, 2, 0), 1_500_000);
    assert_eq!(actual_rate(MHZ_48, 1, 0), 3_000_000);
}

#[test]
fn rates_over_the_clock_over_sixteen_are_refused() {
    for (clock, rate) in [
        (MHZ_48, 4_000_000),
        (MHZ_48, 3_100_000),
        (MHZ_3, 2_000_000),
        (MHZ_3, 921_600),
        (MHZ_4, 2_000_000),
        (MHZ_4, 1_000_000),
    ] {
        assert_eq!(
            validate_rate(clock, rate),
            Err(RateRefusal::OutOfRange { clock, rate }),
            "{rate} at {clock}"
        );
    }
}

#[test]
fn absurd_rates_are_refused() {
    for rate in [0, 1, 10, 45, u32::MAX] {
        assert!(
            matches!(
                validate_rate(MHZ_48, rate),
                Err(RateRefusal::OutOfRange { .. })
            ),
            "{rate}"
        );
    }
}

#[test]
fn slow_clocks_still_make_the_boot_rate() {
    // 3 MHz: 1 + 40/64, 115384 baud, 0.16% fast.
    assert_eq!(validate_rate(MHZ_3, BOOT_BAUD_RATE), Ok((1, 40)));
    // 4 MHz: 2 + 11/64.
    assert_eq!(validate_rate(MHZ_4, BOOT_BAUD_RATE), Ok((2, 11)));
    // The fastest each goes.
    assert_eq!(validate_rate(MHZ_3, 187_500), Ok((1, 0)));
    assert_eq!(validate_rate(MHZ_4, 250_000), Ok((1, 0)));
    // 230400 at 4 MHz: 1 + 5/64, 231884 baud, 0.64% fast.
    assert_eq!(validate_rate(MHZ_4, 230_400), Ok((1, 5)));
}

#[test]
fn the_slowest_divisor_takes_no_fraction() {
    // 65535 is the largest integer divisor, and then only with no fraction.
    assert_eq!(divisors(16 * 65_535, 1), Some((65_535, 0)));
    assert_eq!(divisors(16 * 65_535 + 8, 1), None);
    assert_eq!(divisors(16 * 65_536, 1), None);
}

#[test]
fn whatever_the_registers_hold_is_within_the_limit() {
    // With an integer divisor of at least 1 the fraction's step is under 1/64 of
    // the divisor, so the error stays under about 1.2%: the 2% check is a guard
    // that today's rounding never trips. Sweep every clock the board may report.
    for clock in [MHZ_3, MHZ_4, MHZ_48, 1_600_000] {
        let mut rate = 50;
        while rate <= clock / 16 {
            if let Some((integer, fraction)) = divisors(clock, rate) {
                let actual = actual_rate(clock, integer, fraction);
                let error_ppm = u64::from(actual.abs_diff(rate)) * 1_000_000 / u64::from(rate);
                assert!(
                    error_ppm <= MAX_RATE_ERROR_PPM,
                    "{rate} at {clock}: {error_ppm} ppm"
                );
                assert_eq!(validate_rate(clock, rate), Ok((integer, fraction)));
            }
            rate += rate / 97 + 1;
        }
    }
}

#[test]
fn a_rate_too_far_off_says_by_how_much() {
    let refusal = RateRefusal::TooFarOff {
        rate: 100_000,
        actual: 97_000,
        error_ppm: 30_000,
    };
    assert_eq!(
        refusal.to_string(),
        "100000 baud comes out at 97000 baud, 30000 ppm off (at most 20000)"
    );
}

#[test]
fn refusals_say_why() {
    let out_of_range = RateRefusal::OutOfRange {
        clock: MHZ_48,
        rate: 4_000_000,
    };
    assert_eq!(
        out_of_range.to_string(),
        "4000000 baud is out of range for the 48000000 Hz UART clock (at most 3000000 baud)"
    );
    assert_eq!(
        RateRefusal::Garbled.to_string(),
        "the rate request's check byte does not match"
    );
}

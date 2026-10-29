//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! The protocol chainboot (on the board) and chainofcommand (on the host) speak
//! over the serial line, in one place so the two cannot drift apart.
//!
//! # The load
//!
//! Chainboot starts at [`BOOT_BAUD_RATE`] and sends [`BEACON`] while it waits
//! ([`BOOT_RATE_WAIT`]). The host answers with the kernel's size, a little-endian
//! `u64`; chainboot says `OK` (or `ERR …` and waits again), the host sends the
//! image and then its seahash checksum as a little-endian `u64`, and chainboot says
//! `OK` (or `ERR …` and waits again).
//!
//! # The rate request
//!
//! Instead of the size, the host may first ask for a faster rate with a
//! [`RATE_REQUEST_LENGTH`]-byte message ([`encode_rate_request`]):
//!
//! | bytes | what |
//! |-------|------|
//! | 0..8  | [`RATE_REQUEST_MAGIC`], `BAUDRATE` in ASCII |
//! | 8..12 | the rate in baud, a little-endian `u32` |
//! | 12    | a check byte: the five bytes from 8 on add up to zero, modulo 256 |
//!
//! Chainboot reads the first eight bytes as it would read a size. Taken as a
//! size, the magic is over 2⁶², which no kernel is: an older chainboot refuses it
//! as too big and waits again, and a size can never be taken for the magic
//! ([`opening`]).
//!
//! Chainboot answers at the rate it is at: [`RATE_ACCEPTED`], or [`RATE_REFUSED`]
//! followed by the reason on a line of its own, and waits again at the rate it was
//! at. It accepts a rate its UART clock can make within [`MAX_RATE_ERROR_PPM`]
//! ([`validate_rate`]). On accepting, it lets the answer leave the line, switches,
//! waits [`SWITCH_SETTLE`] for the host to switch too, and beacons at the new rate
//! ([`FAST_RATE_WAIT`]). The host switches when it reads [`RATE_ACCEPTED`], and the
//! load goes on at the new rate as above.
//!
//! # Falling back
//!
//! When the host does not answer a beacon at the new rate within
//! [`FAST_RATE_WAIT`], chainboot goes back to [`BOOT_RATE_WAIT`] at
//! [`BOOT_BAUD_RATE`], so a rate the line cannot carry never loses the board. The
//! host gives up on the new rate after [`HOST_FAST_BEACON_TIMEOUT`], earlier than
//! chainboot does, so it is back at the boot rate for chainboot's first beacon
//! there. Every wait that does not follow an accepted rate request, after a failed
//! load too, starts at [`BOOT_BAUD_RATE`].
//!
//! After a load that succeeds, chainboot jumps to the kernel at the rate the load
//! ran at, and the host stays at it for the console.

#![no_std]

use core::{fmt, time::Duration};

/// The rate chainboot starts at, and waits for the host at, unless a rate request
/// was just accepted.
pub const BOOT_BAUD_RATE: u32 = 115_200;

/// What chainboot sends while it waits for the host: three `^C`.
pub const BEACON: [u8; 3] = [3; 3];

/// The first eight bytes of a rate request, where a load starts with the size.
pub const RATE_REQUEST_MAGIC: [u8; 8] = *b"BAUDRATE";

/// The whole rate request: the magic, the rate and the check byte.
pub const RATE_REQUEST_LENGTH: usize = RATE_REQUEST_MAGIC.len() + RATE_REQUEST_BODY_LENGTH;

/// What follows the magic: the rate and the check byte.
pub const RATE_REQUEST_BODY_LENGTH: usize = 5;

/// Chainboot's answer when it switches to the requested rate.
pub const RATE_ACCEPTED: &str = "BAUD OK";

/// Chainboot's answer when it stays at its rate. A line with the reason follows.
pub const RATE_REFUSED: &str = "BAUD NO";

// The host tells the answers apart by their first characters.
const _: () = assert!(RATE_ACCEPTED.len() == RATE_REFUSED.len());

/// How far the rate the divisors make may be from the requested one, in parts
/// per million: 2%. A frame of ten bits is sampled in the middle of each bit, so
/// the two ends together may be some 4% apart; each end gets half of it. The
/// divisors' rounding stays within about 1.2% for any rate the registers hold, so
/// this is a guard against a future change to it more than a limit that bites.
pub const MAX_RATE_ERROR_PPM: u64 = 20_000;

/// How long chainboot waits after switching before it beacons at the new rate,
/// so that the host has switched too.
pub const SWITCH_SETTLE: Duration = Duration::from_millis(100);

/// How chainboot waits for the host: it beacons every `beacon_period`, and gives
/// up after `give_up_after`, if it ever does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wait {
    pub beacon_period: Duration,
    pub give_up_after: Option<Duration>,
}

impl Wait {
    /// Whether chainboot stops waiting, having waited `waited` without hearing
    /// from the host. It is asked before every beacon.
    #[must_use]
    pub const fn gives_up(&self, waited: Duration) -> bool {
        match self.give_up_after {
            Some(limit) => waited.as_millis() >= limit.as_millis(),
            None => false,
        }
    }
}

/// Waiting at [`BOOT_BAUD_RATE`]: a beacon every five seconds, for ever.
pub const BOOT_RATE_WAIT: Wait = Wait {
    beacon_period: Duration::from_secs(5),
    give_up_after: None,
};

/// Waiting at a rate just accepted: a beacon every half second, for three
/// seconds, then back to [`BOOT_RATE_WAIT`].
pub const FAST_RATE_WAIT: Wait = Wait {
    beacon_period: Duration::from_millis(500),
    give_up_after: Some(Duration::from_secs(3)),
};

/// How long the host waits for a beacon at the new rate before it goes back to
/// the boot rate. It is short of chainboot's limit by a beacon period, so the
/// host is back at the boot rate before chainboot's first beacon there.
pub const HOST_FAST_BEACON_TIMEOUT: Duration = Duration::from_secs(2);

const _: () = assert!(
    HOST_FAST_BEACON_TIMEOUT.as_millis() + FAST_RATE_WAIT.beacon_period.as_millis()
        <= match FAST_RATE_WAIT.give_up_after {
            Some(limit) => limit.as_millis(),
            None => u128::MAX,
        }
);

/// How a load starts: the first eight bytes from the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opening {
    /// The kernel's size.
    Size(u64),
    /// A rate request: [`RATE_REQUEST_BODY_LENGTH`] bytes follow.
    RateRequest,
}

/// What the first eight bytes from the host start.
#[must_use]
pub fn opening(first: [u8; 8]) -> Opening {
    if first == RATE_REQUEST_MAGIC {
        Opening::RateRequest
    } else {
        Opening::Size(u64::from_le_bytes(first))
    }
}

/// The check byte for the rate's bytes: the five add up to zero, modulo 256.
#[must_use]
pub fn check_byte(rate_bytes: [u8; 4]) -> u8 {
    rate_bytes
        .iter()
        .fold(0_u8, |sum, byte| sum.wrapping_add(*byte))
        .wrapping_neg()
}

/// The rate request for `rate` baud.
#[must_use]
pub fn encode_rate_request(rate: u32) -> [u8; RATE_REQUEST_LENGTH] {
    let rate_bytes = rate.to_le_bytes();
    let mut request = [0; RATE_REQUEST_LENGTH];
    request[..8].copy_from_slice(&RATE_REQUEST_MAGIC);
    request[8..12].copy_from_slice(&rate_bytes);
    request[12] = check_byte(rate_bytes);
    request
}

/// The rate a request's body asks for, or `None` when the check byte does not
/// match.
#[must_use]
pub fn decode_rate_request_body(body: [u8; RATE_REQUEST_BODY_LENGTH]) -> Option<u32> {
    let rate_bytes = [body[0], body[1], body[2], body[3]];
    (check_byte(rate_bytes) == body[4]).then_some(u32::from_le_bytes(rate_bytes))
}

/// The PL011's integer and fractional divisors (UARTIBRD, UARTFBRD) for
/// `baud_rate` with a UART clock of `clock` Hz: the clock over sixteen times the
/// rate, as a 16.6 fixed point number, which is `64 * clock / (16 * rate)`,
/// rounded to the nearest. `None` when the registers cannot hold it: the integer
/// part is 1 to 65535, and 65535 only with no fraction (PL011 TRM, DDI 0183G,
/// "Fractional Baud Rate Register, UARTFBRD"), so the fastest rate is the clock
/// over sixteen.
#[allow(
    clippy::cast_possible_truncation,
    reason = "the integer part is checked against the field's width, the fraction is masked to six bits"
)]
#[must_use]
pub const fn divisors(clock: u32, baud_rate: u32) -> Option<(u16, u8)> {
    if baud_rate == 0 {
        return None;
    }
    let value = (8 * clock as u64 / baud_rate as u64).div_ceil(2);
    let integer = value >> 6;
    let fraction = value & 0x3f;
    if integer == 0 || integer > u16::MAX as u64 || (integer == u16::MAX as u64 && fraction != 0) {
        return None;
    }
    Some((integer as u16, fraction as u8))
}

// The 4 MHz clock at 115200 baud: 2 + 11/64, a rate 0.08% slow. The 48 MHz
// default: 26 + 3/64, 0.015% slow.
const _: () = assert!(matches!(divisors(4_000_000, BOOT_BAUD_RATE), Some((2, 11))));
const _: () = assert!(matches!(
    divisors(48_000_000, BOOT_BAUD_RATE),
    Some((26, 3))
));
// 2 Mbaud at 48 MHz: exactly 1 + 32/64.
const _: () = assert!(matches!(divisors(48_000_000, 2_000_000), Some((1, 32))));

/// The rate `clock` Hz makes with these divisors: `4 * clock / (64 * integer +
/// fraction)`.
#[must_use]
pub fn actual_rate(clock: u32, integer: u16, fraction: u8) -> u32 {
    let sixty_fourths = 64 * u64::from(integer) + u64::from(fraction);
    if sixty_fourths == 0 {
        return 0;
    }
    u32::try_from(4 * u64::from(clock) / sixty_fourths).unwrap_or(u32::MAX)
}

/// Why chainboot does not switch to a rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateRefusal {
    /// The check byte did not match the rate.
    Garbled,
    /// The divisors for the rate do not fit the registers: too fast for the
    /// clock, too slow, or zero.
    OutOfRange { clock: u32, rate: u32 },
    /// The nearest rate the divisors make is too far off.
    TooFarOff {
        rate: u32,
        actual: u32,
        error_ppm: u64,
    },
}

impl fmt::Display for RateRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Garbled => write!(formatter, "the rate request's check byte does not match"),
            Self::OutOfRange { clock, rate } => write!(
                formatter,
                "{rate} baud is out of range for the {clock} Hz UART clock (at most {} baud)",
                clock / 16
            ),
            Self::TooFarOff {
                rate,
                actual,
                error_ppm,
            } => write!(
                formatter,
                "{rate} baud comes out at {actual} baud, {error_ppm} ppm off (at most {MAX_RATE_ERROR_PPM})"
            ),
        }
    }
}

/// The divisors for `rate` with a UART clock of `clock` Hz, when the registers
/// hold them and the rate they make is within [`MAX_RATE_ERROR_PPM`].
///
/// # Errors
///
/// [`RateRefusal::OutOfRange`] or [`RateRefusal::TooFarOff`].
pub fn validate_rate(clock: u32, rate: u32) -> Result<(u16, u8), RateRefusal> {
    let (integer, fraction) =
        divisors(clock, rate).ok_or(RateRefusal::OutOfRange { clock, rate })?;
    let actual = actual_rate(clock, integer, fraction);
    let error_ppm = u64::from(actual.abs_diff(rate)) * 1_000_000 / u64::from(rate);
    if error_ppm > MAX_RATE_ERROR_PPM {
        return Err(RateRefusal::TooFarOff {
            rate,
            actual,
            error_ppm,
        });
    }
    Ok((integer, fraction))
}

/// What the host made of chainboot's answer to a rate request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateAnswer {
    Accepted,
    Refused,
    /// Anything else: a chainboot that does not know rate requests says the
    /// "size" is too big.
    NotUnderstood,
}

/// What `answer`, the first characters chainboot sent after a rate request,
/// says. Characters after the answer's length do not matter.
#[must_use]
pub fn rate_answer(answer: &str) -> RateAnswer {
    if answer.starts_with(RATE_ACCEPTED) {
        RateAnswer::Accepted
    } else if answer.starts_with(RATE_REFUSED) {
        RateAnswer::Refused
    } else {
        RateAnswer::NotUnderstood
    }
}

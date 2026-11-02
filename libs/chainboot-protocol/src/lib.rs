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
//!
//! # The handoff
//!
//! At the jump chainboot leaves [`handoff_registers`] in `x1` and `x2`:
//! [`HANDOFF_MAGIC`] and the rate the UART is at. The firmware's stub and QEMU's
//! boot code enter a kernel with both zero (the arm64 boot protocol reserves them),
//! so a kernel that finds the magic ([`kept_rate`]) knows chainboot set the UART
//! up and negotiated its rate with the host: it keeps the UART as it is and skips
//! the console negotiation below. Kickstart reads them through `libboot`'s
//! `loader_registers`.
//!
//! # The console negotiation
//!
//! A kernel started without chainboot (from the SD card) sets the UART up at
//! [`BOOT_BAUD_RATE`] itself and offers the host a faster console:
//!
//! 1. It sends [`CONSOLE_BEACON`] every [`CONSOLE_WAIT`] beacon period, for
//!    [`CONSOLE_WAIT`]'s limit, 1.5 s, listening between beacons.
//! 2. A host that wants a faster console answers a beacon with the rate request
//!    above, all thirteen bytes in one go: the kernel takes a gap of more than
//!    [`CONSOLE_REQUEST_GAP`] between two of them, or eight bytes that are not
//!    [`RATE_REQUEST_MAGIC`], as noise, drops them and goes on beaconing.
//! 3. The kernel answers as chainboot does, at [`BOOT_BAUD_RATE`]:
//!    [`RATE_REFUSED`] and the reason on a line, after which both stay at
//!    [`BOOT_BAUD_RATE`] and the kernel boots on; or [`RATE_ACCEPTED`], after
//!    which it lets the answer leave the line, switches, waits [`SWITCH_SETTLE`]
//!    and sends [`CONSOLE_BEACON`] at the new rate as [`CONSOLE_CONFIRM_WAIT`]
//!    says.
//! 4. The host switches when it reads [`RATE_ACCEPTED`], and answers the first
//!    [`CONSOLE_BEACON`] it hears at the new rate with [`CONSOLE_ACK`]. Both stay
//!    at the new rate from then on.
//! 5. A kernel that hears no [`CONSOLE_ACK`] within [`CONSOLE_CONFIRM_WAIT`] goes
//!    back to [`BOOT_BAUD_RATE`] and boots on; the host gives up after
//!    [`HOST_CONSOLE_CONFIRM_TIMEOUT`], before the kernel does, and goes back too.
//!
//! When nobody answers, the kernel boots on at [`BOOT_BAUD_RATE`] after the 1.5 s
//! window. [`CONSOLE_BEACON`] holds no `^C`: a chainofcommand that does not know
//! it counts no [`BEACON`] in it, prints its three invisible `^B` and stays at its
//! rate, and so does a plain terminal. The rate request and its answers are the
//! ones chainboot uses, so the host reuses its code for both.

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

/// What chainboot leaves in `x1` at the jump to the kernel: `BAUDKEEP` in ASCII.
pub const HANDOFF_MAGIC: u64 = u64::from_le_bytes(*b"BAUDKEEP");

/// What chainboot leaves in `x1` and `x2` at the jump to the kernel, the UART at
/// `rate` baud.
#[must_use]
pub fn handoff_registers(rate: u32) -> [u64; 2] {
    [HANDOFF_MAGIC, u64::from(rate)]
}

/// The rate chainboot left the UART at, when `x1` and `x2` at the kernel's entry
/// are its handoff; `None` for anything else, such as the zeros the firmware
/// leaves.
#[must_use]
pub fn kept_rate(registers: [u64; 2]) -> Option<u32> {
    let [magic, rate] = registers;
    if magic != HANDOFF_MAGIC {
        return None;
    }
    u32::try_from(rate).ok().filter(|rate| *rate != 0)
}

/// What a kernel started without chainboot sends while it offers the host a
/// faster console: three `^B`.
pub const CONSOLE_BEACON: [u8; 3] = [2; 3];

/// What the host answers the first [`CONSOLE_BEACON`] it hears at the new rate
/// with: three `^F` (ASCII ACK).
pub const CONSOLE_ACK: [u8; 3] = [6; 3];

// An older host counts `^C` runs only: the console beacon must hold none.
const _: () = assert!(CONSOLE_BEACON[0] != BEACON[0] && CONSOLE_ACK[0] != BEACON[0]);

/// Offering the console at [`BOOT_BAUD_RATE`]: a beacon every quarter second, for
/// a second and a half.
pub const CONSOLE_WAIT: Wait = Wait {
    beacon_period: Duration::from_millis(250),
    give_up_after: Some(Duration::from_millis(1_500)),
};

/// Waiting for [`CONSOLE_ACK`] at a rate just accepted: a beacon every tenth of a
/// second, for a second, then back to [`BOOT_BAUD_RATE`].
pub const CONSOLE_CONFIRM_WAIT: Wait = Wait {
    beacon_period: Duration::from_millis(100),
    give_up_after: Some(Duration::from_secs(1)),
};

/// How long the host waits for [`CONSOLE_BEACON`] at the new rate before it goes
/// back to [`BOOT_BAUD_RATE`]. The kernel listens for [`CONSOLE_ACK`] until
/// [`SWITCH_SETTLE`] and [`CONSOLE_CONFIRM_WAIT`]'s limit after it switched; the
/// host's last [`CONSOLE_ACK`] goes out a beacon period before that.
pub const HOST_CONSOLE_CONFIRM_TIMEOUT: Duration = Duration::from_millis(800);

const _: () = assert!(
    HOST_CONSOLE_CONFIRM_TIMEOUT.as_millis() + CONSOLE_CONFIRM_WAIT.beacon_period.as_millis()
        <= SWITCH_SETTLE.as_millis()
            + match CONSOLE_CONFIRM_WAIT.give_up_after {
                Some(limit) => limit.as_millis(),
                None => u128::MAX,
            }
);

/// The longest gap the kernel takes between two bytes of a rate request during
/// the console negotiation. The host sends the thirteen bytes in one write, 1.1 ms
/// at [`BOOT_BAUD_RATE`]; a USB serial adapter may hold them back some 16 ms.
pub const CONSOLE_REQUEST_GAP: Duration = Duration::from_millis(50);

/// Counts a run of one byte, such as [`BEACON`], [`CONSOLE_BEACON`] or
/// [`CONSOLE_ACK`], in what arrives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run<const LENGTH: usize> {
    byte: u8,
    count: usize,
}

impl<const LENGTH: usize> Run<LENGTH> {
    /// Watch for `pattern`, `LENGTH` times the same byte.
    #[must_use]
    pub const fn of(pattern: [u8; LENGTH]) -> Self {
        Self {
            byte: pattern[0],
            count: 0,
        }
    }

    /// Whether `byte`, which arrived, ends a run. The count starts over then, and
    /// at any other byte.
    pub fn push(&mut self, byte: u8) -> bool {
        if byte != self.byte {
            self.count = 0;
            return false;
        }
        self.count += 1;
        if self.count == LENGTH {
            self.count = 0;
            return true;
        }
        false
    }
}

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

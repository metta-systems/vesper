//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! Microframe arithmetic for periodic split transactions: in which
//! microframe a start split may go, and in which microframes its complete
//! splits must follow. Nothing here touches hardware; the component reads
//! `HFNUM` and feeds the frame number in.
//!
//! The root port runs at high speed, so `HFNUM.FrNum` counts microframes
//! (125 µs each, USB 2.0 §8.4.3.1): bits 0 to 2 are the microframe within the
//! 1 ms frame, the bits above them the frame. The counter wraps at
//! [`FRAME_NUMBER_MASK`] (the `DWC_otg` databook's `HFNUM` description).
//!
//! A host channel can only be aimed at the next even or odd (micro)frame
//! (`HCCHARn.OddFrm`), so the component always aims at the microframe right
//! after the one it reads: the parity differs from the current microframe's,
//! and the channel cannot start in a microframe that is already running.
//!
//! USB 2.0 §11.18.4 (host split transaction scheduling) places the complete
//! splits of a periodic transaction: the start split goes in microframe
//! `Y-1`, the transaction translator runs the full- or low-speed transaction
//! from microframe `Y`, and the host issues a complete split in each of
//! microframes `Y+1`, `Y+2` and `Y+3`; a complete split answered NYET is
//! followed by the next in the next microframe. Counted from the start split's
//! microframe, the complete splits go 2, 3 and 4 microframes after it. A
//! complete split earlier than that finds nothing in the translator yet, and
//! one later than that finds the result discarded (§11.18.6): the report the
//! device sent is lost either way.

/// `HFNUM.FrNum` counts from 0 to this value and wraps.
pub const FRAME_NUMBER_MASK: u32 = 0x3fff;

/// Microframes in a 1 ms frame.
pub const MICROFRAMES_PER_FRAME: u32 = 8;

/// Distance, in microframes, from a periodic start split to its first
/// complete split (`Y+1` with the start split in `Y-1`).
pub const FIRST_COMPLETE_SPLIT: u32 = 2;

/// Distance, in microframes, from a periodic start split to its last complete
/// split (`Y+3`).
pub const LAST_COMPLETE_SPLIT: u32 = 4;

/// Last microframe of a frame (counted from 0) in which a periodic start
/// split is issued. With the start split in microframe 4 the complete splits
/// run in 6, 7 and the next frame's 0, so every transaction keeps its three
/// complete splits (§11.18.4 gives a transaction budgeted to start in `Y6`
/// only two) and none is budgeted across the end of the full-speed frame.
pub const LAST_START_SPLIT_MICROFRAME: u32 = 4;

/// The frame number `count` microframes after `frame`, wrapped as `HFNUM`
/// wraps.
#[must_use]
pub const fn advance(frame: u32, count: u32) -> u32 {
    frame.wrapping_add(count) & FRAME_NUMBER_MASK
}

/// Microframes from `earlier` to `later`, across the counter's wrap.
#[must_use]
pub const fn between(earlier: u32, later: u32) -> u32 {
    later.wrapping_sub(earlier) & FRAME_NUMBER_MASK
}

/// Whether `frame` is odd, as `HCCHARn.OddFrm` names it.
#[must_use]
pub const fn is_odd(frame: u32) -> bool {
    frame & 1 == 1
}

/// The microframe a periodic transaction is aimed at when `HFNUM` reads `now`:
/// the next one.
#[must_use]
pub const fn next(now: u32) -> u32 {
    advance(now, 1)
}

/// The microframe a periodic start split goes in when `HFNUM` reads `now`, or
/// `None` while the next microframe is too late in its frame
/// ([`LAST_START_SPLIT_MICROFRAME`]); the caller reads `HFNUM` again a
/// microframe later.
#[must_use]
pub const fn start_split_target(now: u32) -> Option<u32> {
    let target = next(now);
    if target % MICROFRAMES_PER_FRAME <= LAST_START_SPLIT_MICROFRAME {
        Some(target)
    } else {
        None
    }
}

/// The microframe a start split aimed at the parity of `aimed` ran in, read
/// back when its result is seen with `HFNUM` at `now`: the latest microframe
/// up to `now` with that parity, and not before `aimed`.
#[must_use]
pub const fn start_split_ran(aimed: u32, now: u32) -> u32 {
    let latest = if is_odd(now) == is_odd(aimed) {
        now
    } else {
        advance(now, FRAME_NUMBER_MASK)
    };
    // `now` one before `aimed` reads as almost a whole wrap: not yet run.
    if between(aimed, latest) > FRAME_NUMBER_MASK / 2 {
        aimed
    } else {
        latest
    }
}

/// Where the next complete split of a periodic transaction goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompleteSplitSlot {
    /// The next microframe is too early (fewer than [`FIRST_COMPLETE_SPLIT`]
    /// after the start split): wait for the microframe to pass and read
    /// `HFNUM` again.
    TooEarly,
    /// Aim the complete split at this microframe.
    Issue { target: u32 },
    /// The window has passed: the translator no longer holds the result
    /// (§11.18.6). Start the transaction over.
    Missed,
}

/// Where the next complete split of a transaction whose start split ran in
/// microframe `start` goes, with `HFNUM` reading `now`. Called after the start
/// split's ACK and after each NYET; the answer is always the next microframe,
/// as long as it lies in the window.
#[must_use]
pub const fn complete_split_slot(start: u32, now: u32) -> CompleteSplitSlot {
    let target = next(now);
    let distance = between(start, target);
    // A distance past half the counter is `now` reading before the start
    // split: the read raced the counter, so it is early too.
    if distance < FIRST_COMPLETE_SPLIT || distance > FRAME_NUMBER_MASK / 2 {
        CompleteSplitSlot::TooEarly
    } else if distance > LAST_COMPLETE_SPLIT {
        CompleteSplitSlot::Missed
    } else {
        CompleteSplitSlot::Issue { target }
    }
}

//! The periodic schedule of interrupt pipes: when each is polled next, which
//! DATA PID it expects, and whether the bus has room for it. Nothing here
//! touches hardware.
//!
//! The controller polls an interrupt-IN pipe with one IN transaction every
//! `interval_ms`. A poll is skipped while the pipe's client has posted no
//! empty buffer, so a client that stops reading never makes the controller
//! move data it has nowhere to put.

use {
    crate::pipes::MAX_PIPES,
    vesper_usb_protocol::{descriptor::TransferKind, enumeration::Speed},
};

/// Most interrupt pipes polled at once.
pub const MAX_POLLED: usize = MAX_PIPES;

/// Longest polling interval, in milliseconds.
pub const MAX_INTERVAL_MS: u32 = 255;

/// Microseconds of every millisecond frame the periodic schedule may use: 80%,
/// as USB 2.0 §5.6.4 leaves the rest to control and bulk transfers.
pub const PERIODIC_BUDGET_US: u32 = 800;

/// Bytes a transaction puts on the wire besides its data: sync, PID, token,
/// handshake, CRC and the inter-packet gaps, rounded up.
const OVERHEAD_BYTES: u32 = 13;

/// Microseconds one IN transaction of `max_packet` bytes takes on a bus
/// running at `speed`, rounded up. A low- or full-speed device behind a
/// high-speed hub takes its own bus's time.
#[must_use]
pub const fn transaction_us(speed: Speed, max_packet: u16) -> u32 {
    let bits = (OVERHEAD_BYTES + max_packet as u32) * 8;
    match speed {
        // 1.5 Mbit/s: 2 bits take 3 µs.
        Speed::Low => (bits * 2).div_ceil(3),
        Speed::Full => bits.div_ceil(12),
        // 480 Mbit/s: 480 bits take 1 µs.
        Speed::High => bits.div_ceil(480),
    }
}

/// What a poll came back with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Polled {
    /// The device sent a packet: the next poll expects the other DATA PID.
    Data,
    /// The device had nothing to send (NAK), or nothing was polled.
    Nothing,
}

/// Why a pipe cannot be scheduled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleError {
    /// The interval is zero or beyond [`MAX_INTERVAL_MS`], or the endpoint is
    /// not an interrupt-IN endpoint.
    Invalid,
    /// The periodic schedule has no room for the pipe's bandwidth.
    NoBandwidth,
    /// Every slot is taken.
    Full,
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    handle: u32,
    interval_ms: u32,
    /// Microseconds of every frame one poll takes.
    us: u32,
    /// Millisecond on the monotonic clock at which the pipe is polled next.
    due_ms: u64,
    /// Whether the next poll expects DATA1.
    data1: bool,
}

/// The interrupt pipes and when to poll them.
#[derive(Clone, Copy, Debug)]
pub struct Poller {
    entries: [Option<Entry>; MAX_POLLED],
}

impl Poller {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: [None; MAX_POLLED],
        }
    }

    /// Microseconds per frame the scheduled pipes use, each prorated over its
    /// interval and rounded up.
    #[must_use]
    pub fn used_us(&self) -> u32 {
        self.entries
            .iter()
            .flatten()
            .map(|entry| entry.us.div_ceil(entry.interval_ms))
            .sum()
    }

    /// Schedule the pipe `handle`, polled every `interval_ms` from `now_ms`,
    /// or replace its schedule. `transaction_us` is [`transaction_us`] of its
    /// endpoint.
    ///
    /// # Errors
    ///
    /// [`ScheduleError::Invalid`] for an interval of zero or beyond
    /// [`MAX_INTERVAL_MS`], [`ScheduleError::NoBandwidth`] when the schedule
    /// would pass [`PERIODIC_BUDGET_US`], [`ScheduleError::Full`] when every
    /// slot is taken. A failure leaves an existing schedule as it was.
    pub fn schedule(
        &mut self,
        handle: u32,
        interval_ms: u32,
        transaction_us: u32,
        now_ms: u64,
    ) -> Result<(), ScheduleError> {
        if interval_ms == 0 || interval_ms > MAX_INTERVAL_MS {
            return Err(ScheduleError::Invalid);
        }
        let existing = self
            .entries
            .iter()
            .position(|entry| entry.is_some_and(|entry| entry.handle == handle));
        let slot = match existing {
            Some(slot) => slot,
            None => self
                .entries
                .iter()
                .position(Option::is_none)
                .ok_or(ScheduleError::Full)?,
        };
        let others: u32 = self
            .entries
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != slot)
            .filter_map(|(_, entry)| entry.as_ref())
            .map(|entry| entry.us.div_ceil(entry.interval_ms))
            .sum();
        if others + transaction_us.div_ceil(interval_ms) > PERIODIC_BUDGET_US {
            return Err(ScheduleError::NoBandwidth);
        }
        self.entries[slot] = Some(Entry {
            handle,
            interval_ms,
            us: transaction_us,
            due_ms: now_ms,
            data1: false,
        });
        Ok(())
    }

    /// Stop polling the pipe `handle`.
    pub fn remove(&mut self, handle: u32) {
        for entry in &mut self.entries {
            if entry.is_some_and(|entry| entry.handle == handle) {
                *entry = None;
            }
        }
    }

    /// Whether the pipe `handle` is scheduled.
    #[must_use]
    pub fn contains(&self, handle: u32) -> bool {
        self.entries
            .iter()
            .flatten()
            .any(|entry| entry.handle == handle)
    }

    /// Reset the pipe's data toggle to DATA0, as after a halt is cleared.
    pub fn reset_toggle(&mut self, handle: u32) {
        for entry in self.entries.iter_mut().flatten() {
            if entry.handle == handle {
                entry.data1 = false;
            }
        }
    }

    /// The most overdue pipe due at `now_ms`, with the DATA PID it expects
    /// (`true`: DATA1). Call [`polled`](Self::polled) for it afterwards.
    #[must_use]
    pub fn due(&self, now_ms: u64) -> Option<(u32, bool)> {
        self.entries
            .iter()
            .flatten()
            .filter(|entry| entry.due_ms <= now_ms)
            .min_by_key(|entry| entry.due_ms)
            .map(|entry| (entry.handle, entry.data1))
    }

    /// Record that the pipe was polled (or skipped) at `now_ms`: it is due one
    /// interval after it was due, or after `now_ms` if it fell behind.
    pub fn polled(&mut self, handle: u32, now_ms: u64, result: Polled) {
        for entry in self.entries.iter_mut().flatten() {
            if entry.handle == handle {
                if result == Polled::Data {
                    entry.data1 = !entry.data1;
                }
                let next = entry.due_ms + u64::from(entry.interval_ms);
                entry.due_ms = if next > now_ms {
                    next
                } else {
                    now_ms + u64::from(entry.interval_ms)
                };
            }
        }
    }

    /// Milliseconds until the next pipe is due, zero if one is due now; `None`
    /// without any pipe.
    #[must_use]
    pub fn wait_ms(&self, now_ms: u64) -> Option<u64> {
        self.entries
            .iter()
            .flatten()
            .map(|entry| entry.due_ms.saturating_sub(now_ms))
            .min()
    }
}

impl Default for Poller {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether an endpoint of `kind` that moves data to the host (`input`) can be
/// scheduled for polling.
#[must_use]
pub const fn is_pollable(kind: TransferKind, input: bool) -> bool {
    matches!(kind, TransferKind::Interrupt) && input
}

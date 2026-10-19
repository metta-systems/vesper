//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! The sequence of a split transaction (USB 2.0 §8.4.2): how a full- or
//! low-speed packet crosses a high-speed hub. The host hands the packet to the
//! hub's transaction translator with a *start split*, and later collects the
//! result with a *complete split*; the hub answers NYET until it has the
//! result. Nothing here touches hardware: the component feeds in what each
//! channel run ended with and runs what the sequence asks for next.

use {
    crate::regs::{ChannelInterrupts, Pid},
    core::cmp::min,
};

/// Complete splits tried after one start split before the transaction is
/// started again: the hub may need several microframes for the device.
pub const MAX_NYET: u32 = 4;

/// Times a transaction is started again (the hub or device answered NAK, or a
/// transaction error) before it is given up.
pub const MAX_RESTARTS: u32 = 8;

/// The two halves of a split transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Hand the transaction (and an OUT packet's data) to the hub.
    Start,
    /// Collect the result (and an IN packet's data) from the hub.
    Complete,
}

/// What a channel run of one phase ended with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// The hub accepted a start split (ACK).
    Accepted,
    /// A complete split returned the transaction's result.
    Done,
    /// The hub has no result yet (NYET).
    NotYet,
    /// The hub or the device answered NAK.
    Nak,
    /// The device answered with a STALL.
    Stalled,
    /// A transaction error, a split that started too late, or a toggle error.
    Error,
    /// A bus error that cannot be recovered from.
    Failed,
}

/// What a channel's interrupts say about a run of `phase`, or `None` while
/// nothing decisive happened. A STALL outranks the other events, a bus error
/// outranks a result, and a result outranks a retry.
#[must_use]
pub fn event_of(phase: Phase, interrupts: ChannelInterrupts) -> Option<Event> {
    if interrupts.stall() {
        Some(Event::Stalled)
    } else if interrupts.ahb_error() || interrupts.babble() {
        Some(Event::Failed)
    } else if interrupts.transfer_complete() && phase == Phase::Complete {
        Some(Event::Done)
    } else if interrupts.nyet() {
        Some(Event::NotYet)
    } else if interrupts.nak() {
        Some(Event::Nak)
    } else if interrupts.transaction_error()
        || interrupts.frame_overrun()
        || interrupts.data_toggle_error()
    {
        Some(Event::Error)
    } else if interrupts.ack() && phase == Phase::Start {
        Some(Event::Accepted)
    } else {
        None
    }
}

/// What to do after an [`Event`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Run this phase next.
    Issue(Phase),
    /// The packet crossed the hub.
    Done,
    /// The device stalled.
    Stalled,
    /// The transaction cannot be completed.
    Failed,
}

/// One packet's split transaction, from its start split to its result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sequence {
    phase: Phase,
    not_yet: u32,
    restarts: u32,
}

impl Sequence {
    /// A transaction about to run its start split.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            phase: Phase::Start,
            not_yet: 0,
            restarts: 0,
        }
    }

    /// The phase to run now.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// Start the transaction again, unless it was started too often.
    fn restart(&mut self) -> Step {
        self.restarts += 1;
        if self.restarts > MAX_RESTARTS {
            return Step::Failed;
        }
        self.phase = Phase::Start;
        self.not_yet = 0;
        Step::Issue(Phase::Start)
    }

    /// The next step after the current phase ended with `event`.
    pub fn next(&mut self, event: Event) -> Step {
        match (self.phase, event) {
            (_, Event::Stalled) => Step::Stalled,
            (_, Event::Failed) => Step::Failed,
            (Phase::Start, Event::Accepted) => {
                self.phase = Phase::Complete;
                self.not_yet = 0;
                Step::Issue(Phase::Complete)
            }
            (Phase::Complete, Event::Done) => Step::Done,
            (Phase::Complete, Event::NotYet) => {
                self.not_yet += 1;
                if self.not_yet > MAX_NYET {
                    self.restart()
                } else {
                    Step::Issue(Phase::Complete)
                }
            }
            // A NAK or an error starts the packet over. Events a phase cannot
            // produce (a result of a start split, an ACK of a complete split,
            // NYET of a start split) are errors of the hub, handled the same.
            _ => self.restart(),
        }
    }
}

impl Default for Sequence {
    fn default() -> Self {
        Self::new()
    }
}

/// The bytes of the next packet of a transfer with `remaining` bytes left, at
/// `max_packet` bytes per packet. A transfer of no bytes still has one packet:
/// a zero-length packet.
#[must_use]
pub fn packet_bytes(remaining: usize, max_packet: u16) -> usize {
    min(remaining, usize::from(max_packet))
}

/// The PID of the packet after one with `pid`: DATA0 and DATA1 alternate. A
/// SETUP packet is followed by the data stage, which starts with DATA1.
#[must_use]
pub const fn next_pid(pid: Pid) -> Pid {
    match pid {
        Pid::Data0 | Pid::Setup => Pid::Data1,
        Pid::Data1 | Pid::Data2 => Pid::Data0,
    }
}

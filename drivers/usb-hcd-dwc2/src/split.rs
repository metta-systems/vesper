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

/// Complete splits of a poll answered NYET before the poll is started again,
/// when the complete splits are not placed by microframe (the controller's
/// interrupt Thread places them, see `microframe`, and ends the window with
/// [`Event::Missed`] instead). A non-periodic transaction is never started
/// again after NYET.
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
    /// A transaction error, a split that started too late, a toggle error, or
    /// a halt without a reason the phase can give.
    Error,
    /// A bus error that cannot be recovered from.
    Failed,
    /// The complete splits of a periodic transaction ran out of their window
    /// (USB 2.0 §11.18.4: microframes `Y+1` to `Y+3`) without the result; the
    /// translator has dropped it (§11.18.6).
    Missed,
}

/// What a channel's interrupts say about a run of `phase`, or `None` while
/// the channel has not halted. In DMA mode the controller halts the channel
/// after every start and complete split and `HCINTn` holds the reason (see
/// `regs::halt_event_mask`); only an AHB error is decisive before the halt.
/// A STALL outranks the other events, a bus error outranks a result, and a
/// result outranks a retry.
#[must_use]
pub fn event_of(phase: Phase, interrupts: ChannelInterrupts) -> Option<Event> {
    if interrupts.ahb_error() {
        Some(Event::Failed)
    } else if !interrupts.halted() {
        None
    } else if interrupts.stall() {
        Some(Event::Stalled)
    } else if interrupts.babble() {
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
        Some(Event::Error)
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
    /// The device had nothing to send: the answer of a poll of an interrupt
    /// endpoint that answered with a NAK.
    NoData,
}

/// One packet's split transaction, from its start split to its result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sequence {
    phase: Phase,
    not_yet: u32,
    restarts: u32,
    /// Whether this is a poll of an interrupt-IN endpoint, where a NAK is an
    /// answer.
    poll: bool,
}

impl Sequence {
    /// A transaction about to run its start split.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            phase: Phase::Start,
            not_yet: 0,
            restarts: 0,
            poll: false,
        }
    }

    /// A poll of an interrupt-IN endpoint about to run its start split: a NAK
    /// ends it with [`Step::NoData`] instead of starting it over.
    #[must_use]
    pub const fn poll() -> Self {
        Self {
            poll: true,
            ..Self::new()
        }
    }

    /// How often the transaction was started over so far.
    #[must_use]
    pub const fn restarts(&self) -> u32 {
        self.restarts
    }

    /// Complete splits answered NYET since the last start split.
    #[must_use]
    pub const fn not_yet(&self) -> u32 {
        self.not_yet
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
            (_, Event::Nak) if self.poll => Step::NoData,
            // A NAK is the device or hub not ready yet: the start split is
            // issued again and the error count is left alone (USB 2.0 §11.17.1).
            // The caller bounds how long it keeps trying.
            (_, Event::Nak) => {
                self.phase = Phase::Start;
                self.not_yet = 0;
                Step::Issue(Phase::Start)
            }
            // A non-periodic transaction stays in the translator's buffer until
            // a complete split collects it (USB 2.0 §11.17.1): NYET only says it
            // has not finished on the full- or low-speed bus yet. Starting it
            // over would hand the translator a second copy while it holds the
            // first. The caller bounds how long it keeps asking.
            (Phase::Complete, Event::NotYet) if !self.poll => {
                self.not_yet += 1;
                Step::Issue(Phase::Complete)
            }
            (Phase::Complete, Event::NotYet) => {
                self.not_yet += 1;
                if self.not_yet > MAX_NYET {
                    self.restart()
                } else {
                    Step::Issue(Phase::Complete)
                }
            }
            // An error starts the packet over. Events a phase cannot
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

/// Bit of a `transfer_failed` status that says the failed transfer ran split
/// transactions; the other bits are the channel's events of the last run.
pub const STATUS_SPLIT: u32 = 1 << 31;
/// Bit of a `transfer_failed` status that says the failed split transaction
/// was in its complete split, not its start split. Set with [`STATUS_SPLIT`].
pub const STATUS_COMPLETE: u32 = 1 << 30;

/// The status a failed split transaction reports: the channel `events` of its
/// last run, marked as split and with the `phase` it failed in.
#[must_use]
pub const fn failure_status(events: u32, phase: Phase) -> u32 {
    let complete = match phase {
        Phase::Start => 0,
        Phase::Complete => STATUS_COMPLETE,
    };
    events | STATUS_SPLIT | complete
}

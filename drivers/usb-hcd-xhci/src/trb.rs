//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! Transfer Request Blocks (§4.11, §6.4): the 16-byte records on every ring.
//! A TRB is a 64-bit parameter, a status word and a control word; the control
//! word holds the cycle bit (bit 0) and the TRB type (bits 15:10).

/// One TRB, as the controller reads and writes it in memory (little endian).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Trb {
    pub parameter: u64,
    pub status: u32,
    pub control: u32,
}

/// TRB type codes (§6.4.6, Table 6-91).
pub mod kind {
    /// Link: the next TRB is at the parameter's address (§6.4.4.1).
    pub const LINK: u8 = 6;
    /// No Op command: completes without doing anything (§6.4.3.1).
    pub const NO_OP_COMMAND: u8 = 23;
    /// Transfer Event (§6.4.2.1).
    pub const TRANSFER_EVENT: u8 = 32;
    /// Command Completion Event: the parameter is the command TRB's address
    /// (§6.4.2.2).
    pub const COMMAND_COMPLETION: u8 = 33;
    /// Port Status Change Event: the parameter's bits 31:24 are the port ID
    /// (§6.4.2.3).
    pub const PORT_STATUS_CHANGE: u8 = 34;
    /// Host Controller Event (§6.4.2.6).
    pub const HOST_CONTROLLER: u8 = 37;
}

/// Completion code of an event that reports success (§6.4.5, Table 6-90).
pub const SUCCESS: u8 = 1;

/// Bytes of one TRB.
pub const TRB_BYTES: u64 = 16;

/// The cycle bit of the control word.
const CYCLE: u32 = 1 << 0;
/// A Link TRB's Toggle Cycle bit: following the link flips the cycle state.
const TOGGLE_CYCLE: u32 = 1 << 1;
/// Where the TRB type sits in the control word.
const KIND_SHIFT: u32 = 10;
const KIND_MASK: u32 = 0x3f;

impl Trb {
    /// A No Op command (§6.4.3.1).
    #[must_use]
    pub const fn no_op_command() -> Self {
        Self {
            parameter: 0,
            status: 0,
            control: (kind::NO_OP_COMMAND as u32) << KIND_SHIFT,
        }
    }

    /// A Link TRB to the ring segment at bus address `target`, which toggles
    /// the cycle state (§6.4.4.1).
    #[must_use]
    pub const fn link(target: u64) -> Self {
        Self {
            parameter: target,
            status: 0,
            control: (kind::LINK as u32) << KIND_SHIFT | TOGGLE_CYCLE,
        }
    }

    /// The TRB with its cycle bit set to `cycle`.
    #[must_use]
    pub const fn with_cycle(self, cycle: bool) -> Self {
        Self {
            control: if cycle {
                self.control | CYCLE
            } else {
                self.control & !CYCLE
            },
            ..self
        }
    }

    /// The TRB type.
    #[must_use]
    pub const fn kind(&self) -> u8 {
        ((self.control >> KIND_SHIFT) & KIND_MASK) as u8
    }

    /// The cycle bit.
    #[must_use]
    pub const fn cycle(&self) -> bool {
        self.control & CYCLE != 0
    }

    /// Whether a Link TRB toggles the cycle state.
    #[must_use]
    pub const fn toggles_cycle(&self) -> bool {
        self.control & TOGGLE_CYCLE != 0
    }

    /// An event's completion code (status bits 31:24).
    #[must_use]
    pub const fn completion_code(&self) -> u8 {
        (self.status >> 24) as u8
    }

    /// A Command Completion Event's slot ID (control bits 31:24).
    #[must_use]
    pub const fn slot(&self) -> u8 {
        (self.control >> 24) as u8
    }

    /// A Port Status Change Event's port ID (parameter bits 31:24).
    #[must_use]
    pub const fn port(&self) -> u8 {
        self.parameter.to_le_bytes()[3]
    }

    /// The four little-endian words the TRB occupies, the control word last.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the parameter's low word is the truncation"
    )]
    pub const fn words(&self) -> [u32; 4] {
        [
            self.parameter as u32,
            (self.parameter >> 32) as u32,
            self.status,
            self.control,
        ]
    }

    /// The TRB stored as `words`.
    #[must_use]
    pub const fn from_words(words: [u32; 4]) -> Self {
        Self {
            parameter: words[0] as u64 | (words[1] as u64) << 32,
            status: words[2],
            control: words[3],
        }
    }
}

/// An Event Ring Segment Table entry (§6.5): the segment's bus address and
/// its size in TRBs, as four little-endian words.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    reason = "the base's low word is the truncation"
)]
pub const fn segment_table_entry(base: u64, trbs: u16) -> [u32; 4] {
    [base as u32, (base >> 32) as u32, trbs as u32, 0]
}

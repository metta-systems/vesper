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
    /// Setup Stage of a control transfer (§6.4.1.2.1).
    pub const SETUP_STAGE: u8 = 2;
    /// Data Stage of a control transfer (§6.4.1.2.2).
    pub const DATA_STAGE: u8 = 3;
    /// Status Stage of a control transfer (§6.4.1.2.3).
    pub const STATUS_STAGE: u8 = 4;
    /// Link: the next TRB is at the parameter's address (§6.4.4.1).
    pub const LINK: u8 = 6;
    /// Enable Slot command: its completion names a free device slot (§6.4.3.2).
    pub const ENABLE_SLOT: u8 = 9;
    /// Disable Slot command (§6.4.3.3).
    pub const DISABLE_SLOT: u8 = 10;
    /// Address Device command (§6.4.3.4).
    pub const ADDRESS_DEVICE: u8 = 11;
    /// Configure Endpoint command (§6.4.3.5).
    pub const CONFIGURE_ENDPOINT: u8 = 12;
    /// Evaluate Context command (§6.4.3.6).
    pub const EVALUATE_CONTEXT: u8 = 13;
    /// Reset Endpoint command: takes a halted endpoint to Stopped (§6.4.3.7).
    pub const RESET_ENDPOINT: u8 = 14;
    /// Stop Endpoint command: stops a running endpoint (§6.4.3.8).
    pub const STOP_ENDPOINT: u8 = 15;
    /// Set TR Dequeue Pointer command (§6.4.3.9).
    pub const SET_TR_DEQUEUE_POINTER: u8 = 16;
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
/// Completion codes of the failures a transfer or command reports (§6.4.5).
pub mod code {
    /// The device babbled: it sent more than the packet size.
    pub const BABBLE: u8 = 3;
    /// A transaction failed on the bus (no answer, CRC, timeout).
    pub const USB_TRANSACTION_ERROR: u8 = 4;
    /// A TRB the controller could not take.
    pub const TRB_ERROR: u8 = 5;
    /// The endpoint answered with a STALL; it is now halted.
    pub const STALL: u8 = 6;
    /// No device slot is free (Enable Slot).
    pub const NO_SLOTS_AVAILABLE: u8 = 9;
    /// A transfer ended short of its length, which `ISP` asked to hear of.
    pub const SHORT_PACKET: u8 = 13;
    /// A context parameter is not valid.
    pub const PARAMETER_ERROR: u8 = 17;
    /// The command does not fit the slot's or endpoint's state.
    pub const CONTEXT_STATE_ERROR: u8 = 19;
}

/// Bytes of one TRB.
pub const TRB_BYTES: u64 = 16;

/// The cycle bit of the control word.
const CYCLE: u32 = 1 << 0;
/// A Link TRB's Toggle Cycle bit: following the link flips the cycle state.
const TOGGLE_CYCLE: u32 = 1 << 1;
/// Interrupt on Short Packet: a transfer TRB that ends short raises an event.
const INTERRUPT_ON_SHORT: u32 = 1 << 2;
/// Interrupt On Completion: the TRB raises an event when it completes.
const INTERRUPT_ON_COMPLETION: u32 = 1 << 5;
/// Immediate Data: the parameter holds the data itself (a Setup Stage's
/// setup packet).
const IMMEDIATE_DATA: u32 = 1 << 6;
/// Where the TRB type sits in the control word.
const KIND_SHIFT: u32 = 10;
const KIND_MASK: u32 = 0x3f;
/// A Data or Status Stage's Direction bit: 1 is IN.
const DIRECTION_IN: u32 = 1 << 16;
/// Where a Setup Stage's Transfer Type sits in the control word.
const TRANSFER_TYPE_SHIFT: u32 = 16;
/// Where an endpoint's Device Context Index sits in the control word.
const ENDPOINT_SHIFT: u32 = 16;
const ENDPOINT_MASK: u32 = 0x1f;
/// Where the slot ID sits in the control word.
const SLOT_SHIFT: u32 = 24;
/// A transfer TRB's TRB Transfer Length, status bits 16:0.
const TRANSFER_LENGTH_MASK: u32 = 0x1_ffff;
/// A Transfer Event's residual length, status bits 23:0.
const RESIDUAL_MASK: u32 = 0xff_ffff;
/// The Dequeue Cycle State bit of a Set TR Dequeue Pointer's parameter.
const DEQUEUE_CYCLE_STATE: u64 = 1 << 0;

/// The Transfer Type of a Setup Stage TRB (§6.4.1.2.1): whether a data stage
/// follows and its direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferType {
    NoData = 0,
    Out = 2,
    In = 3,
}

/// The Device Context Index of endpoint 0, the default control endpoint: what
/// its doorbell and its commands name.
pub const ENDPOINT0: u8 = 1;

impl Trb {
    const fn of_kind(kind: u8) -> Self {
        Self {
            parameter: 0,
            status: 0,
            control: (kind as u32) << KIND_SHIFT,
        }
    }

    const fn with_slot(self, slot: u8) -> Self {
        Self {
            control: self.control | (slot as u32) << SLOT_SHIFT,
            ..self
        }
    }

    const fn with_endpoint(self, endpoint: u8) -> Self {
        Self {
            control: self.control | ((endpoint as u32) & ENDPOINT_MASK) << ENDPOINT_SHIFT,
            ..self
        }
    }

    /// An Enable Slot command (§6.4.3.2).
    #[must_use]
    pub const fn enable_slot() -> Self {
        Self::of_kind(kind::ENABLE_SLOT)
    }

    /// A Disable Slot command for `slot` (§6.4.3.3).
    #[must_use]
    pub const fn disable_slot(slot: u8) -> Self {
        Self::of_kind(kind::DISABLE_SLOT).with_slot(slot)
    }

    /// An Address Device command for `slot` with the input context at bus
    /// address `input` (§6.4.3.4). The controller sends `SET_ADDRESS` itself.
    #[must_use]
    pub const fn address_device(input: u64, slot: u8) -> Self {
        Self {
            parameter: input,
            ..Self::of_kind(kind::ADDRESS_DEVICE).with_slot(slot)
        }
    }

    /// A Configure Endpoint command for `slot` (§6.4.3.5).
    #[must_use]
    pub const fn configure_endpoint(input: u64, slot: u8) -> Self {
        Self {
            parameter: input,
            ..Self::of_kind(kind::CONFIGURE_ENDPOINT).with_slot(slot)
        }
    }

    /// An Evaluate Context command for `slot` (§6.4.3.6).
    #[must_use]
    pub const fn evaluate_context(input: u64, slot: u8) -> Self {
        Self {
            parameter: input,
            ..Self::of_kind(kind::EVALUATE_CONTEXT).with_slot(slot)
        }
    }

    /// A Reset Endpoint command for endpoint `endpoint` (a Device Context
    /// Index) of `slot` (§6.4.3.7).
    #[must_use]
    pub const fn reset_endpoint(slot: u8, endpoint: u8) -> Self {
        Self::of_kind(kind::RESET_ENDPOINT)
            .with_slot(slot)
            .with_endpoint(endpoint)
    }

    /// A Stop Endpoint command for endpoint `endpoint` of `slot` (§6.4.3.8).
    #[must_use]
    pub const fn stop_endpoint(slot: u8, endpoint: u8) -> Self {
        Self::of_kind(kind::STOP_ENDPOINT)
            .with_slot(slot)
            .with_endpoint(endpoint)
    }

    /// A Set TR Dequeue Pointer command (§6.4.3.9): the transfer ring of
    /// `endpoint` of `slot` continues at bus address `dequeue` with cycle
    /// state `cycle`.
    #[must_use]
    pub const fn set_dequeue(dequeue: u64, cycle: bool, slot: u8, endpoint: u8) -> Self {
        Self {
            parameter: dequeue | if cycle { DEQUEUE_CYCLE_STATE } else { 0 },
            ..Self::of_kind(kind::SET_TR_DEQUEUE_POINTER)
                .with_slot(slot)
                .with_endpoint(endpoint)
        }
    }

    /// The Setup Stage of a control transfer, its 8-byte setup packet carried
    /// in the TRB itself (§6.4.1.2.1).
    #[must_use]
    pub const fn setup_stage(setup: [u8; 8], transfer: TransferType) -> Self {
        Self {
            parameter: u64::from_le_bytes(setup),
            status: 8,
            control: (kind::SETUP_STAGE as u32) << KIND_SHIFT
                | IMMEDIATE_DATA
                | (transfer as u32) << TRANSFER_TYPE_SHIFT,
        }
    }

    /// The Data Stage of a control transfer: `length` bytes at bus address
    /// `buffer`, IN when `input` (§6.4.1.2.2). A short IN stage raises an event,
    /// whose residual length tells how much arrived.
    #[must_use]
    pub const fn data_stage(buffer: u64, length: u32, input: bool) -> Self {
        Self {
            parameter: buffer,
            status: length & TRANSFER_LENGTH_MASK,
            control: (kind::DATA_STAGE as u32) << KIND_SHIFT
                | INTERRUPT_ON_SHORT
                | if input { DIRECTION_IN } else { 0 },
        }
    }

    /// The Status Stage of a control transfer, IN when `input` (§6.4.1.2.3);
    /// its completion raises the transfer's event.
    #[must_use]
    pub const fn status_stage(input: bool) -> Self {
        Self {
            parameter: 0,
            status: 0,
            control: (kind::STATUS_STAGE as u32) << KIND_SHIFT
                | INTERRUPT_ON_COMPLETION
                | if input { DIRECTION_IN } else { 0 },
        }
    }

    /// A Transfer Event's residual: the bytes of the TRB it names that did
    /// not move (status bits 23:0).
    #[must_use]
    pub const fn residual(&self) -> u32 {
        self.status & RESIDUAL_MASK
    }

    /// A Transfer Event's endpoint, its Device Context Index (control bits
    /// 20:16).
    #[must_use]
    pub const fn endpoint(&self) -> u8 {
        ((self.control >> ENDPOINT_SHIFT) & ENDPOINT_MASK) as u8
    }
    /// A No Op command (§6.4.3.1).
    #[must_use]
    pub const fn no_op_command() -> Self {
        Self::of_kind(kind::NO_OP_COMMAND)
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

    /// A Command Completion or Transfer Event's slot ID (control bits 31:24).
    #[must_use]
    pub const fn slot(&self) -> u8 {
        (self.control >> SLOT_SHIFT) as u8
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

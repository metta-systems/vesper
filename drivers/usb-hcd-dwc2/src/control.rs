//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! The stages of a control transfer on endpoint 0 and how many packets each
//! takes. The component runs the stages in order: SETUP, then the data stage
//! if the request has one, then the status stage, which runs the other way.

use vesper_usb_protocol::setup::{DEVICE_TO_HOST, SetupPacket};

/// The stages of one control transfer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// Whether a data stage follows the setup stage.
    pub has_data: bool,
    /// Whether the data stage moves bytes to the host (IN).
    pub data_input: bool,
    /// Whether the status stage moves nothing to the host (OUT) or back (IN).
    pub status_input: bool,
}

/// The stages `setup` runs. The data stage goes in the direction the request
/// type says. The status stage goes the other way, and an IN status follows a
/// request without data.
#[must_use]
pub const fn plan(setup: SetupPacket) -> Plan {
    let data_input = setup.request_type & DEVICE_TO_HOST != 0;
    let has_data = setup.length > 0;
    let status_input = if has_data { !data_input } else { true };
    Plan {
        has_data,
        data_input,
        status_input,
    }
}

/// The number of packets `bytes` take at `max_packet` bytes each. A transfer
/// of no bytes still takes one packet: a zero-length packet.
#[must_use]
pub const fn packet_count(bytes: u32, max_packet: u32) -> u32 {
    if bytes == 0 || max_packet == 0 {
        1
    } else {
        bytes.div_ceil(max_packet)
    }
}

/// Bytes the setup packet takes at the start of a control stage buffer.
pub const SETUP_BYTES: usize = 8;

/// Why a control stage buffer cannot run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageError {
    /// The buffer is too short for the setup packet, or for the data stage the
    /// setup packet announces.
    TooShort,
    /// The data stage does not fit the controller's transfer page.
    TooLong,
}

/// The data stage length of a control transfer staged in a buffer of
/// `stage_len` bytes (the setup packet, then the data stage) whose setup
/// packet announces `setup.length` bytes. The controller moves at most
/// `capacity` bytes of data.
///
/// # Errors
///
/// [`StageError::TooShort`] if the buffer cannot hold the setup packet and the
/// announced data, [`StageError::TooLong`] if `capacity` cannot.
pub const fn data_length(
    stage_len: usize,
    setup: SetupPacket,
    capacity: usize,
) -> Result<usize, StageError> {
    let length = setup.length as usize;
    if stage_len < SETUP_BYTES || stage_len - SETUP_BYTES < length {
        Err(StageError::TooShort)
    } else if length > capacity {
        Err(StageError::TooLong)
    } else {
        Ok(length)
    }
}

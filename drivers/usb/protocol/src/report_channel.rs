//! The io-channel interrupt-IN reports travel on: the control page and data
//! page the host controller shares with each of its clients
//! (`usb_hcd_v1`'s `channel 1`).
//!
//! It runs in receive mode (see `libio-channel`): the client submits empty
//! buffers of the data page, and the controller returns each one holding the
//! report of one poll, its record's `len` cut to the report's bytes and its
//! value word set to the handle of the pipe the report came from. The
//! controller polls a pipe only while a buffer waits, so a client that stops
//! submitting stops the polls and no report is dropped.

use libio_channel::{Channel, DataArea};

/// Slots of the channel's FIFOs.
pub const SLOTS: usize = 16;

/// The channel in its control page.
#[repr(C)]
pub struct ReportChannel {
    /// Empty buffers from the client; reports back from the controller.
    pub reports: Channel<SLOTS>,
}

const _: () = assert!(size_of::<ReportChannel>() <= 4096);

impl ReportChannel {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            reports: Channel::new(),
        }
    }

    /// The channel in the control page at `address`.
    ///
    /// # Safety
    ///
    /// `address` is the start of a page mapped read/write for as long as the
    /// channel is used, which holds a channel or zeroes.
    #[must_use]
    pub unsafe fn at<'page>(address: u64) -> &'page Self {
        // SAFETY: forwarded caller contract; every field is atomic, so the
        // peer's concurrent access is sound.
        unsafe { &*(address as *const Self) }
    }
}

impl Default for ReportChannel {
    fn default() -> Self {
        Self::new()
    }
}

/// The data area behind a control page at `control`: the one page that follows
/// it.
#[must_use]
pub const fn data_area(control: u64) -> DataArea {
    DataArea {
        base: control + 4096,
        len: 4096,
    }
}

//! The io-channel input events travel on: the control page and data page the
//! component shares with each client of `input_v1` (`channel 1;`).
//!
//! It runs in receive mode (see `libio-channel`): the client posts empty
//! buffers of the data page, and the component returns each holding the
//! records of one frame, its length cut to them. The records are 24 bytes each
//! (`vesper_input_event::record`).

use libio_channel::{Channel, DataArea};

/// Slots of the channel's FIFOs.
pub const SLOTS: usize = 16;

/// The channel in its control page.
#[repr(C)]
pub struct EventChannel {
    /// Empty buffers from the client; frames back from the component.
    pub events: Channel<SLOTS>,
}

const _: () = assert!(size_of::<EventChannel>() <= 4096);

impl EventChannel {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            events: Channel::new(),
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

impl Default for EventChannel {
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

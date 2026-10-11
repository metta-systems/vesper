#![no_std]

//! What a serial driver and its client agree on: two IO channels
//! (`libio-channel`) in one shared control page, a [`SerialChannel`], with
//! the bytes themselves in a data area of [`DATA_PAGES`] pages mapped at
//! [`DATA_AREA_VA`] on both sides.
//!
//! - `transmit` runs in transmit mode: the client submits packets of bytes
//!   to send and gets each back once the driver handed all of it to the
//!   device.
//! - `receive` runs in receive mode: the client submits empty buffers and
//!   gets them back holding received bytes, each record's `len` cut to the
//!   bytes stored there. The driver returns a buffer as soon as the device
//!   has nothing more to give, so a packet may come back part full. Bytes
//!   that arrive while no buffer is submitted are dropped and counted in
//!   [`SerialChannel::dropped`].
//!
//! The client attaches before the driver's Threads start, by installing in
//! the driver's table:
//!
//! - at [`CONTROL_FRAME_SLOT`], a read/write Frame holding the control page;
//! - from [`DATA_FRAME_SLOT`], [`DATA_PAGES`] read/write Frames holding the
//!   data area, in address order;
//! - at [`TRANSMIT_SUBMITTED_SLOT`], an `EventCount` key (`RECV`) the client
//!   advances after submitting transmit packets;
//! - at [`TRANSMIT_COMPLETED_SLOT`] and [`RECEIVE_COMPLETED_SLOT`],
//!   `EventCount` keys (`SEND`) the driver advances after completing
//!   packets of either channel.
//!
//! Nobody sleeps on `receive.submit`: the driver looks for buffers when the
//! device has received bytes, so submitting empties needs no signal.
//!
//! All Frames and `EventCount`s start out fresh: an all-zero page is an idle
//! channel, and every count starts at zero. A driver whose
//! [`CONTROL_FRAME_SLOT`] is empty has no client and only keeps its device
//! quiet.

use {
    core::sync::atomic::{AtomicU64, Ordering},
    libio_channel::{Channel, DataArea},
};

/// Slot of the driver's table holding the control page's Frame.
pub const CONTROL_FRAME_SLOT: u32 = 248;
/// Slot of the driver's table holding the data area's first Frame.
pub const DATA_FRAME_SLOT: u32 = 240;
/// Slot of the driver's table holding the transmit channel's submit
/// `EventCount`.
pub const TRANSMIT_SUBMITTED_SLOT: u32 = 249;
/// Slot of the driver's table holding the transmit channel's completion
/// `EventCount`.
pub const TRANSMIT_COMPLETED_SLOT: u32 = 250;
/// Slot of the driver's table holding the receive channel's completion
/// `EventCount`.
pub const RECEIVE_COMPLETED_SLOT: u32 = 251;

/// Pages of the data area.
pub const DATA_PAGES: u32 = 4;

/// Where the driver maps the control page: in its device window, above
/// every register page (`libdevice-driver`).
pub const CONTROL_VA: u64 = 0x3801_0000;

/// Where both sides map the data area: the address every [`IoRec`] names.
///
/// [`IoRec`]: libio_channel::IoRec
pub const DATA_AREA_VA: u64 = CONTROL_VA + 4096;

/// The data area.
pub const DATA_AREA: DataArea = DataArea {
    base: DATA_AREA_VA,
    len: DATA_PAGES as u64 * 4096,
};

/// [`SerialChannel::config`]: the device echoes every transmitted byte back
/// to its own receiver instead of sending it out.
pub const LOOPBACK: u64 = 1 << 0;

/// Slots of each FIFO.
pub const SLOTS: usize = 32;

/// The shared control page.
#[repr(C)]
pub struct SerialChannel {
    /// Device options, written by the client before the driver starts
    /// ([`LOOPBACK`]).
    pub config: AtomicU64,
    /// Received bytes the driver dropped for want of a receive buffer.
    pub dropped: AtomicU64,
    /// Bytes from the client to the device.
    pub transmit: Channel<SLOTS>,
    /// Bytes from the device to the client.
    pub receive: Channel<SLOTS>,
}

const _: () = assert!(size_of::<SerialChannel>() <= 4096);

impl SerialChannel {
    pub const fn new() -> Self {
        Self {
            config: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            transmit: Channel::new(),
            receive: Channel::new(),
        }
    }

    /// The channel in the page at `address`.
    ///
    /// # Safety
    ///
    /// `address` is the start of a page mapped read/write for as long as the
    /// channel is used, which holds a channel or zeroes.
    pub unsafe fn at(address: u64) -> &'static Self {
        // SAFETY: forwarded caller contract; every field is atomic, so the
        // peer's concurrent access is sound.
        unsafe { &*(address as *const Self) }
    }

    /// Whether the client asked for `option`.
    pub fn has(&self, option: u64) -> bool {
        self.config.load(Ordering::Acquire) & option != 0
    }
}

impl Default for SerialChannel {
    fn default() -> Self {
        Self::new()
    }
}

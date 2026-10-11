#![no_std]

//! What a serial driver and its client agree on.
//!
//! A client talks to a serial driver through one shared page, a
//! [`SerialChannel`]: a [`Ring`] of bytes to transmit, which the client fills
//! and the driver drains into the device, and a ring of received bytes, which
//! the driver fills from the device and the client drains. An all-zero page
//! is an empty channel, so a freshly retyped Frame needs no initialization.
//!
//! The client attaches before the driver's Thread starts, by installing in
//! the driver's table:
//!
//! - at [`CHANNEL_FRAME_SLOT`], a read/write Frame holding the page; the
//!   driver maps it itself;
//! - at [`CLIENT_SLOT`], a Notification key (`SEND`) the driver signals
//!   whenever it moved bytes in either ring.
//!
//! The client rings the driver's own Notification with a key badged
//! [`DOORBELL_BIT`] after it queued bytes to transmit. A driver whose
//! [`CHANNEL_FRAME_SLOT`] is empty has no client and only keeps its device
//! quiet.

use core::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering};

/// Slot of the driver's table holding the channel page's Frame.
pub const CHANNEL_FRAME_SLOT: u32 = 248;
/// Slot of the driver's table holding the client's Notification key.
pub const CLIENT_SLOT: u32 = 249;
/// The bit a client's doorbell key signals on the driver's Notification. The
/// driver's interrupts signal the bits from 0 up, so a serial device node
/// has fewer than 15 interrupts.
pub const DOORBELL_BIT: u16 = 1 << 15;

/// [`SerialChannel::config`]: the device echoes every transmitted byte back
/// to its own receiver instead of sending it out.
pub const LOOPBACK: u64 = 1 << 0;

/// Capacity of each ring, in bytes.
pub const RING_BYTES: usize = 1024;

/// A single-producer, single-consumer byte ring in shared memory.
///
/// `written` and `read` count bytes ever written and read, wrapping; their
/// difference is the fill level. Each side only advances its own counter, so
/// a peer that corrupts the other's counter can only make the ring look full
/// or empty, never index outside it.
#[repr(C)]
pub struct Ring {
    written: AtomicU32,
    read: AtomicU32,
    data: [AtomicU8; RING_BYTES],
}

impl Ring {
    pub const fn new() -> Self {
        Self {
            written: AtomicU32::new(0),
            read: AtomicU32::new(0),
            data: [const { AtomicU8::new(0) }; RING_BYTES],
        }
    }

    /// Bytes waiting to be read.
    pub fn len(&self) -> usize {
        let written = self.written.load(Ordering::Acquire);
        let read = self.read.load(Ordering::Acquire);
        written.wrapping_sub(read) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Append `byte`, unless the ring is full. Producer side only.
    pub fn push(&self, byte: u8) -> bool {
        let written = self.written.load(Ordering::Relaxed);
        let read = self.read.load(Ordering::Acquire);
        if written.wrapping_sub(read) as usize >= RING_BYTES {
            return false;
        }
        self.data[written as usize % RING_BYTES].store(byte, Ordering::Relaxed);
        self.written
            .store(written.wrapping_add(1), Ordering::Release);
        true
    }

    /// Take the oldest byte, if any. Consumer side only.
    pub fn pop(&self) -> Option<u8> {
        let read = self.read.load(Ordering::Relaxed);
        let written = self.written.load(Ordering::Acquire);
        if read == written {
            return None;
        }
        let byte = self.data[read as usize % RING_BYTES].load(Ordering::Relaxed);
        self.read.store(read.wrapping_add(1), Ordering::Release);
        Some(byte)
    }
}

impl Default for Ring {
    fn default() -> Self {
        Self::new()
    }
}

/// The shared channel page.
#[repr(C)]
pub struct SerialChannel {
    /// Device options, written by the client before the driver starts
    /// ([`LOOPBACK`]).
    pub config: AtomicU64,
    /// Received bytes the driver dropped because `receive` was full.
    pub dropped: AtomicU64,
    /// Bytes from the client to the device.
    pub transmit: Ring,
    /// Bytes from the device to the client.
    pub receive: Ring,
}

const _: () = assert!(size_of::<SerialChannel>() <= 4096);

impl SerialChannel {
    pub const fn new() -> Self {
        Self {
            config: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            transmit: Ring::new(),
            receive: Ring::new(),
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

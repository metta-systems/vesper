#![no_std]

//! IO channels (xbufs) between a client and a server, such as a device driver,
//! after the Nemesis Rbufs channels (seL4's sDDF queues are their cousins).
//!
//! Bytes travel in a *data area*: memory both sides map at the same virtual
//! address (an fbuf), so an address means the same thing to both. What
//! travels through the channel itself are *packets* describing that memory:
//! each is a vector of 1 to [`MAX_PACKET_RECS`] [`IoRec`]s, `{base, len}`
//! pairs naming bytes of the data area, plus a value word the server hands
//! back unchanged.
//!
//! A [`Channel`] is two [`Fifo`]s of packets in a shared control page:
//! `submit`, from the client to the server, and `complete`, back from the
//! server. The client owns the buffers, and every packet it submits comes
//! back on `complete`:
//!
//! - in *transmit* mode it submits full buffers, and the server returns them
//!   once it took their bytes;
//! - in *receive* mode it submits empty buffers, and the server returns them
//!   filled, each record's `len` cut to the bytes stored there.
//!
//! The server never trusts a packet: [`DataArea::holds`] checks that every
//! record lies in the data area before the server touches it.
//!
//! Each FIFO's counters live in the control page, so putting and getting
//! packets needs no system call. To sleep, the consumer of a FIFO awaits an
//! [`Event`] the producer advances after putting packets: an `EventCount`
//! on Vesper, or [`NoEvent`] for a FIFO nobody sleeps on.

use core::sync::atomic::{AtomicU64, Ordering};

/// Most records one packet carries.
pub const MAX_PACKET_RECS: usize = 8;

/// A span of the data area: `len` bytes at virtual address `base`.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IoRec {
    pub base: u64,
    pub len: u64,
}

/// A vector of [`IoRec`]s and a value word, as one FIFO entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet {
    value: u64,
    count: usize,
    recs: [IoRec; MAX_PACKET_RECS],
}

impl Packet {
    /// A packet of `recs` tagged `value`; `None` without records or with
    /// more than [`MAX_PACKET_RECS`].
    pub fn new(value: u64, recs: &[IoRec]) -> Option<Self> {
        if recs.is_empty() || recs.len() > MAX_PACKET_RECS {
            return None;
        }
        let mut packet = Self {
            value,
            count: recs.len(),
            recs: [IoRec::default(); MAX_PACKET_RECS],
        };
        packet.recs[..recs.len()].copy_from_slice(recs);
        Some(packet)
    }

    /// Carried through the server unchanged: the client's own tag.
    pub fn value(&self) -> u64 {
        self.value
    }

    pub fn recs(&self) -> &[IoRec] {
        &self.recs[..self.count]
    }

    pub fn recs_mut(&mut self) -> &mut [IoRec] {
        &mut self.recs[..self.count]
    }

    /// Bytes all records span together, saturating.
    pub fn bytes(&self) -> u64 {
        self.recs()
            .iter()
            .fold(0, |total: u64, rec| total.saturating_add(rec.len))
    }
}

/// A position in the bytes of a packet, record by record: how a server walks
/// the bytes it sends from a packet or stores into one.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    rec: usize,
    offset: u64,
}

impl Cursor {
    /// Before the first byte.
    pub const START: Self = Self { rec: 0, offset: 0 };

    /// The address of the byte at the cursor, moving past it; `None` once
    /// every byte of `packet` was passed.
    pub fn next_address(&mut self, packet: &Packet) -> Option<u64> {
        while let Some(rec) = packet.recs().get(self.rec) {
            if self.offset < rec.len {
                let address = rec.base.wrapping_add(self.offset);
                self.offset += 1;
                return Some(address);
            }
            self.rec += 1;
            self.offset = 0;
        }
        None
    }

    /// Whether every byte of `packet` was passed.
    pub fn is_at_end(&self, packet: &Packet) -> bool {
        let mut probe = *self;
        probe.next_address(packet).is_none()
    }

    /// Bytes passed so far, saturating.
    pub fn passed(&self, packet: &Packet) -> u64 {
        packet.recs()[..self.rec.min(packet.count)]
            .iter()
            .fold(self.offset, |total, rec| total.saturating_add(rec.len))
    }

    /// Cut `packet`'s records to the bytes passed: the ones before the
    /// cursor keep their length, the one at it keeps what was passed, and
    /// the ones after it become empty. How a receive buffer comes back.
    pub fn cut(&self, packet: &mut Packet) {
        for (index, rec) in packet.recs_mut().iter_mut().enumerate() {
            if index == self.rec {
                rec.len = self.offset.min(rec.len);
            } else if index > self.rec {
                rec.len = 0;
            }
        }
    }
}

/// The data area: `len` bytes from virtual address `base`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataArea {
    pub base: u64,
    pub len: u64,
}

impl DataArea {
    /// Whether `rec` lies entirely inside the area.
    pub fn contains(&self, rec: &IoRec) -> bool {
        let Some(offset) = rec.base.checked_sub(self.base) else {
            return false;
        };
        offset
            .checked_add(rec.len)
            .is_some_and(|end| end <= self.len)
    }

    /// Whether every record of `packet` lies inside the area.
    pub fn holds(&self, packet: &Packet) -> bool {
        packet.recs().iter().all(|rec| self.contains(rec))
    }
}

/// Why a packet could not be put.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutError {
    /// The FIFO lacks room for the packet now.
    Full,
    /// The counters are inconsistent: the peer wrote ones it does not own.
    Corrupt,
}

/// The FIFO held something that is not a packet: the producer wrote
/// counters or records it does not own. The consumer skipped everything
/// published so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Corrupt;

/// One slot of a [`Fifo`]: a record, or a packet's header, whose `base` is
/// the packet's value and `len` its record count.
#[repr(C)]
struct Slot {
    base: AtomicU64,
    len: AtomicU64,
}

impl Slot {
    const fn new() -> Self {
        Self {
            base: AtomicU64::new(0),
            len: AtomicU64::new(0),
        }
    }

    fn load(&self) -> IoRec {
        IoRec {
            base: self.base.load(Ordering::Relaxed),
            len: self.len.load(Ordering::Relaxed),
        }
    }

    fn store(&self, rec: IoRec) {
        self.base.store(rec.base, Ordering::Relaxed);
        self.len.store(rec.len, Ordering::Relaxed);
    }
}

/// A single-producer, single-consumer FIFO of packets in shared memory. A
/// packet takes one header slot and one slot per record.
///
/// `produced` and `consumed` count slots ever written and read; their
/// difference is the fill level. Each side only advances its own counter,
/// and indexing wraps modulo `SLOTS`, so a peer writing anything into the
/// FIFO can garble its packets but never make either side touch memory
/// outside it. An all-zero FIFO is empty.
#[repr(C)]
pub struct Fifo<const SLOTS: usize> {
    produced: AtomicU64,
    consumed: AtomicU64,
    slots: [Slot; SLOTS],
}

impl<const SLOTS: usize> Fifo<SLOTS> {
    const FITS_A_PACKET: () = assert!(SLOTS > MAX_PACKET_RECS);

    pub const fn new() -> Self {
        let () = Self::FITS_A_PACKET;
        Self {
            produced: AtomicU64::new(0),
            consumed: AtomicU64::new(0),
            slots: [const { Slot::new() }; SLOTS],
        }
    }

    fn slot(&self, counter: u64) -> &Slot {
        // The remainder is below SLOTS, so it converts and indexes.
        &self.slots[usize::try_from(counter % SLOTS as u64).unwrap_or_default()]
    }

    /// Slots published and not yet consumed, if the counters agree.
    fn filled(&self) -> Option<u64> {
        let produced = self.produced.load(Ordering::Acquire);
        let consumed = self.consumed.load(Ordering::Acquire);
        let filled = produced.wrapping_sub(consumed);
        (filled <= SLOTS as u64).then_some(filled)
    }

    /// Whether no packet waits.
    pub fn is_empty(&self) -> bool {
        self.filled() == Some(0)
    }

    /// Whether a packet of `recs` records fits now. Producer side only.
    pub fn has_room(&self, recs: usize) -> bool {
        self.filled()
            .is_some_and(|filled| filled + (recs as u64) < SLOTS as u64)
    }

    /// Append `packet`. Producer side only.
    pub fn put(&self, packet: &Packet) -> Result<(), PutError> {
        let filled = self.filled().ok_or(PutError::Corrupt)?;
        let recs = packet.recs();
        if filled + recs.len() as u64 >= SLOTS as u64 {
            return Err(PutError::Full);
        }
        let produced = self.produced.load(Ordering::Relaxed);
        self.slot(produced).store(IoRec {
            base: packet.value,
            len: recs.len() as u64,
        });
        for (index, rec) in (1..).zip(recs) {
            self.slot(produced.wrapping_add(index)).store(*rec);
        }
        self.produced.store(
            produced.wrapping_add(recs.len() as u64 + 1),
            Ordering::Release,
        );
        Ok(())
    }

    /// Take the oldest packet, if one waits. Consumer side only.
    pub fn get(&self) -> Result<Option<Packet>, Corrupt> {
        let consumed = self.consumed.load(Ordering::Relaxed);
        let produced = self.produced.load(Ordering::Acquire);
        let filled = produced.wrapping_sub(consumed);
        if filled == 0 {
            return Ok(None);
        }
        let header = self.slot(consumed).load();
        let count = usize::try_from(header.len).unwrap_or(usize::MAX);
        if filled > SLOTS as u64
            || count == 0
            || count > MAX_PACKET_RECS
            || count as u64 + 1 > filled
        {
            self.consumed.store(produced, Ordering::Release);
            return Err(Corrupt);
        }
        let mut packet = Packet {
            value: header.base,
            count,
            recs: [IoRec::default(); MAX_PACKET_RECS],
        };
        for (index, rec) in (1..).zip(packet.recs_mut()) {
            *rec = self.slot(consumed.wrapping_add(index)).load();
        }
        self.consumed
            .store(consumed.wrapping_add(count as u64 + 1), Ordering::Release);
        Ok(Some(packet))
    }
}

impl<const SLOTS: usize> Default for Fifo<SLOTS> {
    fn default() -> Self {
        Self::new()
    }
}

/// One IO channel's control area: packets the client submits to the server
/// and the server completes back. All zeroes is an idle channel.
#[repr(C)]
pub struct Channel<const SLOTS: usize> {
    pub submit: Fifo<SLOTS>,
    pub complete: Fifo<SLOTS>,
}

impl<const SLOTS: usize> Channel<SLOTS> {
    pub const fn new() -> Self {
        Self {
            submit: Fifo::new(),
            complete: Fifo::new(),
        }
    }

    /// The client's ends: it submits through `submitted`'s producer side and
    /// waits on `completed`.
    pub fn client<Submitted: Event, Completed: Event>(
        &self,
        submitted: Submitted,
        completed: Completed,
    ) -> (
        Producer<'_, SLOTS, Submitted>,
        Consumer<'_, SLOTS, Completed>,
    ) {
        (
            Producer::new(&self.submit, submitted),
            Consumer::new(&self.complete, completed),
        )
    }

    /// The server's ends: it waits on `submitted` and completes through
    /// `completed`.
    pub fn server<Submitted: Event, Completed: Event>(
        &self,
        submitted: Submitted,
        completed: Completed,
    ) -> (
        Consumer<'_, SLOTS, Submitted>,
        Producer<'_, SLOTS, Completed>,
    ) {
        (
            Consumer::new(&self.submit, submitted),
            Producer::new(&self.complete, completed),
        )
    }
}

impl<const SLOTS: usize> Default for Channel<SLOTS> {
    fn default() -> Self {
        Self::new()
    }
}

/// What a FIFO's consumer sleeps on and its producer advances: an
/// `EventCount`, counting the producer's signals.
pub trait Event {
    type Error;

    /// Advance the count by one.
    fn advance(&self) -> Result<(), Self::Error>;

    /// Block until the count reaches `target`; the count then.
    fn await_at_least(&self, target: u64) -> Result<u64, Self::Error>;
}

/// The event of a FIFO nobody sleeps on: advancing it does nothing, and
/// awaiting it fails.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoEvent;

/// Awaiting [`NoEvent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unwaitable;

impl Event for NoEvent {
    type Error = Unwaitable;

    fn advance(&self) -> Result<(), Unwaitable> {
        Ok(())
    }

    fn await_at_least(&self, _target: u64) -> Result<u64, Unwaitable> {
        Err(Unwaitable)
    }
}

#[cfg(target_os = "vesper")]
impl Event for libobject::EventCountKey {
    type Error = libobject::CapError;

    fn advance(&self) -> Result<(), Self::Error> {
        Self::advance(self, 1).map(|_| ())
    }

    fn await_at_least(&self, target: u64) -> Result<u64, Self::Error> {
        self.await_ge(target, Self::WAIT_INFINITE)
    }
}

/// The producer side of a [`Fifo`] and the event its consumer sleeps on.
pub struct Producer<'channel, const SLOTS: usize, E: Event> {
    fifo: &'channel Fifo<SLOTS>,
    event: E,
    unsignalled: bool,
}

impl<'channel, const SLOTS: usize, E: Event> Producer<'channel, SLOTS, E> {
    pub const fn new(fifo: &'channel Fifo<SLOTS>, event: E) -> Self {
        Self {
            fifo,
            event,
            unsignalled: false,
        }
    }

    /// Whether a packet of `recs` records fits now.
    pub fn has_room(&self, recs: usize) -> bool {
        self.fifo.has_room(recs)
    }

    /// Append `packet`; the consumer learns of it at the next
    /// [`Self::signal`].
    pub fn put(&mut self, packet: &Packet) -> Result<(), PutError> {
        self.fifo.put(packet)?;
        self.unsignalled = true;
        Ok(())
    }

    /// Advance the event once if packets were put since the last signal.
    pub fn signal(&mut self) -> Result<(), E::Error> {
        if self.unsignalled {
            self.event.advance()?;
            self.unsignalled = false;
        }
        Ok(())
    }
}

/// Why [`Consumer::wait`] returned no packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitError<E> {
    Corrupt,
    Event(E),
}

/// The consumer side of a [`Fifo`] and the event it sleeps on.
pub struct Consumer<'channel, const SLOTS: usize, E: Event> {
    fifo: &'channel Fifo<SLOTS>,
    event: E,
    /// The event's count when this side last woke
    seen: u64,
}

impl<'channel, const SLOTS: usize, E: Event> Consumer<'channel, SLOTS, E> {
    pub const fn new(fifo: &'channel Fifo<SLOTS>, event: E) -> Self {
        Self {
            fifo,
            event,
            seen: 0,
        }
    }

    /// Take the oldest packet, if one waits.
    pub fn get(&mut self) -> Result<Option<Packet>, Corrupt> {
        self.fifo.get()
    }

    /// Take the oldest packet, sleeping on the event until one comes.
    ///
    /// The producer publishes packets before it advances the event, so
    /// every packet signalled by a count this side has seen is already in
    /// the FIFO; an advance with no packet behind it costs one more look.
    pub fn wait(&mut self) -> Result<Packet, WaitError<E::Error>> {
        loop {
            if let Some(packet) = self.fifo.get().map_err(|Corrupt| WaitError::Corrupt)? {
                return Ok(packet);
            }
            self.seen = self
                .event
                .await_at_least(self.seen.wrapping_add(1))
                .map_err(WaitError::Event)?;
        }
    }
}

//! The event channel of a client: the io-channel each client of
//! `compositor_v1` shares with the compositor (`channel 1;`), and both of its
//! ends.
//!
//! It runs in receive mode (see `libio-channel`): the client posts empty
//! buffers of the channel's data page ([`Receiver`]), and the compositor
//! returns each holding the records of one routed frame for the client's
//! windows ([`deliver`]): for each window its records, closed by a `Sync`,
//! with the window's id in `subject`. A delivery that finds no buffer posted,
//! or one too small, is dropped whole: the newest records are lost, and the
//! next delivery starts with a `Dropped` record (subject 0). A client that
//! reads `Dropped` rebuilds its state with `compositor_v1.snapshot`
//! ([`Router::snapshot`](crate::router::Router::snapshot)).

use {
    crate::{router::Routed, windows::Windows},
    libio_channel::{
        Channel, Consumer, Cursor, DataArea, Event as Signal, IoRec, NoEvent, Packet, Producer,
        WaitError,
    },
    vesper_input_event::record::{EVENT_SIZE, Event, Kind},
};

/// Slots of the channel's FIFOs.
pub const SLOTS: usize = 16;

/// Bytes of the channel's data page.
pub const DATA_BYTES: u64 = 4096;

/// The bytes of each buffer a [`Receiver`] keeps posted, and how many it
/// keeps: the data page split in four.
pub const BUFFER_LEN: usize = 1024;
pub const BUFFER_BYTES: u64 = BUFFER_LEN as u64;
pub const BUFFERS: u64 = DATA_BYTES / BUFFER_BYTES;

/// Records a buffer holds.
pub const BUFFER_RECORDS: usize = BUFFER_LEN / EVENT_SIZE;

/// The channel in its control page.
#[repr(C)]
pub struct EventChannel {
    /// Empty buffers from the client; deliveries back from the compositor.
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

/// The data area behind a control page at `control`: the one page that
/// follows it.
#[must_use]
pub const fn data_area(control: u64) -> DataArea {
    DataArea {
        base: control + 4096,
        len: DATA_BYTES,
    }
}

/// The records of `routed` that go to the windows of `client`, in order.
pub fn client_records<'routed>(
    routed: &'routed [Routed],
    windows: &'routed Windows,
    client: u32,
) -> impl Iterator<Item = Event> + Clone + 'routed {
    routed
        .iter()
        .filter(move |routed| {
            windows
                .get(routed.window)
                .is_some_and(|window| window.client == client)
        })
        .map(|routed| routed.event)
}

/// What the compositor knows of one client's channel: whether deliveries
/// were lost since the last one that arrived.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outbox {
    missed: bool,
    dropped: u32,
}

impl Outbox {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            missed: false,
            dropped: 0,
        }
    }

    /// Deliveries dropped so far.
    #[must_use]
    pub const fn dropped(&self) -> u32 {
        self.dropped
    }

    /// Whether the next delivery starts with `Dropped`.
    #[must_use]
    pub const fn missed(&self) -> bool {
        self.missed
    }

    /// Admit a delivery of `count` records into a buffer of `capacity` bytes
    /// (`None`: the client posted none). When it fits, returns the `Dropped`
    /// record stamped `stamp` to send before the records if deliveries were
    /// lost since the last one, else `None` inside. Otherwise the delivery is
    /// dropped and counted, the next one starts with `Dropped`, and `None` is
    /// returned.
    pub fn admit(
        &mut self,
        count: usize,
        stamp: u64,
        capacity: Option<u64>,
    ) -> Option<Option<Event>> {
        let length = (usize::from(self.missed) + count) * EVENT_SIZE;
        if !capacity.is_some_and(|capacity| capacity >= length as u64) {
            self.missed = true;
            self.dropped = self.dropped.saturating_add(1);
            return None;
        }
        let marker = self
            .missed
            .then(|| Event::new(stamp, 0, Kind::Dropped, 0, 0));
        self.missed = false;
        Some(marker)
    }
}

/// Deliver `records` to the client of `channel` in the first buffer it
/// posted, following `outbox`, and advance `completed` for it. The records
/// are streamed into the buffer, so the caller's stack holds none of them.
/// Nothing is sent for no records. Returns whether the records were
/// delivered.
///
/// # Safety
///
/// `data` is the channel's data area, mapped read/write for the call.
pub unsafe fn deliver<Completed: Signal>(
    channel: &EventChannel,
    data: DataArea,
    completed: Completed,
    outbox: &mut Outbox,
    records: impl Iterator<Item = Event> + Clone,
    stamp: u64,
) -> bool {
    let count = records.clone().count();
    if count == 0 {
        return true;
    }
    let (mut posted, mut done) = channel.events.server(NoEvent, completed);
    let mut packet = posted.get().ok().flatten();
    // A packet outside the data area is no buffer at all: it goes back empty.
    let capacity = packet
        .as_ref()
        .filter(|packet| data.holds(packet))
        .map(Packet::bytes);
    let admitted = outbox.admit(count, stamp, capacity);
    let Some(packet) = packet.as_mut() else {
        return false;
    };
    let mut cursor = Cursor::START;
    if let Some(marker) = admitted {
        for record in marker.into_iter().chain(records) {
            for byte in record.to_bytes() {
                let Some(address) = cursor.next_address(packet) else {
                    break;
                };
                // SAFETY: the data area holds the packet (it is not admitted
                // otherwise, and nothing is written) and is mapped
                // read/write; the client gave the buffer to the compositor
                // until it comes back.
                unsafe {
                    core::ptr::with_exposed_provenance_mut::<u8>(
                        usize::try_from(address).unwrap_or(usize::MAX),
                    )
                    .write_volatile(byte);
                }
            }
        }
    }
    cursor.cut(packet);
    // A client with more buffers out than the FIFO holds loses the delivery.
    let _lost = done.put(packet);
    // Only the client's EventCount can fail to advance.
    let _signalled = done.signal();
    admitted.is_some()
}

/// The client's end: it keeps the data page's buffers posted and hands out
/// the records that come back, one at a time, without blocking
/// ([`next_event`](Self::next_event)) or sleeping until one comes
/// ([`wait_event`](Self::wait_event)).
pub struct Receiver<'channel, Completed: Signal> {
    buffers: Producer<'channel, SLOTS, NoEvent>,
    completed: Consumer<'channel, SLOTS, Completed>,
    data: DataArea,
    pending: [Event; BUFFER_RECORDS],
    pending_len: usize,
    next: usize,
    rejected: u32,
}

impl<'channel, Completed: Signal> Receiver<'channel, Completed> {
    /// Post the buffers of `data` on `channel`, whose deliveries advance
    /// `completed`.
    ///
    /// # Safety
    ///
    /// `data` is the channel's data area, mapped readable for as long as the
    /// receiver is used.
    pub unsafe fn new(
        channel: &'channel EventChannel,
        data: DataArea,
        completed: Completed,
    ) -> Self {
        let (mut buffers, completed) = channel.events.client(NoEvent, completed);
        for index in 0..BUFFERS {
            let rec = IoRec {
                base: data.base + index * BUFFER_BYTES,
                len: BUFFER_BYTES,
            };
            if let Some(packet) = Packet::new(index, &[rec]) {
                let _posted = buffers.put(&packet);
            }
        }
        Self {
            buffers,
            completed,
            data,
            pending: [Event::sync(0, 0); BUFFER_RECORDS],
            pending_len: 0,
            next: 0,
            rejected: 0,
        }
    }

    /// Records the compositor sent that did not decode, or lay outside the
    /// data area.
    #[must_use]
    pub const fn rejected(&self) -> u32 {
        self.rejected
    }

    /// The next record, if one arrived.
    pub fn next_event(&mut self) -> Option<Event> {
        while self.next == self.pending_len {
            let packet = self.completed.get().ok().flatten()?;
            self.take(&packet);
        }
        let event = self.pending[self.next];
        self.next += 1;
        Some(event)
    }

    /// The next record, sleeping on the channel's `EventCount` until a
    /// delivery arrives; for a client with nothing to do until its windows'
    /// events change.
    ///
    /// # Errors
    ///
    /// The channel is corrupt, or awaiting the `EventCount` failed.
    pub fn wait_event(&mut self) -> Result<Event, WaitError<Completed::Error>> {
        loop {
            if let Some(event) = self.next_event() {
                return Ok(event);
            }
            let packet = self.completed.wait()?;
            self.take(&packet);
        }
    }

    /// Copy the records of a returned buffer and post it again.
    fn take(&mut self, packet: &Packet) {
        self.pending_len = 0;
        self.next = 0;
        let Some(rec) = packet.recs().first().copied() else {
            return;
        };
        let whole = IoRec {
            base: rec.base,
            len: BUFFER_BYTES,
        };
        if !self.data.contains(&rec) || !self.data.contains(&whole) || rec.len > BUFFER_BYTES {
            self.rejected = self.rejected.saturating_add(1);
            return;
        }
        let length = usize::try_from(rec.len).unwrap_or(0);
        let base = usize::try_from(rec.base).unwrap_or(0);
        for start in (0..length / EVENT_SIZE).map(|index| base + index * EVENT_SIZE) {
            let mut record = [0_u8; EVENT_SIZE];
            for (offset, byte) in record.iter_mut().enumerate() {
                // SAFETY: the buffer lies in the data area, mapped readable
                // (the contract of `new`); the compositor gave it back.
                *byte = unsafe {
                    core::ptr::with_exposed_provenance::<u8>(start + offset).read_volatile()
                };
            }
            match Event::from_bytes(&record) {
                Ok(event) => {
                    self.pending[self.pending_len] = event;
                    self.pending_len += 1;
                }
                Err(_) => self.rejected = self.rejected.saturating_add(1),
            }
        }
        if let Some(again) = Packet::new(packet.value(), &[whole]) {
            let _posted = self.buffers.put(&again);
        }
    }
}

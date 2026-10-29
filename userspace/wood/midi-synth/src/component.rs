//! The component's start-up and its `midi_synth_v1` exports (see the crate
//! documentation in `main.rs`).

use {
    crate::heap,
    alloc::vec::Vec,
    core::{
        cell::UnsafeCell,
        sync::atomic::{AtomicBool, AtomicU64, Ordering},
    },
    libdevice_driver::{DriverInit, INIT_VA, driver_init},
    libio_channel::{DataArea, IoRec, NoEvent},
    libmeddle_runtime::PAGE_BYTES,
    libobject::{EventCountKey, RawKey, export::init_return_key, thread::ThreadReturnKey},
    libserial_trace::note,
    midi_synth::{Handled, Memory, RATE_MILLIHERTZ, Session, channel_at},
    midi_synth_server::midi_synth_v1::{CHANNEL_PAGES, Exception, MidiSynthV1},
    wood_synth::Bank,
};

libuser::entry!(main);

fn init() -> &'static DriverInit {
    // SAFETY: Kickstart maps every component's init record at `INIT_VA`; it
    // is read in this component's own `AddressSpace` only.
    unsafe { driver_init(INIT_VA) }
}

/// The client holding the synthesiser, by its position among the data areas,
/// plus one; zero while nobody does.
static OWNER: AtomicU64 = AtomicU64::new(0);

/// The synthesiser, behind a spin lock: built by the component's Thread at
/// start, used by `service` on the client's Thread afterwards.
struct Shared {
    locked: AtomicBool,
    session: UnsafeCell<Option<Session<'static>>>,
}

// SAFETY: `session` is touched only while `locked` is held.
unsafe impl Sync for Shared {}

impl Shared {
    /// Run `body` on the session, holding the lock; `None` before start-up
    /// built it.
    fn with<Answer>(&self, body: impl FnOnce(&mut Session<'static>) -> Answer) -> Option<Answer> {
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        // SAFETY: the lock is held, so this is the only reference.
        let answer = unsafe { &mut *self.session.get() }.as_mut().map(body);
        self.locked.store(false, Ordering::Release);
        answer
    }

    fn set(&self, session: Session<'static>) {
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        // SAFETY: the lock is held, so this is the only reference.
        unsafe {
            *self.session.get() = Some(session);
        }
        self.locked.store(false, Ordering::Release);
    }
}

static SHARED: Shared = Shared {
    locked: AtomicBool::new(false),
    session: UnsafeCell::new(None),
};

fn main(_init_va: u64) -> ! {
    let init = init();
    let ready = EventCountKey::from_key(RawKey::from_wire(init.event_count));
    let slot = u32::try_from(init.first_free_slot).unwrap_or(u32::MAX);
    match start(init, slot) {
        Ok(session) => {
            SHARED.set(session);
            note(format_args!("midi-synth: ready at {RATE_MILLIHERTZ} mHz"));
        }
        Err(reason) => note(format_args!(
            "midi-synth: {reason}; every song stays silent"
        )),
    }
    // Say the exports can be served (without a bank they answer `busy`).
    ready
        .advance(1)
        .unwrap_or_else(|error| panic!("midi-synth: ready advance failed: {:?}", error.code()));
    loop {
        let _parked = ready.await_ge(u64::MAX, EventCountKey::WAIT_INFINITE);
    }
}

/// Carve the heap and load the bundled SoundFont.
fn start(init: &DriverInit, slot: u32) -> Result<Session<'static>, &'static str> {
    heap::carve(init, slot).map_err(|_error| "carving the heap failed")?;
    let bytes = init.resource(0).ok_or("no SoundFont resource")?;
    let bank = Bank::new(bytes).map_err(|_error| "the SoundFont resource does not parse")?;
    note(format_args!(
        "midi-synth: SoundFont of {} bytes, {} presets, {} instruments, {} samples",
        bytes.len(),
        bank.font().preset_count(),
        bank.font().instrument_count(),
        bank.font().sample_count()
    ));
    Ok(Session::new(bank))
}

/// Wait until start-up finished.
fn wait_ready() {
    EventCountKey::from_key(RawKey::from_wire(init().event_count))
        .await_ge(1, EventCountKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("midi-synth: ready wait failed: {:?}", error.code()));
}

/// The data pages of a client's channel, mapped at the same address here.
struct DataPages {
    area: DataArea,
}

/// The bytes of `record`.
///
/// # Safety
///
/// `record` must lie in data pages mapped read/write for `'page` that the
/// peer leaves alone meanwhile.
unsafe fn record_bytes<'page>(record: IoRec) -> &'page mut [u8] {
    let address = usize::try_from(record.base).unwrap_or(usize::MAX);
    let len = usize::try_from(record.len).unwrap_or(0);
    // SAFETY: forwarded caller contract.
    unsafe { core::slice::from_raw_parts_mut(core::ptr::with_exposed_provenance_mut(address), len) }
}

impl Memory for DataPages {
    fn read(&self, record: IoRec, out: &mut Vec<u8>) {
        if self.area.contains(&record) {
            // SAFETY: the area holds the record; the data pages are mapped
            // read/write for good, and the client leaves a buffer alone
            // while its packet is here.
            out.extend_from_slice(unsafe { record_bytes(record) });
        }
    }

    fn write(&mut self, address: u64, bytes: &[u8]) {
        let record = IoRec {
            base: address,
            len: bytes.len() as u64,
        };
        if self.area.contains(&record) {
            // SAFETY: as in `read`.
            unsafe { record_bytes(record) }.copy_from_slice(bytes);
        }
    }
}

/// The caller's position among the data areas, if it holds the synthesiser.
fn owner(badge: u64) -> Result<usize, Exception> {
    let (client, _area) = init().area_for_badge(badge).ok_or(Exception::NotOpen)?;
    if OWNER.load(Ordering::Acquire) == client as u64 + 1 {
        Ok(client)
    } else {
        Err(Exception::NotOpen)
    }
}

/// Handle every packet the client of `badge` put; how many.
fn service_channel(badge: u64) -> Result<u32, Exception> {
    owner(badge)?;
    let (_client, area) = init().area_for_badge(badge).ok_or(Exception::NotOpen)?;
    if area.channel_base == 0 {
        return Ok(0);
    }
    // SAFETY: Kickstart mapped the client's control page read/write for good,
    // shared with that client only.
    let channel = unsafe { channel_at(area.channel_base) };
    let (mut submitted, mut completed) = channel.server::<NoEvent, EventCountKey>(
        NoEvent,
        EventCountKey::from_key(RawKey::from_wire(area.channel_event)),
    );
    let data = DataArea {
        base: area.channel_base + PAGE_BYTES,
        len: CHANNEL_PAGES * PAGE_BYTES,
    };
    let mut pages = DataPages { area: data };
    let mut handled = 0_u32;
    SHARED
        .with(|session| {
            while let Ok(Some(mut packet)) = submitted.get() {
                match session.handle(&mut packet, data, &mut pages) {
                    Handled::Song(Some(bytes)) => {
                        note(format_args!("midi-synth: a song of {bytes} bytes plays"));
                    }
                    Handled::BadSong => note(format_args!("midi-synth: a song does not parse")),
                    Handled::Refused => {
                        note(format_args!("midi-synth: packet {:#x} refused", packet.value()));
                    }
                    Handled::Song(None) | Handled::Rendered(_) | Handled::Control => {}
                }
                if completed.put(&packet).is_err() {
                    note(format_args!("midi-synth: the completion FIFO is full"));
                }
                handled += 1;
                if let Some(start) = session.take_start_hash() {
                    note(format_args!(
                        "midi-synth: song of {} bytes (looping {}, volume {}): first 64 periods hash {:#018x}, {} audible",
                        start.song_bytes,
                        start.looping,
                        start.volume,
                        start.hash,
                        start.audible_periods
                    ));
                }
            }
        })
        .ok_or(Exception::Busy)?;
    if completed.signal().is_err() {
        note(format_args!("midi-synth: signalling completions failed"));
    }
    Ok(handled)
}

/// The synthesiser, behind `midi_synth_v1`. The calls run on their callers'
/// Threads.
struct Synthesiser;

impl MidiSynthV1 for Synthesiser {
    fn enter(_badge: u64) {
        init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
            init().return_key,
        )));
    }

    fn rate(_badge: u64) -> u32 {
        RATE_MILLIHERTZ
    }

    fn open(badge: u64) -> Result<(), Exception> {
        wait_ready();
        let (client, _area) = init().area_for_badge(badge).ok_or(Exception::Busy)?;
        // Without a bank there is nothing to open.
        SHARED.with(|_session| ()).ok_or(Exception::Busy)?;
        OWNER
            .compare_exchange(0, client as u64 + 1, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_owner| Exception::Busy)?;
        Ok(())
    }

    fn close(badge: u64) -> Result<(), Exception> {
        let client = owner(badge)?;
        SHARED.with(Session::reset);
        OWNER
            .compare_exchange(client as u64 + 1, 0, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_owner| Exception::NotOpen)?;
        Ok(())
    }

    fn service(badge: u64) -> Result<u32, Exception> {
        service_channel(badge)
    }

    fn playing(badge: u64) -> Result<u32, Exception> {
        owner(badge)?;
        Ok(SHARED
            .with(|session| u32::from(session.playing()))
            .unwrap_or(0))
    }

    fn voices(badge: u64) -> Result<u32, Exception> {
        owner(badge)?;
        Ok(SHARED
            .with(|session| u32::try_from(session.take_peak_voices()).unwrap_or(u32::MAX))
            .unwrap_or(0))
    }
}

midi_synth_server::midi_synth_v1_export!(Synthesiser);

//! Wood's music: the engine's songs played by the MIDI synthesiser component
//! through `midi_synth_v1`, pulled by the playback Thread one period at a
//! time and mixed under the sound effects (decision D10 of
//! `doc/agentic/audio-design.md`).
//!
//! The game Thread, through Wood's `music_*` platform methods, only queues
//! [`MusicCommand`]s ([`COMMANDS`]) and never waits on another component. The
//! playback Thread ([`Music::fill`]) sends what was queued on the
//! interface's io-channel: a song's MIDI bytes as song packets through the
//! song area of the data pages (several packets for a large song), the other
//! commands as packets of their own, then a render packet for the period's
//! frames; one `service` call has the synthesiser handle them all, and the
//! rendered frames come back in the render buffer. Without the synthesiser
//! (an image without it, or a refusal) the music is silence and the commands
//! are dropped.

use {
    alloc::vec::Vec,
    core::{
        cell::UnsafeCell,
        sync::atomic::{AtomicBool, Ordering},
    },
    libdevice_driver::DriverInit,
    libio_channel::{Consumer, IoRec, NoEvent, Packet, Producer},
    libmeddle_runtime::PAGE_BYTES,
    libobject::{EventCountKey, RawKey},
    mailbox_client::mailbox_v1::{Bindings as MailboxBindings, Client as MailboxClient, Clock},
    midi_synth::{
        CHANNEL_SLOTS, Command, PERIOD_BYTES, PERIOD_FRAMES, RENDER_AREA_BYTES, SONG_FIRST,
        SONG_LAST, SONG_LOOPING, ServiceWindow, channel_at, volume_value,
    },
    midi_synth_client::midi_synth_v1::{Bindings, CHANNEL_PAGES, Client, Error, Exception},
};

/// What the engine asked of the music, in order.
pub enum MusicCommand {
    /// Play the MIDI file, looping or not.
    Play(Vec<u8>, bool),
    Stop,
    Pause,
    Resume,
    /// Music volume, 0 to 127.
    Volume(u8),
}

/// The commands the game Thread queued and the playback Thread has not sent
/// yet, behind a spin lock (held for a push or a swap only).
pub struct Commands {
    locked: AtomicBool,
    queue: UnsafeCell<Vec<MusicCommand>>,
}

// SAFETY: `queue` is touched only while `locked` is held.
unsafe impl Sync for Commands {}

impl Commands {
    const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
            queue: UnsafeCell::new(Vec::new()),
        }
    }

    fn with<Answer>(&self, body: impl FnOnce(&mut Vec<MusicCommand>) -> Answer) -> Answer {
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        // SAFETY: the lock is held, so this is the only reference.
        let answer = body(unsafe { &mut *self.queue.get() });
        self.locked.store(false, Ordering::Release);
        answer
    }

    /// Queue `command`. Game Thread.
    pub fn push(&self, command: MusicCommand) {
        self.with(|queue| queue.push(command));
    }

    /// Everything queued so far. Playback Thread.
    pub fn take(&self) -> Vec<MusicCommand> {
        self.with(core::mem::take)
    }
}

/// The commands between the Threads.
pub static COMMANDS: Commands = Commands::new();

/// Whether a song plays, as the engine sees it: set when it asks for one,
/// cleared when it stops one, and refreshed from the synthesiser.
pub static PLAYING: AtomicBool = AtomicBool::new(false);

/// Periods between two `playing` questions to the synthesiser (about 0.4 s).
const PLAYING_EVERY: u64 = 32;

type Submit = Producer<'static, CHANNEL_SLOTS, NoEvent>;
type Completions = Consumer<'static, CHANNEL_SLOTS, EventCountKey>;

/// The synthesiser, taken: Wood's end of `midi_synth_v1`.
pub struct Music {
    client: Client,
    submit: Submit,
    completions: Completions,
    /// The first data page: the render buffer, then the song area.
    data_base: u64,
    /// Periods pulled, those with sound, and songs sent
    periods: u64,
    audible: u64,
    songs: u64,
    /// Nanoseconds the last `service` took
    last_service_ns: u64,
    /// The `service` calls since the last [`Music::take_window`]
    window: ServiceWindow,
}

/// Log the ARM and core clocks the firmware runs at, if the image lets Wood
/// ask the mailbox: the synthesiser's render time per period depends on them.
fn log_clocks(init: &DriverInit) {
    let mailbox = MailboxClient::new(MailboxBindings::from_imports(init.imports()));
    let megahertz = |clock: Clock| {
        mailbox
            .get_clock_rate(clock)
            .map_or(0, |hertz| hertz / 1_000_000)
    };
    log::info!(
        "music: ARM clock {} MHz, core clock {} MHz (0: unknown)",
        megahertz(Clock::Arm),
        megahertz(Clock::Core)
    );
}

impl Music {
    /// Take the synthesiser, if the image has one; why not otherwise.
    pub fn open(init: &DriverInit, audio_millihertz: u32) -> Result<Self, &'static str> {
        log_clocks(init);
        let client = Client::new(Bindings::from_imports(init.imports()));
        let rate = client.rate().map_err(|error| match error {
            Error::NotGranted => "no MIDI synthesiser in the image",
            _ => "the MIDI synthesiser did not answer",
        })?;
        if rate != audio_millihertz {
            return Err("the MIDI synthesiser renders at another rate");
        }
        let area = client
            .channel()
            .ok_or("no io-channel to the MIDI synthesiser")?;
        if area.event == 0 || area.data.len != CHANNEL_PAGES * PAGE_BYTES {
            return Err("the io-channel to the MIDI synthesiser is not as declared");
        }
        client.open().map_err(|error| match error {
            Error::Exception(Exception::Busy) => "the MIDI synthesiser is busy or has no bank",
            _ => "opening the MIDI synthesiser failed",
        })?;
        // SAFETY: Kickstart mapped the channel's control page read/write for
        // good, shared with the synthesiser only.
        let channel = unsafe { channel_at(area.control) };
        let (submit, completions) = channel.client(
            NoEvent,
            EventCountKey::from_key(RawKey::from_wire(area.event)),
        );
        log::info!("music: the MIDI synthesiser plays at {rate} mHz");
        Ok(Self {
            client,
            submit,
            completions,
            data_base: area.data.base,
            periods: 0,
            audible: 0,
            songs: 0,
            last_service_ns: 0,
            window: ServiceWindow::EMPTY,
        })
    }

    /// What happened since the last call, starting a new window.
    pub fn take_window(&mut self) -> ServiceWindow {
        core::mem::replace(&mut self.window, ServiceWindow::EMPTY)
    }

    /// Bytes of the song area, after the render buffers.
    const fn song_area_bytes() -> u64 {
        CHANNEL_PAGES * PAGE_BYTES - RENDER_AREA_BYTES
    }

    /// Put `packet`; `false` if the FIFO is full.
    fn put(&mut self, packet: &Packet) -> bool {
        if self.submit.put(packet).is_ok() {
            true
        } else {
            log::warn!("music: the io-channel is full");
            false
        }
    }

    /// Have the synthesiser handle what was put and take every packet back;
    /// the render buffer's frames, if a render packet came back.
    fn service(&mut self) -> Option<u64> {
        let started = libuser::clock::now_ns();
        let handled = self.client.service().ok();
        if handled.is_none() {
            log::warn!("music: service failed");
        }
        self.last_service_ns = libuser::clock::now_ns().saturating_sub(started);
        self.window.record_call(self.last_service_ns, handled);
        let mut rendered = None;
        while let Ok(Some(back)) = self.completions.get() {
            if Command::of(back.value()) == Some(Command::Render) {
                rendered = back.recs().first().map(|record| record.len / 4);
            }
        }
        rendered
    }

    /// A packet of `command` with an empty record.
    fn control(&mut self, value: u64) {
        let record = IoRec {
            base: self.data_base,
            len: 0,
        };
        if let Some(packet) = Packet::new(value, &[record]) {
            self.put(&packet);
        }
    }

    /// Send `midi` through the song area, a packet at a time.
    fn send_song(&mut self, midi: &[u8], looping: bool) {
        let area = self.data_base + RENDER_AREA_BYTES;
        let chunk = usize::try_from(Self::song_area_bytes()).unwrap_or(4096);
        let count = midi.len().div_ceil(chunk);
        for (index, bytes) in midi.chunks(chunk).enumerate() {
            let target = usize::try_from(area).unwrap_or(usize::MAX);
            // SAFETY: the song area lies in the data pages, mapped
            // read/write for good; the synthesiser reads it only while it
            // handles this packet, during the `service` below.
            unsafe {
                core::slice::from_raw_parts_mut(
                    core::ptr::with_exposed_provenance_mut::<u8>(target),
                    bytes.len(),
                )
            }
            .copy_from_slice(bytes);
            let mut flags = 0;
            if index == 0 {
                flags |= SONG_FIRST;
            }
            if index + 1 == count {
                flags |= SONG_LAST;
                if looping {
                    flags |= SONG_LOOPING;
                }
            }
            let record = IoRec {
                base: area,
                len: bytes.len() as u64,
            };
            if let Some(packet) = Packet::new(Command::Song.value(flags, 0), &[record])
                && self.put(&packet)
            {
                self.service();
            }
        }
        self.songs += 1;
        log::info!(
            "music: song {} of {} bytes sent, looping {looping}",
            self.songs,
            midi.len()
        );
    }

    /// Send what the engine queued, then render the next period into
    /// `frames` (silence where the synthesiser gave none). Playback Thread.
    pub fn fill(&mut self, frames: &mut [[i16; 2]]) {
        for command in COMMANDS.take() {
            match command {
                MusicCommand::Play(midi, looping) => self.send_song(&midi, looping),
                MusicCommand::Stop => self.control(Command::Stop.value(0, 0)),
                MusicCommand::Pause => self.control(Command::Pause.value(0, 0)),
                MusicCommand::Resume => self.control(Command::Resume.value(0, 0)),
                MusicCommand::Volume(volume) => self.control(volume_value(volume, 0)),
            }
        }
        let render = IoRec {
            base: self.data_base,
            len: PERIOD_BYTES.min(frames.len() as u64 * 4),
        };
        let rendered = Packet::new(Command::Render.value(0, 0), &[render])
            .filter(|packet| self.put(packet))
            .and_then(|_packet| self.service())
            .unwrap_or(0);
        let rendered = usize::try_from(rendered).unwrap_or(0).min(frames.len());
        let address = usize::try_from(self.data_base).unwrap_or(usize::MAX);
        // SAFETY: the render buffer lies at the start of the data pages,
        // mapped read/write for good; the synthesiser wrote it during
        // `service` and holds no packet now. A frame is two little-endian
        // signed 16-bit samples, as `[i16; 2]` lies in memory.
        let music = unsafe {
            core::slice::from_raw_parts(
                core::ptr::with_exposed_provenance::<[i16; 2]>(address),
                PERIOD_FRAMES,
            )
        };
        frames[..rendered].copy_from_slice(&music[..rendered]);
        frames[rendered..].fill([0, 0]);

        self.periods += 1;
        let audible = frames[..rendered].iter().any(|&frame| frame != [0, 0]);
        self.window.record_period(audible);
        if audible {
            self.audible += 1;
            if self.audible.is_power_of_two() {
                self.report();
            }
        }
        if self.periods.is_multiple_of(PLAYING_EVERY) {
            let playing = self.client.playing().is_ok_and(|playing| playing != 0);
            PLAYING.store(playing, Ordering::Release);
        }
    }

    /// Log the periods pulled and the last `service` time.
    fn report(&self) {
        log::info!(
            "music: {} periods pulled, {} with sound; the last service took {} us",
            self.periods,
            self.audible,
            self.last_service_ns / 1000
        );
    }

    /// Give the synthesiser back.
    pub fn close(&self) {
        self.report();
        if self.client.close().is_err() {
            log::warn!("music: closing the MIDI synthesiser failed");
        }
    }
}

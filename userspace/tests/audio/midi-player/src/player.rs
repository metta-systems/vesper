//! The player's start-up and playback loop (see the crate documentation in
//! `main.rs`).

use {
    crate::heap,
    audio_client::audio_v1::{
        Bindings as AudioBindings, Client as AudioClient, Error as AudioError,
        Exception as AudioException,
    },
    audio_pwm_bcm2835::stream::{
        CHANNEL_SLOTS as AUDIO_SLOTS, FRAME_BYTES, channel_at as audio_channel_at,
    },
    driver_protocol::report_key,
    libdevice_driver::{DriverInit, driver_init},
    libio_channel::{Consumer, IoRec, NoEvent, Packet, Producer},
    liblog_client::Logger,
    libmeddle_runtime::PAGE_BYTES,
    libobject::{EventCountKey, NotificationKey, RawKey},
    log::LevelFilter,
    mailbox_client::mailbox_v1::{Bindings as MailboxBindings, Client as MailboxClient, Clock},
    midi_player::{LevelMeter, NoteList, SongInfo},
    midi_synth::{
        CHANNEL_SLOTS as SYNTH_SLOTS, Command, PERIOD_BYTES, PERIOD_FRAMES, RENDER_AREA_BYTES,
        SONG_FIRST, SONG_LAST, SONG_LOOPING, ServiceWindow, channel_at as synth_channel_at,
        volume_value,
    },
    midi_synth_client::midi_synth_v1::{
        Bindings as SynthBindings, CHANNEL_PAGES as SYNTH_PAGES, Client as SynthClient,
        Error as SynthError, Exception as SynthException,
    },
    wood_synth::{OUTPUT_RATE, Song},
};

libuser::entry!(main);

/// The player's log, through the log server.
static LOGGER: Logger = Logger::new("midi-player");

/// The music volume, 0 to 127: the engine's default, as Wood plays its songs
/// (the start hash `just test-midi` checks is for this volume).
const MUSIC_VOLUME: u8 = 64;

/// Whether the song loops (it does in Wood's levels).
const LOOPING: bool = true;

/// Buffers kept queued on `audio_v1`: 46 ms ahead of the driver's ring, as
/// Wood's playback Thread keeps.
const QUEUED_BUFFERS: u64 = 4;

/// Bytes of one audio buffer: one period.
const BUFFER_BYTES: u64 = PERIOD_FRAMES as u64 * FRAME_BYTES;

/// Periods between two summaries (about 3 s), as Wood's.
const SUMMARY_PERIODS: u64 = 256;

/// Periods played before closing: two summaries under QEMU, for good on the
/// board.
const PERIOD_LIMIT: Option<u64> = if cfg!(feature = "qemu") {
    Some(2 * SUMMARY_PERIODS)
} else {
    None
};

/// Give up with `reason`: under QEMU the panic fails the run.
#[cfg(feature = "qemu")]
fn give_up(_init: &DriverInit, reason: &str) -> ! {
    log::error!("{reason}; not playing");
    panic!("midi-player: {reason}");
}

/// Give up with `reason`: on the board the line reaches the UART and the
/// program parks.
#[cfg(not(feature = "qemu"))]
fn give_up(init: &DriverInit, reason: &str) -> ! {
    log::error!("{reason}; not playing");
    park(init)
}

fn park(init: &DriverInit) -> ! {
    let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
    }
}

/// Sleep for about `milliseconds`: await a count the program's own
/// `EventCount` never reaches.
fn sleep_ms(init: &DriverInit, milliseconds: u64) {
    let _timed_out = EventCountKey::from_key(RawKey::from_wire(init.event_count))
        .await_ge(u64::MAX, milliseconds * 1_000_000);
}

/// Log the ARM and core clocks the firmware runs at: the synthesiser's
/// render time per period depends on them.
fn log_clocks(init: &DriverInit) {
    let mailbox = MailboxClient::new(MailboxBindings::from_imports(init.imports()));
    let megahertz = |clock: Clock| {
        mailbox
            .get_clock_rate(clock)
            .map_or(0, |hertz| hertz / 1_000_000)
    };
    log::info!(
        "ARM clock {} MHz, core clock {} MHz (0: unknown)",
        megahertz(Clock::Arm),
        megahertz(Clock::Core)
    );
}

/// Log what the song is, as the synthesiser's sequencer will play it; `None`
/// if it does not parse (the synthesiser will refuse it too).
fn describe(midi: &[u8], rate_millihertz: u32) -> Option<SongInfo> {
    let song = match Song::parse(midi) {
        Ok(song) => song,
        Err(error) => {
            log::error!("song: {} bytes that do not parse: {error:?}", midi.len());
            return None;
        }
    };
    let info = SongInfo::of(&song, OUTPUT_RATE);
    let pass_ms = info.pass_frames() * 1_000_000 / u64::from(rate_millihertz.max(1));
    log::info!(
        "song: {} bytes, division {} ticks per quarter, tempo {} us per quarter ({} bpm), {} tempo events, {} events, {} notes, end tick {}: a pass of {} frames ({}.{:03} s)",
        midi.len(),
        info.division,
        info.tempo,
        info.beats_per_minute(),
        info.tempo_changes,
        info.events,
        info.notes,
        info.end_tick,
        info.pass_frames(),
        pass_ms / 1000,
        pass_ms % 1000
    );
    log::info!("song: first notes: {}", NoteList(&info.first_notes));
    Some(info)
}

/// The synthesiser, taken: the player's end of `midi_synth_v1`.
struct Synthesiser {
    client: SynthClient,
    submit: Producer<'static, SYNTH_SLOTS, NoEvent>,
    completions: Consumer<'static, SYNTH_SLOTS, EventCountKey>,
    /// The first data page: the render buffer, then the song area.
    data_base: u64,
    window: ServiceWindow,
    last_service_ns: u64,
    /// Render packets that came back with fewer frames than asked
    short_renders: u64,
}

impl Synthesiser {
    fn open(init: &DriverInit, audio_millihertz: u32) -> Result<Self, &'static str> {
        let client = SynthClient::new(SynthBindings::from_imports(init.imports()));
        let rate = client.rate().map_err(|error| match error {
            SynthError::NotGranted => "no MIDI synthesiser in the image",
            _ => "the MIDI synthesiser did not answer",
        })?;
        if rate != audio_millihertz {
            return Err("the MIDI synthesiser renders at another rate than the audio driver");
        }
        let area = client
            .channel()
            .ok_or("no io-channel to the MIDI synthesiser")?;
        if area.event == 0 || area.data.len != SYNTH_PAGES * PAGE_BYTES {
            return Err("the io-channel to the MIDI synthesiser is not as declared");
        }
        client.open().map_err(|error| match error {
            SynthError::Exception(SynthException::Busy) => {
                "the MIDI synthesiser is busy or has no bank"
            }
            _ => "opening the MIDI synthesiser failed",
        })?;
        // SAFETY: Kickstart mapped the channel's control page read/write for
        // good, shared with the synthesiser only.
        let channel = unsafe { synth_channel_at(area.control) };
        let (submit, completions) = channel.client(
            NoEvent,
            EventCountKey::from_key(RawKey::from_wire(area.event)),
        );
        log::info!("the MIDI synthesiser plays at {rate} mHz");
        Ok(Self {
            client,
            submit,
            completions,
            data_base: area.data.base,
            window: ServiceWindow::EMPTY,
            last_service_ns: 0,
            short_renders: 0,
        })
    }

    /// Put `packet`; `false` if the FIFO is full.
    fn put(&mut self, packet: &Packet) -> bool {
        if self.submit.put(packet).is_ok() {
            true
        } else {
            log::warn!("the io-channel to the synthesiser is full");
            false
        }
    }

    /// Have the synthesiser handle what was put and take every packet back;
    /// the render buffer's frames, if a render packet came back.
    fn service(&mut self) -> Option<u64> {
        let started = libuser::clock::now_ns();
        let handled = self.client.service().ok();
        if handled.is_none() {
            log::warn!("service failed");
        }
        self.last_service_ns = libuser::clock::now_ns().saturating_sub(started);
        self.window.record_call(self.last_service_ns, handled);
        let mut rendered = None;
        while let Ok(Some(back)) = self.completions.get() {
            if Command::of(back.value()) == Some(Command::Render) {
                rendered = back.recs().first().map(|record| record.len / FRAME_BYTES);
            }
        }
        rendered
    }

    /// A control packet of `value` with an empty record, handled at once.
    fn control(&mut self, value: u64) {
        let record = IoRec {
            base: self.data_base,
            len: 0,
        };
        if let Some(packet) = Packet::new(value, &[record])
            && self.put(&packet)
        {
            self.service();
        }
    }

    /// Send `midi` through the song area, a packet at a time; the packets
    /// the synthesiser handled.
    fn send_song(&mut self, midi: &[u8], looping: bool) -> u64 {
        let area = self.data_base + RENDER_AREA_BYTES;
        let chunk = usize::try_from(SYNTH_PAGES * PAGE_BYTES - RENDER_AREA_BYTES).unwrap_or(4096);
        let count = midi.len().div_ceil(chunk);
        let before = self.window.handled;
        for (index, bytes) in midi.chunks(chunk).enumerate() {
            let target = usize::try_from(area).unwrap_or(usize::MAX);
            // SAFETY: the song area lies in the data pages, mapped read/write
            // for good; the synthesiser reads it only while it handles this
            // packet, during the `service` below.
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
        log::info!(
            "song of {} bytes sent in {count} packets, looping {looping}",
            midi.len()
        );
        self.window.handled - before
    }

    /// Render the next period into `frames`, silence where the synthesiser
    /// gave none; whether it had sound.
    fn render(&mut self, frames: &mut [[i16; 2]]) -> bool {
        let render = IoRec {
            base: self.data_base,
            len: PERIOD_BYTES.min(frames.len() as u64 * FRAME_BYTES),
        };
        let rendered = Packet::new(Command::Render.value(0, 0), &[render])
            .filter(|packet| self.put(packet))
            .and_then(|_packet| self.service())
            .unwrap_or(0);
        let rendered = usize::try_from(rendered).unwrap_or(0).min(frames.len());
        if rendered < frames.len() {
            self.short_renders += 1;
        }
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
        let audible = frames.iter().any(|&frame| frame != [0, 0]);
        self.window.record_period(audible);
        audible
    }
}

/// The open output: the player's end of `audio_v1`.
struct Output {
    client: AudioClient,
    submit: Producer<'static, AUDIO_SLOTS, NoEvent>,
    completions: Consumer<'static, AUDIO_SLOTS, EventCountKey>,
    /// The first data page
    data_base: u64,
    rate_millihertz: u32,
}

impl Output {
    /// Ask the driver for its rate and take the io-channel (the output opens
    /// once the first buffers are queued).
    fn new(init: &DriverInit) -> Result<Self, &'static str> {
        let client = AudioClient::new(AudioBindings::from_imports(init.imports()));
        let rate_millihertz = client.rate().map_err(|error| match error {
            AudioError::NotGranted => "no audio driver in the image (is the PWM node enabled?)",
            _ => "the audio driver did not answer",
        })?;
        let area = client
            .channel()
            .ok_or("no io-channel to the audio driver")?;
        if area.event == 0 || area.data.len < QUEUED_BUFFERS * BUFFER_BYTES {
            return Err("the io-channel to the audio driver is too small");
        }
        // SAFETY: Kickstart mapped the channel's control page read/write for
        // good, shared with the driver only.
        let channel = unsafe { audio_channel_at(area.control) };
        let (submit, completions) = channel.client(
            NoEvent,
            EventCountKey::from_key(RawKey::from_wire(area.event)),
        );
        Ok(Self {
            client,
            submit,
            completions,
            data_base: area.data.base,
            rate_millihertz,
        })
    }

    const fn record(&self, index: u64) -> IoRec {
        IoRec {
            base: self.data_base + index * BUFFER_BYTES,
            len: BUFFER_BYTES,
        }
    }

    /// The frames of buffer `index`.
    fn frames(&mut self, index: u64) -> &mut [[i16; 2]] {
        let address = usize::try_from(self.record(index).base).unwrap_or(usize::MAX);
        // SAFETY: `new` checked the data pages hold every buffer; they are
        // mapped read/write for good, and the driver reads a buffer only
        // while it holds the packet naming it, which the player puts back
        // after it filled the buffer. A frame is two little-endian signed
        // 16-bit samples, left then right, as `[i16; 2]` lies in memory.
        unsafe {
            core::slice::from_raw_parts_mut(
                core::ptr::with_exposed_provenance_mut::<[i16; 2]>(address),
                PERIOD_FRAMES,
            )
        }
    }
}

/// The playback loop's state: the two channels and what the log reports.
struct Player {
    synth: Synthesiser,
    output: Output,
    song: Option<SongInfo>,
    /// Periods pulled since the song started, and those with sound
    periods: u64,
    audible: u64,
    level: LevelMeter,
    /// The longest refill (render, copy, queue) since the last summary
    refill_max_ns: u64,
    /// The driver's underrun count at the last summary
    summary_underruns: u32,
}

impl Player {
    /// Pull the next period into buffer `index` and queue it.
    fn refill(&mut self, index: u64) {
        let started = libuser::clock::now_ns();
        let record = self.output.record(index);
        let frames = self.output.frames(index);
        let audible = self.synth.render(frames);
        self.level.add(frames);
        if let Some(packet) = Packet::new(index, &[record])
            && self.output.submit.put(&packet).is_err()
        {
            log::warn!("the io-channel to the audio driver is full");
        }
        self.refill_max_ns = self
            .refill_max_ns
            .max(libuser::clock::now_ns().saturating_sub(started));
        self.periods += 1;
        if audible {
            self.audible += 1;
            if self.audible == 1 {
                log::info!("first sound in period {}", self.periods);
            }
        }
        if self.periods.is_power_of_two() {
            self.report();
        }
        if self.periods.is_multiple_of(SUMMARY_PERIODS) {
            self.summary();
        }
    }

    /// Log the periods pulled and the last `service` time.
    fn report(&self) {
        log::info!(
            "music: {} periods pulled, {} with sound; the last service took {} us",
            self.periods,
            self.audible,
            self.synth.last_service_ns / 1000
        );
    }

    /// Wood's summary line, then the level, the voices and the position.
    /// Never waits on the log.
    fn summary(&mut self) {
        let underruns = self.output.client.underruns().unwrap_or(u32::MAX);
        let new_underruns = underruns.wrapping_sub(self.summary_underruns);
        self.summary_underruns = underruns;
        let refill_max_us = core::mem::take(&mut self.refill_max_ns) / 1000;
        let period_us = PERIOD_FRAMES as u64 * 1_000_000_000
            / u64::from(self.output.rate_millihertz.max(1));
        let playing = self
            .synth
            .client
            .playing()
            .is_ok_and(|playing| playing != 0);
        let voices = self.synth.client.voices().unwrap_or(u32::MAX);
        let window = core::mem::replace(&mut self.synth.window, ServiceWindow::EMPTY);
        log::info!(
            "music: summary of {} periods: service min/avg/max {}/{}/{} us over {} calls ({} failed), {} packets handled, {} periods with sound, playing {playing}; refill max {refill_max_us} us of a {period_us} us period; underruns {underruns} (+{new_underruns})",
            window.periods,
            window.shortest_ns() / 1000,
            window.average_ns() / 1000,
            window.max_ns / 1000,
            window.calls,
            window.failed,
            window.handled,
            window.audible,
        );
        let level = self.level.take();
        log::info!(
            "music: level peak {} rms {} of 32767; voices peak {voices} of 32; {} short renders",
            level.peak(),
            level.rms(),
            core::mem::take(&mut self.synth.short_renders)
        );
        let frames = self.periods * PERIOD_FRAMES as u64;
        if let Some(song) = &self.song {
            let position = song.position(frames);
            log::info!(
                "position: pass {}, tick {} of {}, frame {} of {}; {frames} frames since the song started",
                position.pass,
                position.tick,
                song.end_tick,
                position.frame,
                song.pass_frames()
            );
        } else {
            log::info!("position: {frames} frames since the song started");
        }
    }

    /// Keep the queue full, until the period limit under QEMU; then close
    /// the output after the last packet and take every packet back.
    fn play(&mut self) {
        // Queue the first buffers before opening: the driver starts on a
        // full queue, as audio-test's tone does.
        for index in 0..QUEUED_BUFFERS {
            self.refill(index);
        }
        if let Err(error) = self.output.client.open() {
            log::error!(
                "{}",
                match error {
                    AudioError::Exception(AudioException::Busy) =>
                        "another client has the audio output",
                    _ => "opening the audio output failed",
                }
            );
            return;
        }
        log::info!(
            "audio: open at {}.{:03} Hz, {QUEUED_BUFFERS} buffers of {PERIOD_FRAMES} frames queued",
            self.output.rate_millihertz / 1000,
            self.output.rate_millihertz % 1000
        );
        let mut outstanding = QUEUED_BUFFERS;
        loop {
            let Ok(back) = self.output.completions.wait() else {
                log::warn!("waiting for a buffer failed; stopping");
                break;
            };
            outstanding -= 1;
            if PERIOD_LIMIT.is_some_and(|limit| self.periods >= limit) {
                break;
            }
            self.refill(back.value());
            outstanding += 1;
        }
        if self.output.client.close().is_err() {
            log::warn!("closing the audio output failed");
        }
        // `close` returns once every packet is back.
        while outstanding > 0 && matches!(self.output.completions.get(), Ok(Some(_))) {
            outstanding -= 1;
        }
        if self.synth.client.close().is_err() {
            log::warn!("closing the MIDI synthesiser failed");
        }
        log::info!(
            "closed after {} periods, {} with sound, {} underruns",
            self.periods,
            self.audible,
            self.output.client.underruns().unwrap_or(u32::MAX)
        );
    }
}

fn main(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    LOGGER.install(LevelFilter::Info);
    let slot = u32::try_from(init.first_free_slot).unwrap_or(u32::MAX);
    if heap::carve(init, slot).is_err() {
        give_up(init, "carving the heap failed");
    }
    let Some(midi) = init.resource(0) else {
        give_up(init, "no song resource");
    };
    log_clocks(init);
    let output = Output::new(init).unwrap_or_else(|reason| give_up(init, reason));
    let song = describe(midi, output.rate_millihertz);
    let mut synth = Synthesiser::open(init, output.rate_millihertz)
        .unwrap_or_else(|reason| give_up(init, reason));
    synth.control(volume_value(MUSIC_VOLUME, 0));
    let handled = synth.send_song(midi, LOOPING);
    let playing = synth.client.playing().is_ok_and(|playing| playing != 0);
    log::info!(
        "the synthesiser handled {handled} song packets; playing {playing} at volume {MUSIC_VOLUME}"
    );
    if !playing {
        give_up(init, "the synthesiser does not play the song");
    }
    let mut player = Player {
        synth,
        output,
        song,
        periods: 0,
        audible: 0,
        level: LevelMeter::EMPTY,
        refill_max_ns: 0,
        summary_underruns: 0,
    };
    player.play();

    // Under QEMU: give the log server a moment to write the last lines to
    // the UART, then report to midi-test, which ends the run.
    sleep_ms(init, 200);
    NotificationKey::from_key(report_key(init.table))
        .signal(0)
        .unwrap_or_else(|error| panic!("midi-player: report failed: {:?}", error.code()));
    park(init)
}

//! Wood's sound: the engine's voices mixed by Room's no_std mixer
//! (`room::engine::sound::mixer`) on a playback Thread of Wood's own, on top
//! of the music the MIDI synthesiser component renders for each period
//! ([`music`](crate::music)), and streamed to the PWM audio driver through
//! `audio_v1` (decisions D5 and D10 of `doc/agentic/audio-design.md`).
//!
//! The two Threads share [`VOICES`] and nothing else of the engine: the game
//! Thread, through Wood's [`Platform`](room::engine::platform::Platform)
//! sound methods, only stores and loads its atomics, and never waits on
//! audio; the playback Thread mixes them into the io-channel's buffers.
//!
//! The playback Thread ([`playback`], declared with `driver_threads!` at
//! high priority, so the root dispatcher runs it as soon as a buffer comes
//! back, ahead of the engine, the compositor and the lava lamp) is the only
//! one calling `audio_v1`. Kickstart starts it at boot; it waits on Wood's
//! `EventCount` until the engine's `sound_init` releases it ([`RELEASED`]),
//! then asks the driver for its rate, takes the MIDI synthesiser, queues the
//! first [`QUEUED_BUFFERS`] buffers and only then opens the output, so the
//! driver starts on a full queue (as midi-test's player does). It answers
//! ([`ANSWERED`]) with [`STATE`]: playing, or silent when the image has no
//! audio driver or another client has the output (logged once). While it
//! plays it keeps [`QUEUED_BUFFERS`] buffers of [`BUFFER_FRAMES`] frames
//! queued: it waits for a packet to come back on the channel (a buffer
//! free), mixes the next buffer into it and puts it back. At
//! `sound_shutdown` it stops refilling, closes the output right after the
//! last packet (`close` plays out what is queued), and answers [`CLOSED`].
//!
//! Wood's `EventCount` doubles as the engine's sleep timer: a sleep awaits a
//! count it never reaches, so these advances do not cut sleeps short.
//!
//! The Thread runs on the 32 KiB stack Kickstart gives every Thread of a
//! program: the mixer's state is a few hundred bytes, and it mixes straight
//! into the channel's data pages.

use {
    crate::music::{COMMANDS, Music, PLAYING},
    audio_client::audio_v1::{Bindings, Client, Error, Exception},
    audio_pwm_bcm2835::stream::{CHANNEL_SLOTS, FRAME_BYTES, channel_at},
    core::sync::atomic::{AtomicBool, AtomicU8, Ordering},
    libdevice_driver::driver_init,
    libio_channel::{Consumer, IoRec, NoEvent, Packet, Producer},
    libobject::{EventCountKey, NotificationKey, RawKey},
    room::engine::sound::mixer::{Mixer, SharedVoices},
};

/// The voices the game Thread changes and the playback Thread mixes.
pub static VOICES: SharedVoices = SharedVoices::new();

/// Where the playback Thread is: one of the `STATE_*` values.
static STATE: AtomicU8 = AtomicU8::new(STATE_WAITING);
/// Not released yet, or not answered.
const STATE_WAITING: u8 = 0;
/// The output is open and the voices play.
const STATE_PLAYING: u8 = 1;
/// No output, or closed: the engine runs silent.
const STATE_SILENT: u8 = 2;

/// Set by `sound_shutdown`: stop refilling and close.
static QUIT: AtomicBool = AtomicBool::new(false);

/// Counts of Wood's `EventCount` the hand-over between the Threads waits
/// for: the game Thread releases the playback Thread, which answers once it
/// opened the output (or found none), and once it closed it.
const RELEASED: u64 = 1;
const ANSWERED: u64 = 2;
const CLOSED: u64 = 3;

/// How long `sound_init` waits for the answer, and `sound_shutdown` for the
/// close (which plays out the queue, about 50 ms).
const ANSWER_TIMEOUT_NS: u64 = 2_000_000_000;
const CLOSE_TIMEOUT_NS: u64 = 1_000_000_000;

/// Frames of one buffer: one period of the driver's ring, 11.6 ms.
const BUFFER_FRAMES: usize = 512;
/// Bytes of one buffer.
const BUFFER_BYTES: u64 = BUFFER_FRAMES as u64 * FRAME_BYTES;
/// Buffers kept queued: 93 ms of sound ahead of the driver's ring, which
/// holds 46 ms more. That is the slack the playback Thread has when it is
/// late to refill; it covers several 10 ms dispatcher slices of the other
/// Threads with margin. The cost is latency: a sound starts reaching the
/// jack within about 150 ms of the engine starting it. The eight buffers
/// fill the channel's four data pages (`channel 4;` in `audio_v1`).
const QUEUED_BUFFERS: u64 = 8;

/// Release the playback Thread and wait for it to open the output; `true`
/// when it plays. Game Thread, from `sound_init`.
pub fn start(hand_over: &EventCountKey) -> bool {
    if hand_over.advance(1).is_err() {
        log::warn!("audio: releasing the playback Thread failed");
        return false;
    }
    let _answered = hand_over.await_ge(ANSWERED, ANSWER_TIMEOUT_NS);
    is_playing()
}

/// Whether the output is open and the voices play.
pub fn is_playing() -> bool {
    STATE.load(Ordering::Acquire) == STATE_PLAYING
}

/// Have the playback Thread close the output after what it queued, and wait
/// for it. Game Thread, from `sound_shutdown`.
pub fn shut_down(hand_over: &EventCountKey) {
    if !is_playing() || QUIT.swap(true, Ordering::AcqRel) {
        return;
    }
    if hand_over.await_ge(CLOSED, CLOSE_TIMEOUT_NS).is_err() {
        log::warn!("audio: the output did not close in time");
    }
}

libdevice_driver::driver_threads!(playback => libdevice_driver::Priority::HIGH);

/// Wood's playback Thread: once `sound_init` releases it, open the output
/// and keep it fed with the mixed voices until `sound_shutdown`.
fn playback(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    let hand_over = EventCountKey::from_key(RawKey::from_wire(init.event_count));
    if hand_over
        .await_ge(RELEASED, EventCountKey::WAIT_INFINITE)
        .is_ok()
    {
        let client = Client::new(Bindings::from_imports(init.imports()));
        // The music first, so that the first buffers carry it.
        match Output::new(&client).and_then(|mut output| {
            output.music = Music::open(init, output.rate_millihertz)
                .inspect_err(|reason| log::warn!("music: {reason}; music stays silent"))
                .ok();
            output.start(&client).map(|()| output)
        }) {
            Ok(mut output) => {
                STATE.store(STATE_PLAYING, Ordering::Release);
                answer(&hand_over);
                output.play(&client);
                STATE.store(STATE_SILENT, Ordering::Release);
                answer(&hand_over);
            }
            Err(reason) => {
                log::warn!("audio: {reason}; running silent");
                STATE.store(STATE_SILENT, Ordering::Release);
                answer(&hand_over);
            }
        }
    }
    let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
    }
}

/// Tell the game Thread the playback Thread moved on.
fn answer(hand_over: &EventCountKey) {
    if hand_over.advance(1).is_err() {
        log::warn!("audio: answering the game Thread failed");
    }
}

type Submit<'channel> = Producer<'channel, CHANNEL_SLOTS, NoEvent>;
type Completions<'channel> = Consumer<'channel, CHANNEL_SLOTS, EventCountKey>;

/// The io-channel's data pages, which hold the buffers.
struct DataPages {
    /// The first data page
    base: u64,
}

impl DataPages {
    /// The record of buffer `index`.
    const fn record(&self, index: u64) -> IoRec {
        IoRec {
            base: self.base + index * BUFFER_BYTES,
            len: BUFFER_BYTES,
        }
    }

    /// The frames of buffer `index`.
    fn frames(&mut self, index: u64) -> &mut [[i16; 2]] {
        let address = usize::try_from(self.record(index).base).unwrap_or(usize::MAX);
        // SAFETY: `Output::open` checked the data pages hold every buffer;
        // they are mapped read/write for good, and the driver reads a buffer
        // only while it holds the packet naming it, which the playback
        // Thread puts back after it filled the buffer. A frame is two
        // little-endian signed 16-bit samples, left then right, as `[i16; 2]`
        // lies in memory.
        unsafe {
            core::slice::from_raw_parts_mut(
                core::ptr::with_exposed_provenance_mut::<[i16; 2]>(address),
                BUFFER_FRAMES,
            )
        }
    }
}

/// The open output: Wood's end of the io-channel and the mixer feeding it.
struct Output {
    submit: Submit<'static>,
    completions: Completions<'static>,
    pages: DataPages,
    mixer: Mixer<'static>,
    /// The output's rate in millihertz
    rate_millihertz: u32,
    /// The MIDI synthesiser, when the image has one
    music: Option<Music>,
    /// Buffers mixed, and those of them with a voice playing
    mixed: u64,
    audible: u64,
    /// The longest refill (music and mixing) since the last summary
    refill_max_ns: u64,
    /// The driver's underrun count at the last summary
    summary_underruns: u32,
}

/// Buffers between two summary lines (about 3 s): the music's `service`
/// times, what the synthesiser handled, the longest refill and the
/// underruns. One line per summary keeps the log far below even 115 200 baud.
const SUMMARY_BUFFERS: u64 = 256;

impl Output {
    /// Ask the driver for its rate and take the io-channel (the output opens
    /// once the first buffers are queued, [`start`](Self::start)); why not,
    /// if it cannot be.
    fn new(client: &Client) -> Result<Self, &'static str> {
        let rate_millihertz = client.rate().map_err(|error| match error {
            Error::NotGranted => "no audio driver in the image",
            _ => "the audio driver did not answer",
        })?;
        let area = client
            .channel()
            .ok_or("no io-channel to the audio driver")?;
        if area.event == 0 || area.data.len < QUEUED_BUFFERS * BUFFER_BYTES {
            return Err("the io-channel to the audio driver is too small");
        }
        // SAFETY: Kickstart mapped the channel's control page read/write
        // for good, shared with the driver only.
        let channel = unsafe { channel_at(area.control) };
        let (submit, completions) = channel.client(
            NoEvent,
            EventCountKey::from_key(RawKey::from_wire(area.event)),
        );
        Ok(Self {
            submit,
            completions,
            pages: DataPages {
                base: area.data.base,
            },
            mixer: Mixer::new(rate_millihertz),
            rate_millihertz,
            music: None,
            mixed: 0,
            audible: 0,
            refill_max_ns: 0,
            summary_underruns: 0,
        })
    }

    /// Queue the first [`QUEUED_BUFFERS`] buffers, then open the output: the
    /// driver starts on a full queue, so the stream does not begin counting
    /// underruns with one buffer queued while the engine starts up. On
    /// failure the MIDI synthesiser is let go.
    fn start(&mut self, client: &Client) -> Result<(), &'static str> {
        for index in 0..QUEUED_BUFFERS {
            self.refill(client, index);
        }
        if let Err(error) = client.open() {
            if let Some(music) = self.music.take() {
                music.close();
            }
            return Err(match error {
                Error::Exception(Exception::Busy) => "another client has the audio output",
                _ => "opening the audio output failed",
            });
        }
        log::info!(
            "audio: open at {}.{:03} Hz, {QUEUED_BUFFERS} buffers of {BUFFER_FRAMES} frames queued",
            self.rate_millihertz / 1000,
            self.rate_millihertz % 1000
        );
        Ok(())
    }

    /// Log what the last [`SUMMARY_BUFFERS`] buffers took: the line to read
    /// when the music sounds wrong on the board. Never waits on the log.
    fn summary(&mut self, client: &Client) {
        let underruns = client.underruns().unwrap_or(u32::MAX);
        let new_underruns = underruns.wrapping_sub(self.summary_underruns);
        self.summary_underruns = underruns;
        let refill_max_us = core::mem::take(&mut self.refill_max_ns) / 1000;
        let period_us =
            BUFFER_FRAMES as u64 * 1_000_000_000 / u64::from(self.rate_millihertz.max(1));
        match &mut self.music {
            Some(music) => {
                let window = music.take_window();
                log::info!(
                    "music: summary of {} periods: service min/avg/max {}/{}/{} us over {} calls ({} failed), {} packets handled, {} periods with sound, playing {}; refill max {refill_max_us} us of a {period_us} us period; underruns {underruns} (+{new_underruns})",
                    window.periods,
                    window.shortest_ns() / 1000,
                    window.average_ns() / 1000,
                    window.max_ns / 1000,
                    window.calls,
                    window.failed,
                    window.handled,
                    window.audible,
                    PLAYING.load(Ordering::Acquire)
                );
            }
            None => log::info!(
                "audio: summary of {SUMMARY_BUFFERS} buffers: no music; refill max {refill_max_us} us of a {period_us} us period; underruns {underruns} (+{new_underruns})"
            ),
        }
    }

    /// Pull the music into buffer `index`, mix the voices on top and queue
    /// it.
    fn refill(&mut self, client: &Client, index: u64) {
        let started = libuser::clock::now_ns();
        let frames = self.pages.frames(index);
        if let Some(music) = &mut self.music {
            music.fill(frames);
        } else {
            frames.fill([0, 0]);
            // Nobody plays the songs: drop what the engine asked for.
            drop(COMMANDS.take());
            PLAYING.store(false, Ordering::Release);
        }
        let playing = self.mixer.mix_shared_onto(&VOICES, frames);
        let record = self.pages.record(index);
        let packet = Packet::new(index, &[record]).expect("one record");
        if self.submit.put(&packet).is_err() {
            log::warn!("audio: the io-channel is full");
        }
        self.refill_max_ns = self
            .refill_max_ns
            .max(libuser::clock::now_ns().saturating_sub(started));
        self.mixed += 1;
        if self.mixed.is_multiple_of(SUMMARY_BUFFERS) {
            self.summary(client);
        }
        if playing != 0 {
            self.audible += 1;
            if self.audible.is_power_of_two() {
                self.report(client);
            }
        } else if self.mixed.is_power_of_two() && self.mixed >= 1024 {
            self.report(client);
        }
    }

    /// Log the buffers mixed and the driver's underrun count.
    fn report(&self, client: &Client) {
        log::info!(
            "audio: {} buffers mixed, {} with sound, {} underruns",
            self.mixed,
            self.audible,
            client.underruns().unwrap_or(u32::MAX)
        );
    }

    /// Keep the queue [`start`](Self::start) filled full until
    /// `sound_shutdown`, then close the output after the last packet and
    /// take back every packet.
    fn play(&mut self, client: &Client) {
        let mut outstanding = QUEUED_BUFFERS;
        loop {
            let Ok(back) = self.completions.wait() else {
                log::warn!("audio: waiting for a buffer failed; stopping");
                break;
            };
            outstanding -= 1;
            if QUIT.load(Ordering::Acquire) {
                break;
            }
            self.refill(client, back.value());
            outstanding += 1;
        }
        if client.close().is_err() {
            log::warn!("audio: closing the output failed");
        }
        if let Some(music) = self.music.take() {
            music.close();
        }
        // `close` returns once every packet is back.
        while outstanding > 0 {
            if !matches!(self.completions.get(), Ok(Some(_))) {
                break;
            }
            outstanding -= 1;
        }
        self.report(client);
        log::info!("audio: closed");
    }
}

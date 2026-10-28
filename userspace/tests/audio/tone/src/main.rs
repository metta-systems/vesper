#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! audio-test's program: a client of the PWM audio driver through the
//! `audio_v1` import Kickstart wired along the PWM node's compatible string.
//! It streams signed 16-bit stereo buffers from its io-channel's data pages.
//!
//! On the board it plays a 440 Hz tone on the left side of the jack for
//! 3 s, then the right, then both, then both for a minute, opening and closing
//! the output around each; the driver traces every stream on the UART when it
//! closes (underruns, late periods, the measured rate).
//!
//! Under QEMU, where the driver plays each period of its ring by a DMA copy
//! into a capture buffer and checks the words against the capture test's
//! pattern (`audio_pwm_bcm2835::stream::test_pattern`), it runs the capture
//! test: it queues six buffers of the pattern and a refused one, opens,
//! gets every packet back in order, stops sending until the driver counts
//! underruns, and closes; then it queues four more and closes at once, which
//! plays them out: they come back whole, with no underrun. It also checks the
//! refusals of `open` and `close`, then reports on the key audio-test
//! installed (`driver_protocol::REPORT_SLOT`).

use {
    audio_client::audio_v1::{Bindings, Client},
    audio_pwm_bcm2835::stream::{AudioChannel, FRAME_BYTES, channel_at, encode_frame},
    driver_protocol::report_key,
    libdevice_driver::{DriverInit, driver_init},
    libio_channel::{Consumer, IoRec, NoEvent, Packet, Producer},
    libobject::{EventCountKey, NotificationKey, RawKey},
};

libuser::entry!(main);

/// Frames of one buffer: one period of the driver's ring.
const BUFFER_FRAMES: u64 = 512;
/// Bytes of one buffer.
const BUFFER_BYTES: u64 = BUFFER_FRAMES * FRAME_BYTES;
/// The rate `audio_v1.rate` reports: 100 MHz / 2268, in millihertz.
const RATE_MILLIHERTZ: u32 = 44_091_710;

type Submit<'channel> = Producer<'channel, { audio_pwm_bcm2835::stream::CHANNEL_SLOTS }, NoEvent>;
type Completions<'channel> =
    Consumer<'channel, { audio_pwm_bcm2835::stream::CHANNEL_SLOTS }, EventCountKey>;

/// The program's end of the io-channel.
struct Output<'channel> {
    client: Client,
    submit: Submit<'channel>,
    completions: Completions<'channel>,
    /// The data pages
    base: u64,
    buffers: u64,
}

impl<'channel> Output<'channel> {
    fn new(channel: &'channel AudioChannel, client: Client) -> Self {
        let area = client
            .channel()
            .expect("the program shares an io-channel with the driver");
        assert_ne!(area.event, 0, "the channel has an EventCount");
        let (submit, completions) = channel.client(
            NoEvent,
            EventCountKey::from_key(RawKey::from_wire(area.event)),
        );
        Self {
            client,
            submit,
            completions,
            base: area.data.base,
            buffers: area.data.len / BUFFER_BYTES,
        }
    }

    /// Buffer `index` of the data pages, `frames` long.
    fn buffer(&self, index: u64, frames: u64) -> IoRec {
        assert!(index < self.buffers && frames <= BUFFER_FRAMES);
        IoRec {
            base: self.base + index * BUFFER_BYTES,
            len: frames * FRAME_BYTES,
        }
    }

    /// Write `next`'s frames into `rec`.
    fn write(rec: IoRec, mut next: impl FnMut() -> [i16; 2]) {
        for offset in (0..rec.len).step_by(4) {
            // SAFETY: `buffer` keeps every record inside the data pages,
            // which are mapped read/write for good; the driver reads a
            // buffer only while it holds the packet naming it.
            unsafe {
                core::ptr::write_volatile(
                    core::ptr::with_exposed_provenance_mut::<u32>(
                        usize::try_from(rec.base + offset).expect("a 64-bit address"),
                    ),
                    encode_frame(next()),
                );
            }
        }
    }

    /// Put a packet of `recs` tagged `value`.
    fn put(&mut self, value: u64, recs: &[IoRec]) {
        let packet = Packet::new(value, recs).expect("one to eight records");
        self.submit.put(&packet).expect("the FIFO has room");
    }

    /// The next packet back from the driver.
    fn back(&mut self) -> Packet {
        self.completions
            .wait()
            .unwrap_or_else(|_| panic!("audio-test: waiting for a packet failed"))
    }
}

/// Sleep for about `milliseconds`: await a count the program's own
/// `EventCount` never reaches.
fn sleep_ms(init: &DriverInit, milliseconds: u64) {
    let _timed_out = EventCountKey::from_key(RawKey::from_wire(init.event_count))
        .await_ge(u64::MAX, milliseconds * 1_000_000);
}

fn main(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    let client = Client::new(Bindings::from_imports(init.imports()));
    assert_eq!(client.rate(), Ok(RATE_MILLIHERTZ), "100 MHz / 2268");
    let control = client
        .channel()
        .expect("the program shares an io-channel with the driver")
        .control;
    // SAFETY: Kickstart mapped the channel's control page read/write for good.
    let channel = unsafe { channel_at(control) };
    let mut output = Output::new(channel, client);

    #[cfg(feature = "qemu")]
    capture::run(init, &mut output);
    #[cfg(not(feature = "qemu"))]
    tone::run(init, &mut output);

    NotificationKey::from_key(report_key(init.table))
        .signal(0)
        .unwrap_or_else(|error| panic!("audio-test: report failed: {:?}", error.code()));
    let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
    }
}

/// The capture test, under QEMU.
#[cfg(feature = "qemu")]
mod capture {
    use {
        super::{BUFFER_FRAMES, Output, sleep_ms},
        audio_client::audio_v1::{Error, Exception},
        audio_pwm_bcm2835::stream::{FRAME_BYTES, test_pattern},
        libdevice_driver::DriverInit,
        libio_channel::IoRec,
        libuser::semihosting as semi,
    };

    /// Buffers of pattern the first stream plays.
    const FIRST_BUFFERS: u64 = 6;
    /// Buffers the second stream queues before it closes at once.
    const SECOND_BUFFERS: u64 = 4;
    /// Silent periods to wait for once the pattern ran out.
    const UNDERRUNS: u32 = 2;
    /// How long to wait for them.
    const UNDERRUN_TIMEOUT_MS: u64 = 5_000;

    /// Fill buffer `index` with `frames` frames of the pattern from position
    /// `first`.
    fn pattern(output: &Output<'_>, index: u64, first: u64, frames: u64) -> IoRec {
        let rec = output.buffer(index, frames);
        let mut position = first;
        Output::write(rec, || {
            let frame = test_pattern(position);
            position += 1;
            frame
        });
        rec
    }

    pub fn run(init: &DriverInit, output: &mut Output<'_>) {
        assert_eq!(
            output.client.close(),
            Err(Error::Exception(Exception::NotOpen)),
            "nothing is open yet"
        );

        // The first stream: six buffers of the pattern, the third followed by
        // one the driver refuses (a ragged length), all queued before `open`.
        let mut sent = 0;
        for index in 0..FIRST_BUFFERS {
            let rec = pattern(output, index, index * BUFFER_FRAMES, BUFFER_FRAMES);
            output.put(index, &[rec]);
            sent += 1;
            if index == 2 {
                let ragged = IoRec {
                    len: 6,
                    ..output.buffer(FIRST_BUFFERS, 2)
                };
                output.put(100, &[ragged]);
                sent += 1;
            }
        }
        output.client.open().expect("the output opens");
        assert_eq!(
            output.client.open(),
            Err(Error::Exception(Exception::Busy)),
            "already open"
        );
        let mut order = [0_u64; 8];
        for slot in order.iter_mut().take(sent) {
            let packet = output.back();
            let expected = if packet.value() == 100 {
                0
            } else {
                BUFFER_FRAMES * FRAME_BYTES
            };
            assert_eq!(
                packet.bytes(),
                expected,
                "packet {} back with the bytes taken",
                packet.value()
            );
            *slot = packet.value();
        }
        assert_eq!(order[..sent], [0, 1, 2, 100, 3, 4, 5], "back in order");
        semi::println!(
            "audio-test: {sent} packets back in order, {} frames taken, the refused one empty",
            FIRST_BUFFERS * BUFFER_FRAMES
        );

        // Stop sending: the driver fills periods with silence and counts them.
        let mut waited = 0;
        let underruns = loop {
            let underruns = output.client.underruns().expect("underruns");
            if underruns >= UNDERRUNS || waited >= UNDERRUN_TIMEOUT_MS {
                break underruns;
            }
            sleep_ms(init, 10);
            waited += 10;
        };
        assert!(underruns >= UNDERRUNS, "{underruns} underruns counted");
        output.client.close().expect("the output closes");
        let after = output.client.underruns().expect("underruns");
        assert!(after >= underruns, "the count stays after close");
        assert_eq!(
            output.client.close(),
            Err(Error::Exception(Exception::NotOpen)),
            "closed already"
        );
        semi::println!("audio-test: the client stopped sending: {after} underruns counted, closed");

        // The second stream: four buffers queued, then closed at once. Close
        // plays out what was queued: every packet comes back whole, in order,
        // and the end of the stream is no underrun.
        for index in 0..SECOND_BUFFERS {
            let rec = pattern(output, index, index * BUFFER_FRAMES, BUFFER_FRAMES);
            output.put(10 + index, &[rec]);
        }
        output.client.open().expect("the output opens again");
        output.client.close().expect("and closes at once");
        let mut taken = 0;
        for index in 0..SECOND_BUFFERS {
            let packet = output.back();
            assert_eq!(packet.value(), 10 + index, "back in order");
            assert_eq!(packet.bytes(), BUFFER_FRAMES * FRAME_BYTES, "played whole");
            taken += packet.bytes() / FRAME_BYTES;
        }
        assert_eq!(
            output.client.underruns(),
            Ok(0),
            "the end of a stream is no underrun"
        );
        semi::println!(
            "audio-test: closed at once: {SECOND_BUFFERS} packets back in order, {taken} frames played out, no underruns"
        );
    }
}

/// The tone, on the board.
#[cfg(not(feature = "qemu"))]
mod tone {
    use {
        super::{BUFFER_FRAMES, Output, RATE_MILLIHERTZ, sleep_ms},
        audio_pwm_bcm2835::{Side, Tone, side_frame},
        libdevice_driver::DriverInit,
    };

    /// The tone's pitch.
    const TONE_HZ: u32 = 440;
    /// Each sample shifted right by this: a quarter of full scale, gentle on
    /// headphones (full scale is about 1 V peak to peak at the jack).
    const ATTENUATION: u32 = 2;
    /// What plays, in order, and for how many milliseconds: each side alone,
    /// then both, then both for the minute of the hardware check.
    const SEGMENTS: [(Side, u64); 4] = [
        (Side::Left, 3_000),
        (Side::Right, 3_000),
        (Side::Both, 3_000),
        (Side::Both, 60_000),
    ];
    /// Milliseconds of quiet between segments.
    const PAUSE_MS: u64 = 700;
    /// Buffers kept queued: about 70 ms ahead of the ring.
    const QUEUED: u64 = 6;

    /// Play `milliseconds` of the tone on `side`, keeping `QUEUED` buffers
    /// queued, and close after the last one: `close` plays out what is
    /// queued, so the end of the tone is no underrun.
    fn segment(output: &mut Output<'_>, side: Side, milliseconds: u64) {
        let mut tone = Tone::new(TONE_HZ, u64::from(RATE_MILLIHERTZ), ATTENUATION);
        let mut remaining = milliseconds * u64::from(RATE_MILLIHERTZ) / 1_000_000;
        let mut outstanding = 0;
        let mut next = |output: &mut Output<'_>, index: u64, remaining: &mut u64| {
            let frames = (*remaining).min(BUFFER_FRAMES);
            *remaining -= frames;
            let rec = output.buffer(index, frames);
            Output::write(rec, || side_frame(side, tone.next_sample()));
            output.put(index, &[rec]);
        };
        for index in 0..QUEUED.min(output.buffers) {
            if remaining == 0 {
                break;
            }
            next(output, index, &mut remaining);
            outstanding += 1;
        }
        output.client.open().expect("the output opens");
        while remaining > 0 {
            let back = output.back();
            next(output, back.value(), &mut remaining);
        }
        output.client.close().expect("the output closes");
        for _ in 0..outstanding {
            let _back = output.back();
        }
    }

    pub fn run(init: &DriverInit, output: &mut Output<'_>) {
        for (side, milliseconds) in SEGMENTS {
            segment(output, side, milliseconds);
            sleep_ms(init, PAUSE_MS);
        }
    }
}

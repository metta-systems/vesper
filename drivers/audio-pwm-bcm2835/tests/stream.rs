//! Host tests of the `audio_v1` stream: sample conversion, refilling periods
//! from a client's packets, underruns, closing, ramps and the ring's
//! bookkeeping.

#[cfg(test)]
mod tests {
    use {
        audio_pwm_bcm2835::{
            PERIOD_FRAMES, PERIODS, RANGE,
            stream::{
                AudioChannel, FRAME_BYTES, Fill, Queue, Refill, Ring, Stream, admits, decode_frame,
                encode_frame, expected_stream_period, fifo_frame, ramp_down, ramp_up, silent_frame,
                test_pattern,
            },
            word,
        },
        libio_channel::{DataArea, IoRec, Packet},
        std::collections::VecDeque,
    };

    /// Where the fake data pages start, and their size: four pages, as
    /// `audio_v1` declares.
    const BASE: u64 = 0x3a00_1000;
    const DATA: DataArea = DataArea {
        base: BASE,
        len: 4 * 4096,
    };

    /// A client's channel and data pages, from the driver's side.
    struct FakeQueue {
        memory: Vec<u8>,
        submitted: VecDeque<Packet>,
        completed: Vec<Packet>,
    }

    impl FakeQueue {
        fn new() -> Self {
            Self {
                memory: vec![0; usize::try_from(DATA.len).unwrap()],
                submitted: VecDeque::new(),
                completed: Vec::new(),
            }
        }

        /// Write `frames` at `address`, as the client does.
        fn write(&mut self, address: u64, frames: &[[i16; 2]]) {
            let offset = usize::try_from(address - BASE).unwrap();
            for (index, frame) in frames.iter().enumerate() {
                let at = offset + index * 4;
                self.memory[at..at + 4].copy_from_slice(&encode_frame(*frame).to_le_bytes());
            }
        }

        /// A buffer of `count` pattern frames from position `first`, at
        /// `address`.
        fn pattern(&mut self, address: u64, first: u64, count: u64) -> IoRec {
            let frames: Vec<_> = (first..first + count).map(test_pattern).collect();
            self.write(address, &frames);
            IoRec {
                base: address,
                len: count * FRAME_BYTES,
            }
        }

        fn submit(&mut self, value: u64, recs: &[IoRec]) {
            self.submitted.push_back(Packet::new(value, recs).unwrap());
        }
    }

    impl Queue for FakeQueue {
        fn next(&mut self) -> Option<Packet> {
            self.submitted.pop_front()
        }

        fn complete(&mut self, packet: &Packet) {
            self.completed.push(*packet);
        }

        fn read_frame(&mut self, address: u64) -> u32 {
            assert!(address.is_multiple_of(4), "frames are read aligned");
            let offset = usize::try_from(address - BASE).unwrap();
            u32::from_le_bytes(self.memory[offset..offset + 4].try_into().unwrap())
        }
    }

    fn period() -> Vec<[u32; 2]> {
        vec![[u32::MAX; 2]; PERIOD_FRAMES]
    }

    /// The FIFO words of pattern frames `first..first + count`.
    fn pattern_words(first: u64, count: u64) -> Vec<[u32; 2]> {
        (first..first + count)
            .map(|position| fifo_frame(test_pattern(position), RANGE))
            .collect()
    }

    #[test]
    fn samples_scale_to_the_range_right_side_first() {
        let silence = RANGE / 2;
        assert_eq!(fifo_frame([0, 0], RANGE), [silence, silence]);
        assert_eq!(silent_frame(RANGE), [silence, silence]);
        // Left loudest, right quietest: the right word comes first.
        assert_eq!(fifo_frame([i16::MAX, i16::MIN], RANGE), [0, RANGE - 1]);
        assert_eq!(fifo_frame([i16::MIN, i16::MAX], RANGE), [RANGE - 1, 0]);
        // Every sample lands inside the range: the extremes clip to 0 and
        // RANGE - 1, never RANGE.
        for sample in [i16::MIN, -1, 0, 1, i16::MAX] {
            let words = fifo_frame([sample, sample], RANGE);
            assert!(words.iter().all(|&value| value < RANGE), "{sample}");
            assert_eq!(words, [word(sample, RANGE); 2]);
        }
        // One step of the 16-bit input is 2268/65536 of a range step.
        assert_eq!(word(-32768 + 29, RANGE), 1);
        assert_eq!(word(-32768 + 28, RANGE), 0);
    }

    #[test]
    fn frames_are_little_endian_left_then_right() {
        // Left -32767 (0x8001), right 32767 (0x7fff), as the bytes lie.
        let raw = u32::from_le_bytes([0x01, 0x80, 0xff, 0x7f]);
        assert_eq!(decode_frame(raw), [-32767, 32767]);
        for frame in [[0, -1], [i16::MIN, i16::MAX], [1234, -4321]] {
            assert_eq!(decode_frame(encode_frame(frame)), frame);
        }
    }

    #[test]
    fn the_test_pattern_tells_sides_and_neighbours_apart() {
        let mut seen = vec![false; 65_536];
        for position in 0..65_536 {
            let [left, right] = test_pattern(position);
            assert_ne!(left, right);
            assert_ne!(test_pattern(position), test_pattern(position + 1));
            seen[usize::from(left.cast_unsigned())] = true;
        }
        assert!(seen.iter().all(|&hit| hit), "every left value once");
        let mut expected = period();
        expected_stream_period(&mut expected, 100, 10, RANGE);
        assert_eq!(expected[..10], pattern_words(100, 10)[..]);
        assert!(
            expected[10..]
                .iter()
                .all(|&frame| frame == silent_frame(RANGE))
        );
    }

    #[test]
    fn periods_take_packets_in_order_across_records() {
        let mut queue = FakeQueue::new();
        // Packet 1: 300 frames; packet 2: two records of 200 and 100 frames
        // (not adjacent); packet 3: 512 frames.
        let first = queue.pattern(BASE, 0, 300);
        let second_a = queue.pattern(BASE + 0x1000, 300, 200);
        let second_b = queue.pattern(BASE + 0x2000, 500, 100);
        let third = queue.pattern(BASE + 0x2800, 600, 512);
        queue.submit(1, &[first]);
        queue.submit(2, &[second_a, second_b]);
        queue.submit(3, &[third]);

        let mut stream = Stream::new(RANGE);
        let mut words = period();
        let fill = stream.fill(&mut words, &DATA, &mut queue);
        assert_eq!(
            fill,
            Fill {
                client_frames: 512,
                silent_frames: 0
            }
        );
        assert_eq!(words, pattern_words(0, 512));
        // Packet 1 is all in; packet 2 is 212 frames short.
        let values: Vec<_> = queue.completed.iter().map(Packet::value).collect();
        assert_eq!(values, [1]);
        assert_eq!(queue.completed[0].recs(), [first], "whole");

        let fill = stream.fill(&mut words, &DATA, &mut queue);
        assert_eq!(fill.client_frames, 512);
        assert_eq!(words, pattern_words(512, 512));
        let values: Vec<_> = queue.completed.iter().map(Packet::value).collect();
        assert_eq!(
            values,
            [1, 2],
            "packet 2 is back once its last frame went in"
        );
        assert_eq!(queue.completed[1].recs(), [second_a, second_b]);

        // 88 frames of packet 3 remain: then silence, an underrun.
        let fill = stream.fill(&mut words, &DATA, &mut queue);
        assert_eq!(
            fill,
            Fill {
                client_frames: 88,
                silent_frames: 424
            }
        );
        assert_eq!(words[..88], pattern_words(1024, 88)[..]);
        assert!(
            words[88..]
                .iter()
                .all(|&frame| frame == silent_frame(RANGE))
        );
        assert_eq!(queue.completed.len(), 3);
        assert_eq!(stream.underruns(), 1);
        assert_eq!(stream.frames(), 1112);
        assert_eq!(stream.last(), silent_frame(RANGE));
    }

    #[test]
    fn a_packet_ending_with_the_period_comes_back_with_it() {
        let mut queue = FakeQueue::new();
        let rec = queue.pattern(BASE, 0, PERIOD_FRAMES as u64);
        queue.submit(7, &[rec]);
        let mut stream = Stream::new(RANGE);
        let mut words = period();
        stream.fill(&mut words, &DATA, &mut queue);
        assert_eq!(queue.completed.len(), 1);
        assert_eq!(stream.underruns(), 0);
        assert_eq!(stream.last(), *pattern_words(511, 1).first().unwrap());
    }

    #[test]
    fn underruns_count_silent_periods_after_the_first_packet() {
        let mut queue = FakeQueue::new();
        let mut stream = Stream::new(RANGE);
        let mut words = period();
        // Open, nothing sent yet: silence, not an underrun.
        stream.fill(&mut words, &DATA, &mut queue);
        stream.fill(&mut words, &DATA, &mut queue);
        assert_eq!(stream.underruns(), 0);
        assert!(words.iter().all(|&frame| frame == silent_frame(RANGE)));
        // One full period, then the client stops: each silent period counts.
        let rec = queue.pattern(BASE, 0, PERIOD_FRAMES as u64);
        queue.submit(1, &[rec]);
        stream.fill(&mut words, &DATA, &mut queue);
        assert_eq!(stream.underruns(), 0);
        for expected in 1..=3 {
            let fill = stream.fill(&mut words, &DATA, &mut queue);
            assert_eq!(fill.silent_frames, PERIOD_FRAMES);
            assert_eq!(stream.underruns(), expected);
        }
        // The client catches up: no more.
        let rec = queue.pattern(BASE, 512, PERIOD_FRAMES as u64);
        queue.submit(2, &[rec]);
        stream.fill(&mut words, &DATA, &mut queue);
        assert_eq!(stream.underruns(), 3);
        assert_eq!(words, pattern_words(512, 512));
    }

    #[test]
    fn draining_plays_the_rest_without_underruns() {
        let mut queue = FakeQueue::new();
        let mut stream = Stream::new(RANGE);
        let mut words = period();
        let rec = queue.pattern(BASE, 0, 600);
        queue.submit(1, &[rec]);
        stream.fill(&mut words, &DATA, &mut queue);
        // The output closes: the last 88 frames play, then silence, which is
        // the end of the stream.
        let fill = stream.drain(&mut words, &DATA, &mut queue);
        assert_eq!(fill.client_frames, 88);
        assert_eq!(words[..88], pattern_words(512, 88)[..]);
        assert_eq!(queue.completed.len(), 1);
        let fill = stream.drain(&mut words, &DATA, &mut queue);
        assert_eq!(fill.client_frames, 0, "drained: time to ramp down");
        assert_eq!(stream.underruns(), 0);
        assert_eq!(stream.last(), silent_frame(RANGE));
    }

    #[test]
    fn refused_packets_come_back_empty_and_the_stream_goes_on() {
        let mut queue = FakeQueue::new();
        let good = queue.pattern(BASE, 0, 16);
        let outside = IoRec {
            base: DATA.base + DATA.len,
            len: 16,
        };
        let ragged = IoRec {
            base: BASE + 0x100,
            len: 6,
        };
        let unaligned = IoRec {
            base: BASE + 0x102,
            len: 8,
        };
        for rec in [outside, ragged, unaligned] {
            let packet = Packet::new(0, &[rec]).unwrap();
            assert!(!admits(&packet, &DATA), "{rec:?}");
        }
        assert!(admits(&Packet::new(0, &[good]).unwrap(), &DATA));
        queue.submit(1, &[outside]);
        queue.submit(2, &[good, ragged]);
        queue.submit(3, &[unaligned]);
        queue.submit(4, &[good]);
        let mut stream = Stream::new(RANGE);
        let mut words = period();
        let fill = stream.fill(&mut words, &DATA, &mut queue);
        assert_eq!(fill.client_frames, 16, "only packet 4 plays");
        assert_eq!(words[..16], pattern_words(0, 16)[..]);
        let back: Vec<_> = queue
            .completed
            .iter()
            .map(|packet| (packet.value(), packet.bytes()))
            .collect();
        assert_eq!(back, [(1, 0), (2, 0), (3, 0), (4, 64)], "in order");
    }

    #[test]
    fn closing_hands_back_what_was_not_taken() {
        let mut queue = FakeQueue::new();
        let first_a = queue.pattern(BASE, 0, 400);
        let first_b = queue.pattern(BASE + 0x1000, 400, 300);
        let second = queue.pattern(BASE + 0x2000, 700, 100);
        queue.submit(1, &[first_a, first_b]);
        queue.submit(2, &[second]);
        let mut stream = Stream::new(RANGE);
        let mut words = period();
        stream.fill(&mut words, &DATA, &mut queue);
        assert!(queue.completed.is_empty(), "packet 1 is 188 frames short");
        assert_eq!(stream.finish(&mut queue), 2);
        let first = queue.completed[0];
        assert_eq!(first.value(), 1);
        assert_eq!(first.recs()[0], first_a, "taken whole");
        assert_eq!(first.recs()[1].len, 112 * FRAME_BYTES, "112 frames taken");
        assert_eq!(first.bytes(), 512 * FRAME_BYTES);
        assert_eq!(
            (queue.completed[1].value(), queue.completed[1].bytes()),
            (2, 0)
        );
        // Nothing left: the next period is silent, and not an underrun of a
        // stream that has nothing queued.
        assert_eq!(stream.finish(&mut queue), 0);
    }

    #[test]
    fn ramps_rise_to_silence_and_fall_to_off() {
        let silence = RANGE / 2;
        let mut words = period();
        ramp_up(&mut words, RANGE);
        assert_eq!(words[0], [silence / 512; 2]);
        assert_eq!(*words.last().unwrap(), [silence, silence]);
        let rising = words.iter().zip(&words[1..]);
        assert!(
            rising
                .clone()
                .all(|(earlier, later)| earlier[0] <= later[0])
        );
        assert!(
            rising
                .clone()
                .all(|(earlier, _later)| earlier[0] == earlier[1])
        );
        let from = [2000, 300];
        ramp_down(&mut words, from);
        assert_eq!(words[0], [2000 * 511 / 512, 300 * 511 / 512]);
        assert_eq!(*words.last().unwrap(), [0, 0]);
        assert!(
            words
                .iter()
                .zip(&words[1..])
                .all(|(earlier, later)| earlier[0] >= later[0] && earlier[1] >= later[1])
        );
    }

    #[test]
    fn the_ring_refills_the_slots_the_dma_left() {
        let periods = PERIODS as u64;
        let mut ring = Ring::new(periods);
        assert_eq!(ring.finished(), 0);
        assert!(!ring.has_played(0));
        // One period done: its slot takes period 4.
        assert_eq!(
            ring.advance(1),
            Refill {
                first: 4,
                count: 1,
                late: 0
            }
        );
        assert_eq!(ring.slot(4), 0);
        assert!(ring.has_played(0) && !ring.has_played(1));
        // Two at once (one interrupt for two periods).
        assert_eq!(
            ring.advance(3),
            Refill {
                first: 5,
                count: 2,
                late: 0
            }
        );
        // Nothing new: nothing to do.
        assert_eq!(
            ring.advance(3),
            Refill {
                first: 7,
                count: 0,
                late: 0
            }
        );
        // Three: the most that can finish without the DMA playing a slot
        // that was not refilled.
        assert_eq!(
            ring.advance(6),
            Refill {
                first: 7,
                count: 3,
                late: 0
            }
        );
        // Six at once: the DMA went round. Periods 10 and 11 were never
        // written and played old ones, and 12 is playing an old one: three
        // late. Fill the three after it.
        assert_eq!(
            ring.advance(12),
            Refill {
                first: 13,
                count: 3,
                late: 3
            }
        );
        assert_eq!(ring.slot(13), 1, "never the playing slot 0");
    }

    #[test]
    fn the_channel_fits_its_control_page() {
        assert!(size_of::<AudioChannel>() <= 4096);
    }
}

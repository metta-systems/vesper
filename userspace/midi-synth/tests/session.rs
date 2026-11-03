//! Host tests of the synthesiser component's session: the packet protocol of
//! `midi_synth_v1` over memory of the test's own, with the test song and the
//! bank trimmed to it (`fixtures/`), as the component plays them.

#[cfg(test)]
mod tests {
    use {
        libio_channel::{DataArea, IoRec, Packet},
        midi_synth::{
            Command, Fnv, HASH_FRAMES, Handled, Memory, PERIOD_BYTES, PERIOD_FRAMES,
            RENDER_AREA_BYTES, SONG_FIRST, SONG_LAST, SONG_LOOPING, Session, volume_value,
        },
        std::{fs, path::Path, sync::OnceLock},
        vesper_synth::{Bank, OUTPUT_RATE, Sequencer, Song, Synth},
    };

    /// The test song, Bach's Prelude in C (BWV 846, from the Mutopia
    /// Project): `fixtures/README.md`.
    const PRELUDE: &[u8] = include_bytes!("../fixtures/wtk1-prelude1.mid");

    /// `GeneralUser GS` trimmed to the test song, read once.
    fn bank() -> &'static [u8] {
        static BANK: OnceLock<Vec<u8>> = OnceLock::new();
        BANK.get_or_init(|| {
            fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/wtk1-prelude1.sf2"))
                .unwrap()
        })
    }

    /// Where the test's data pages start, and their size: sixteen pages.
    const BASE: u64 = 0x4000_1000;
    const AREA: DataArea = DataArea {
        base: BASE,
        len: 16 * 4096,
    };

    /// The data pages, in a vector.
    struct Pages(Vec<u8>);

    impl Pages {
        fn new() -> Self {
            Self(vec![0; 16 * 4096])
        }

        fn offset(address: u64) -> usize {
            usize::try_from(address - BASE).unwrap()
        }

        fn frames(&self, record: IoRec) -> Vec<i16> {
            let start = Self::offset(record.base);
            self.0[start..start + usize::try_from(record.len).unwrap()]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&pair| i16::from_le_bytes(pair))
                .collect()
        }
    }

    impl Memory for Pages {
        fn read(&self, record: IoRec, out: &mut Vec<u8>) {
            let start = Self::offset(record.base);
            out.extend_from_slice(&self.0[start..start + usize::try_from(record.len).unwrap()]);
        }

        fn write(&mut self, address: u64, bytes: &[u8]) {
            let start = Self::offset(address);
            self.0[start..start + bytes.len()].copy_from_slice(bytes);
        }
    }

    fn session() -> Session<'static> {
        Session::new(Bank::new(bank()).unwrap())
    }

    /// Send `midi` as song packets of at most `chunk` bytes, through the song
    /// area after the render buffers, as Wood does.
    fn send_song(
        session: &mut Session<'static>,
        pages: &mut Pages,
        midi: &[u8],
        looping: bool,
        chunk: usize,
    ) -> Vec<Handled> {
        let count = midi.len().div_ceil(chunk);
        midi.chunks(chunk)
            .enumerate()
            .map(|(index, bytes)| {
                let base = BASE + RENDER_AREA_BYTES;
                pages.write(base, bytes);
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
                    base,
                    len: bytes.len() as u64,
                };
                let mut packet = Packet::new(Command::Song.value(flags, 7), &[record]).unwrap();
                session.handle(&mut packet, AREA, pages)
            })
            .collect()
    }

    /// One control packet, its record empty.
    fn control(session: &mut Session<'static>, pages: &mut Pages, value: u64) -> Handled {
        let record = IoRec { base: BASE, len: 0 };
        let mut packet = Packet::new(value, &[record]).unwrap();
        session.handle(&mut packet, AREA, pages)
    }

    /// Render one period into buffer `index`; its frames.
    fn render(session: &mut Session<'static>, pages: &mut Pages, index: u64) -> Vec<i16> {
        let record = IoRec {
            base: BASE + index * PERIOD_BYTES,
            len: PERIOD_BYTES,
        };
        let mut packet = Packet::new(Command::Render.value(0, index), &[record]).unwrap();
        assert_eq!(
            session.handle(&mut packet, AREA, pages),
            Handled::Rendered(PERIOD_FRAMES as u64)
        );
        assert_eq!(packet.recs()[0].len, PERIOD_BYTES);
        assert_eq!(packet.value() >> 16, index);
        pages.frames(packet.recs()[0])
    }

    /// The start hash of the test song at `volume`, rendered straight by the
    /// sequencer in periods, as the component hashes it.
    fn reference_hash(looping: bool, volume: u8) -> (u64, u32) {
        let mut sequencer = Sequencer::new(Synth::new(Bank::new(bank()).unwrap(), OUTPUT_RATE));
        sequencer.play(Song::parse(PRELUDE).unwrap(), looping);
        sequencer.set_volume(volume);
        let mut hash = Fnv::NEW;
        let mut audible = 0;
        let mut period = vec![0_i16; 2 * PERIOD_FRAMES];
        for _ in 0..HASH_FRAMES / PERIOD_FRAMES as u64 {
            sequencer.render(&mut period);
            let bytes: Vec<u8> = period
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect();
            hash.add(&bytes);
            audible += u32::from(period.iter().any(|&sample| sample != 0));
        }
        (hash.value(), audible)
    }

    #[test]
    fn commands_encode_and_decode() {
        for command in [
            Command::Song,
            Command::Render,
            Command::Stop,
            Command::Pause,
            Command::Resume,
            Command::Volume,
        ] {
            let value = command.value(SONG_LAST, 0x1234);
            assert_eq!(Command::of(value), Some(command));
            assert_eq!(value >> 16, 0x1234);
            assert_eq!(value & SONG_LAST, SONG_LAST);
        }
        assert_eq!(Command::of(0), None);
        assert_eq!(Command::of(7), None);
        assert_eq!(volume_value(200, 0) >> 8 & 0xff, 127);
        assert_eq!(volume_value(64, 3), 6 | 64 << 8 | 3 << 16);
    }

    #[test]
    fn a_song_in_chunks_plays_as_the_sequencer_renders_it() {
        let midi = PRELUDE;
        let mut session = session();
        let mut pages = Pages::new();
        assert!(!session.playing());
        // A period before any song is silent.
        assert!(render(&mut session, &mut pages, 0).iter().all(|&s| s == 0));
        let song_area = usize::try_from(16 * 4096 - RENDER_AREA_BYTES).unwrap();
        let handled = send_song(&mut session, &mut pages, midi, true, song_area);
        assert_eq!(handled.last(), Some(&Handled::Song(Some(midi.len()))));
        assert!(session.playing());
        let mut hash = Fnv::NEW;
        for period in 0..HASH_FRAMES / PERIOD_FRAMES as u64 {
            let frames = render(&mut session, &mut pages, period % 4);
            let bytes: Vec<u8> = frames
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect();
            hash.add(&bytes);
        }
        let start = session.take_start_hash().unwrap();
        assert_eq!(start.hash, hash.value());
        assert_eq!(
            (start.hash, start.audible_periods),
            reference_hash(true, 127)
        );
        assert_eq!(
            (start.song_bytes, start.looping, start.volume),
            (midi.len(), true, 127)
        );
        assert_eq!(session.take_start_hash(), None);

        // The same song in 1000-byte chunks hashes the same.
        let mut session = self::session();
        let handled = send_song(&mut session, &mut pages, midi, true, 1000);
        assert!(
            handled[..handled.len() - 1]
                .iter()
                .all(|&h| h == Handled::Song(None))
        );
        for period in 0..HASH_FRAMES / PERIOD_FRAMES as u64 {
            render(&mut session, &mut pages, period % 4);
        }
        assert_eq!(session.take_start_hash().unwrap().hash, start.hash);
    }

    /// The start hash of the song `just _test-midi` plays: the test song
    /// (4 676 bytes), looping, at the MIDI test player's volume, 64
    /// (`MUSIC_VOLUME` in userspace/tests/audio/midi-player). `just _test-midi`
    /// checks the synthesiser's log line against it (`midi_test_hash` in the
    /// Justfile); change both together, and only for an intended change of
    /// the output. Wood's songs' start hashes are checked in vesper-doom's
    /// Room (`tests/doom_music.rs`).
    #[test]
    fn start_hash_of_the_midi_test_song() {
        assert_eq!(PRELUDE.len(), 4_676);
        let (hash, audible) = reference_hash(true, 64);
        println!("prelude looping true volume 64: {hash:#018x}, {audible} audible");
        assert_eq!((hash, audible), (0x56aa_0af3_23da_6658, 64));
    }

    /// The peak voice count `voices` reports: none before a song, some while
    /// it plays, and the count starts over at each question.
    #[test]
    fn peak_voices_follow_the_song() {
        let mut session = session();
        let mut pages = Pages::new();
        render(&mut session, &mut pages, 0);
        assert_eq!(session.take_peak_voices(), 0);
        send_song(&mut session, &mut pages, PRELUDE, true, 60_000);
        for index in 0..64 {
            render(&mut session, &mut pages, index % 4);
        }
        let peak = session.take_peak_voices();
        assert!((1..=32).contains(&peak), "{peak} voices");
        assert_eq!(session.take_peak_voices(), 0);
    }

    #[test]
    fn controls_pause_volume_and_stop() {
        let mut session = session();
        let mut pages = Pages::new();
        send_song(&mut session, &mut pages, PRELUDE, true, 60_000);
        for index in 0..20 {
            render(&mut session, &mut pages, index % 4);
        }
        assert!(render(&mut session, &mut pages, 0).iter().any(|&s| s != 0));
        assert_eq!(
            control(&mut session, &mut pages, Command::Pause.value(0, 0)),
            Handled::Control
        );
        assert!(render(&mut session, &mut pages, 1).iter().all(|&s| s == 0));
        control(&mut session, &mut pages, Command::Resume.value(0, 0));
        assert!(render(&mut session, &mut pages, 2).iter().any(|&s| s != 0));
        control(&mut session, &mut pages, volume_value(0, 0));
        assert!(render(&mut session, &mut pages, 3).iter().all(|&s| s == 0));
        control(&mut session, &mut pages, volume_value(127, 0));
        assert_eq!(
            control(&mut session, &mut pages, Command::Stop.value(0, 0)),
            Handled::Control
        );
        assert!(!session.playing());
        // The notes fade with their release, then silence.
        for index in 0..400 {
            render(&mut session, &mut pages, index % 4);
        }
        assert!(render(&mut session, &mut pages, 0).iter().all(|&s| s == 0));
        session.reset();
        assert!(!session.playing());
    }

    #[test]
    fn bad_packets_are_refused() {
        let mut session = session();
        let mut pages = Pages::new();
        // A record past the data pages comes back empty.
        let outside = IoRec {
            base: BASE + 16 * 4096 - 4,
            len: 8,
        };
        let mut packet = Packet::new(Command::Render.value(0, 0), &[outside]).unwrap();
        assert_eq!(
            session.handle(&mut packet, AREA, &mut pages),
            Handled::Refused
        );
        assert_eq!(packet.recs()[0].len, 0);
        // So does an unknown command.
        let inside = IoRec { base: BASE, len: 8 };
        let mut packet = Packet::new(0x99, &[inside]).unwrap();
        assert_eq!(
            session.handle(&mut packet, AREA, &mut pages),
            Handled::Refused
        );
        assert_eq!(packet.recs()[0].len, 0);
        // A render record is cut to whole frames.
        let ragged = IoRec {
            base: BASE,
            len: 10,
        };
        let mut packet = Packet::new(Command::Render.value(0, 0), &[ragged]).unwrap();
        assert_eq!(
            session.handle(&mut packet, AREA, &mut pages),
            Handled::Rendered(2)
        );
        assert_eq!(packet.recs()[0].len, 8);
        // Bytes that are no MIDI file play nothing.
        let handled = send_song(&mut session, &mut pages, b"not a MIDI file", false, 100);
        assert_eq!(handled, [Handled::BadSong]);
        assert!(!session.playing());
    }
}

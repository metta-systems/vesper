//! Host tests of the synthesiser component's session: the packet protocol of
//! `midi_synth_v1` over memory of the test's own, with the shareware IWAD's
//! music and the SC-55 trimmed to it, as the component plays them.

#[cfg(test)]
mod tests {
    use {
        libio_channel::{DataArea, IoRec, Packet},
        midi_synth::{
            Command, Fnv, HASH_FRAMES, Handled, Memory, PERIOD_BYTES, PERIOD_FRAMES,
            RENDER_AREA_BYTES, SONG_FIRST, SONG_LAST, SONG_LOOPING, Session, volume_value,
        },
        room::engine::sound::mus2midi::mus2midi,
        std::{fs, path::Path, sync::OnceLock},
        wood_synth::{
            Bank, OUTPUT_RATE, Sequencer, Song, Synth,
            sf2::SoundFont,
            trim::{Usage, trim},
        },
    };

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

    struct Data {
        songs: Vec<(String, Vec<u8>)>,
        bank: Vec<u8>,
    }

    /// The IWAD's songs as MIDI and the SC-55 trimmed to them, as
    /// `just wood-soundfont` writes it.
    fn data() -> &'static Data {
        static DATA: OnceLock<Data> = OnceLock::new();
        DATA.get_or_init(|| {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
            let wad = fs::read(root.join("wad/doom1.wad")).unwrap();
            let source =
                fs::read(root.join("room/soundfonts/SC55Soundfont-1.2b/SC-55 SoundFont v1.2b.sf2"))
                    .unwrap();
            let dword = |offset: usize| {
                usize::try_from(u32::from_le_bytes(
                    wad[offset..offset + 4].try_into().unwrap(),
                ))
                .unwrap()
            };
            let songs: Vec<(String, Vec<u8>)> = (0..dword(4))
                .filter_map(|entry| {
                    let record = dword(8) + entry * 16;
                    let (position, size) = (dword(record), dword(record + 4));
                    let name: String = wad[record + 8..record + 16]
                        .iter()
                        .take_while(|&&byte| byte != 0)
                        .map(|&byte| char::from(byte))
                        .collect();
                    let lump = &wad[position..position + size];
                    (name.starts_with("D_") && lump.starts_with(b"MUS\x1a"))
                        .then(|| (name, mus2midi(lump).unwrap()))
                })
                .collect();
            let mut usage = Usage::default();
            for (_, midi) in &songs {
                usage.add_song(&Song::parse(midi).unwrap());
            }
            let bank = trim(&SoundFont::parse(&source).unwrap(), &usage).bytes;
            Data { songs, bank }
        })
    }

    fn song(name: &str) -> &'static [u8] {
        &data()
            .songs
            .iter()
            .find(|(song, _)| song == name)
            .unwrap()
            .1
    }

    fn session() -> Session<'static> {
        Session::new(Bank::new(&data().bank).unwrap())
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

    /// The start hash of `name` at `volume`, rendered straight by the
    /// sequencer in periods, as the component hashes it.
    fn reference_hash(name: &str, looping: bool, volume: u8) -> (u64, u32) {
        let mut sequencer =
            Sequencer::new(Synth::new(Bank::new(&data().bank).unwrap(), OUTPUT_RATE));
        sequencer.play(Song::parse(song(name)).unwrap(), looping);
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
        let midi = song("D_E1M1");
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
            reference_hash("D_E1M1", true, 127)
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

    /// The start hashes of the first two songs of `just test-wood`, as the
    /// component logs them there ("first 64 periods hash"): the title's
    /// `D_INTROA` once, then the first demo's level song (`D_E1M5`) looping,
    /// both at the volume the engine sets, 64. `just test-wood` checks its log against
    /// the same values (`wood_music_hashes` in the Justfile); change both
    /// together, and only for an intended change of the output.
    #[test]
    fn start_hashes_of_the_first_songs() {
        // The QEMU log names the songs by size: 878 and 13 101 bytes.
        assert_eq!(song("D_INTROA").len(), 878);
        assert_eq!(song("D_E1M5").len(), 13_101);
        let hashes: Vec<(&str, u64, u32)> = [("D_INTROA", false), ("D_E1M5", true)]
            .into_iter()
            .map(|(name, looping)| {
                let (hash, audible) = reference_hash(name, looping, 64);
                println!("{name} looping {looping} volume 64: {hash:#018x}, {audible} audible");
                (name, hash, audible)
            })
            .collect();
        assert_eq!(
            hashes,
            [
                ("D_INTROA", 0x40f1_6e7e_ae7a_5f45, 64),
                ("D_E1M5", 0x4f0a_9ddf_3a4a_14be, 64)
            ]
        );
    }

    #[test]
    fn controls_pause_volume_and_stop() {
        let mut session = session();
        let mut pages = Pages::new();
        send_song(&mut session, &mut pages, song("D_E1M1"), true, 60_000);
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

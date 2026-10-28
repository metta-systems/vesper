//! Host tests on the real data: the shareware IWAD's music (converted with
//! Room's `mus2midi`, as the game does) and the SC-55 bank trimmed to it.
//! They check what the trim keeps, that `rustysynth` (the host's synth)
//! loads the trimmed bank, golden hashes of rendered music, and a loose
//! loudness comparison against `rustysynth`.

mod common;

#[cfg(test)]
mod tests {
    use {
        super::common::music_lumps,
        room::engine::sound::mus2midi::mus2midi,
        std::{
            collections::BTreeSet,
            fs,
            io::Cursor,
            path::Path,
            sync::{Arc, OnceLock},
        },
        wood_synth::{
            Bank, OUTPUT_RATE, Rate, Sequencer, Song, Synth,
            sf2::SoundFont,
            trim::{Usage, trim},
        },
    };

    struct Data {
        songs: Vec<(String, Vec<u8>)>,
        source: Vec<u8>,
        trimmed: Vec<u8>,
        usage: Usage,
    }

    /// The IWAD's songs as MIDI, the SC-55 bank and its trimmed copy (built
    /// once, in memory: the same as `just wood-soundfont` writes).
    fn data() -> &'static Data {
        static DATA: OnceLock<Data> = OnceLock::new();
        DATA.get_or_init(|| {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
            let wad = fs::read(root.join("wad/doom1.wad")).unwrap();
            let source =
                fs::read(root.join("room/soundfonts/SC55Soundfont-1.2b/SC-55 SoundFont v1.2b.sf2"))
                    .unwrap();
            let songs: Vec<(String, Vec<u8>)> = music_lumps(&wad)
                .into_iter()
                .map(|(name, mus)| (name, mus2midi(&mus).unwrap()))
                .collect();
            let mut usage = Usage::default();
            for (_, midi) in &songs {
                usage.add_song(&Song::parse(midi).unwrap());
            }
            let trimmed = trim(&SoundFont::parse(&source).unwrap(), &usage).bytes;
            Data {
                songs,
                source,
                trimmed,
                usage,
            }
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

    #[test]
    fn sc55_parses() {
        let font = SoundFont::parse(&data().source).unwrap();
        assert_eq!(font.preset_count(), 137);
        assert_eq!(font.sample_count(), 682);
        let bank = Bank::from_font(font);
        assert!(bank.exact_preset(0, 0).is_some());
        assert!(bank.exact_preset(128, 0).is_some());
    }

    #[test]
    fn songs_use_the_measured_programs_and_drums() {
        let data = data();
        assert_eq!(data.songs.len(), 13);
        let programs: BTreeSet<(u16, u16)> = [
            0, 6, 7, 10, 11, 15, 18, 29, 30, 31, 32, 33, 34, 37, 38, 40, 41, 42, 44, 45, 46, 47,
            48, 51, 52, 62, 63, 81, 82, 92, 94, 102, 108, 117, 118, 119, 120,
        ]
        .into_iter()
        .map(|program| (0, program))
        .collect();
        let drums: BTreeSet<(u16, u8)> = [
            35, 36, 38, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 55, 57, 59, 75, 80,
            81,
        ]
        .into_iter()
        .map(|key| (0, key))
        .collect();
        assert_eq!(data.usage.melodic, programs);
        assert_eq!(data.usage.percussion, drums);
    }

    #[test]
    fn trimmed_bank_is_a_valid_soundfont() {
        let data = data();
        let font = SoundFont::parse(&data.trimmed).unwrap();
        println!(
            "trimmed: {} bytes, {} of sample data, {} presets, {} instruments, {} samples",
            data.trimmed.len(),
            font.sample_data.len(),
            font.preset_count(),
            font.instrument_count(),
            font.sample_count()
        );
        assert!((4_000_000..4_600_000).contains(&data.trimmed.len()));
        assert_eq!(&font.preset_header(font.preset_count()).name[..3], b"EOP");
        assert_eq!(
            &font.instrument_header(font.instrument_count()).name[..3],
            b"EOI"
        );
        assert_eq!(&font.sample_header(font.sample_count()).name[..3], b"EOS");
        // Every sample lies in the data with 46 zero points after it.
        for index in 0..font.sample_count() {
            let header = font.sample_header(index);
            assert!(header.start < header.end);
            assert!(header.start <= header.loop_start && header.loop_end <= header.end);
            for point in header.end..header.end + 46 {
                assert_eq!(font.sample_point(point), 0);
            }
        }
        assert_eq!(
            font.sample_header(font.sample_count() - 1).end as usize + 46,
            font.sample_points()
        );
        // Every program the songs use has its own preset, with zones that
        // reach samples.
        let bank = Bank::from_font(font);
        for &(bank_number, program) in &data.usage.melodic {
            let preset = bank.exact_preset(bank_number, program).unwrap();
            let mut regions = 0;
            bank.regions(preset, 60, 100, &mut |_, _, _| regions += 1);
            assert!(regions > 0, "program {program}");
        }
        let kit = bank.exact_preset(128, 0).unwrap();
        for &(_, key) in &data.usage.percussion {
            let mut regions = 0;
            bank.regions(kit, key, 100, &mut |_, _, _| regions += 1);
            assert!(regions > 0, "drum {key}");
        }
        // A drum the songs never play was trimmed away.
        let mut regions = 0;
        bank.regions(kit, 56, 100, &mut |_, _, _| regions += 1);
        assert_eq!(regions, 0);
    }

    /// Mean square of each 100 ms window of interleaved stereo.
    fn window_power(frames: &[i16], rate: usize) -> Vec<f64> {
        frames
            .chunks(rate / 10 * 2)
            .map(|window| {
                window
                    .iter()
                    .map(|&sample| f64::from(sample).powi(2))
                    .sum::<f64>()
                    / f64::from(u32::try_from(window.len()).unwrap())
            })
            .collect()
    }

    #[expect(clippy::cast_possible_truncation, reason = "clamped to the i16 range")]
    fn rustysynth_render(sound_font: &[u8], midi: &[u8], seconds: usize) -> Vec<i16> {
        let font = Arc::new(rustysynth::SoundFont::new(&mut Cursor::new(sound_font)).unwrap());
        let settings = rustysynth::SynthesizerSettings::new(44_100);
        let synthesizer = rustysynth::Synthesizer::new(&font, &settings).unwrap();
        let file = Arc::new(rustysynth::MidiFile::new(&mut Cursor::new(midi)).unwrap());
        let mut sequencer = rustysynth::MidiFileSequencer::new(synthesizer);
        sequencer.play(&file, true);
        let mut left = vec![0.0; 44_100 * seconds];
        let mut right = vec![0.0; 44_100 * seconds];
        sequencer.render(&mut left, &mut right);
        left.iter()
            .zip(&right)
            .flat_map(|(&l, &r)| [l, r])
            .map(|sample: f32| (sample * 32_767.0).clamp(-32_768.0, 32_767.0) as i16)
            .collect()
    }

    #[test]
    fn rustysynth_loads_the_trimmed_bank() {
        let data = data();
        let font = Arc::new(
            rustysynth::SoundFont::new(&mut Cursor::new(data.trimmed.as_slice())).unwrap(),
        );
        assert_eq!(font.get_presets().len(), 38);
        let settings = rustysynth::SynthesizerSettings::new(44_100);
        let mut synthesizer = rustysynth::Synthesizer::new(&font, &settings).unwrap();
        synthesizer.note_on(0, 60, 100);
        let mut left = vec![0.0; 22_050];
        let mut right = vec![0.0; 22_050];
        synthesizer.render(&mut left, &mut right);
        let energy: f32 = left.iter().map(|sample| sample * sample).sum();
        assert!(energy > 1.0, "{energy}");
    }

    fn render(midi: &[u8], rate: Rate, seconds: u32) -> Vec<i16> {
        let bank = Bank::new(&data().trimmed).unwrap();
        let mut sequencer = Sequencer::new(Synth::new(bank, rate));
        sequencer.play(Song::parse(midi).unwrap(), true);
        let frames = u64::from(rate.numerator) * u64::from(seconds) / u64::from(rate.denominator);
        let mut out = vec![0; usize::try_from(frames).unwrap() * 2];
        // In the playback Thread's 512-frame periods.
        for period in out.chunks_mut(1024) {
            sequencer.render(period);
        }
        out
    }

    fn hash(frames: &[i16]) -> u64 {
        let bytes: Vec<u8> = frames
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        seahash::hash(&bytes)
    }

    /// Golden hashes of the first 10 s of two songs at the output rate,
    /// with the trimmed bank: any change to the synthesiser's output shows
    /// here (update the values only for an intended change).
    #[test]
    fn golden_renders() {
        let golden = [
            // Recorded 2026-10-28.
            ("D_E1M1", 0xd58a_e0b3_b486_ce67_u64, 853),
            // Seven seconds long: the loop restarts within the ten.
            ("D_INTRO", 0xce65_a2de_16f0_2611_u64, 862),
        ];
        let rendered: Vec<(&str, u64, usize)> = golden
            .iter()
            .map(|&(name, _, _)| {
                let out = render(song(name), OUTPUT_RATE, 10);
                assert_eq!(out.len(), 440_917 * 2);
                let audible = out
                    .chunks(1024)
                    .filter(|period| period.iter().any(|&sample| sample != 0))
                    .count();
                let peak = out
                    .iter()
                    .map(|&sample| i32::from(sample).abs())
                    .max()
                    .unwrap();
                println!("{name}: {audible} audible periods of 512 frames, peak {peak}");
                (name, hash(&out), audible)
            })
            .collect();
        for (name, hash, _) in &rendered {
            println!("{name}: {hash:#018x}");
        }
        assert_eq!(rendered, golden);
    }

    /// Loose similarity to the host's rustysynth (no filter, effects or
    /// modulators here, so not equal): the loudness of each 100 ms follows
    /// the same course and the overall level is within a factor of two.
    #[test]
    fn loudness_follows_rustysynth() {
        let data = data();
        for name in ["D_E1M1", "D_E1M2", "D_INTER"] {
            let ours = window_power(&render(song(name), Rate::hz(44_100), 20), 44_100);
            let theirs = window_power(&rustysynth_render(&data.trimmed, song(name), 20), 44_100);
            let count = ours.len().min(theirs.len());
            let mean = |values: &[f64]| {
                values.iter().sum::<f64>() / f64::from(u32::try_from(values.len()).unwrap())
            };
            let (ours_db, theirs_db): (Vec<f64>, Vec<f64>) = ours[..count]
                .iter()
                .zip(&theirs[..count])
                .map(|(a, b)| (10.0 * (a + 1.0).log10(), 10.0 * (b + 1.0).log10()))
                .unzip();
            let (mean_ours, mean_theirs) = (mean(&ours_db), mean(&theirs_db));
            let covariance: f64 = ours_db
                .iter()
                .zip(&theirs_db)
                .map(|(a, b)| (a - mean_ours) * (b - mean_theirs))
                .sum();
            let spread = |values: &[f64], mean: f64| {
                values
                    .iter()
                    .map(|value| (value - mean).powi(2))
                    .sum::<f64>()
                    .sqrt()
            };
            let correlation =
                covariance / (spread(&ours_db, mean_ours) * spread(&theirs_db, mean_theirs));
            let level = 10.0 * (mean(&ours[..count]) / mean(&theirs[..count])).log10();
            println!("{name}: correlation {correlation:.3}, level {level:+.1} dB");
            assert!(correlation > 0.7, "{name}: correlation {correlation}");
            assert!(level.abs() < 6.0, "{name}: level {level} dB");
        }
    }
}

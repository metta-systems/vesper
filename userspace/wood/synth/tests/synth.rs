//! Host tests of the synthesiser on small SoundFont and MIDI files built in
//! memory: the fixed-point tables, zone selection, envelope stages, pitch,
//! loops, channel controllers, the sequencer's timing, and the absence of
//! floating point in the library.

mod common;

#[cfg(test)]
mod tests {
    use {
        super::common::{
            LOOP, TestSample, TestZone, constant_sample, midi_file, peak, soundfont, tempo,
            timecents,
        },
        std::{fs, path::Path},
        wood_synth::{
            Bank, OUTPUT_RATE, Rate, Sequencer, Song, Synth,
            envelope::{Envelope, EnvelopeParameters, Stage},
            generator,
            sequencer::frames_per_tick,
            tables::{
                attenuation_gain, concave_centibels, gain_attenuation, pan_gains, phase_step,
                scale_by_cents, timecents_frames,
            },
        },
    };

    fn render(synth: &mut Synth<'_>, frames: usize) -> Vec<i16> {
        let mut out = vec![0; frames * 2];
        synth.render(&mut out);
        out
    }

    /// Frequency of the left channel from its rising zero crossings.
    fn frequency(frames: &[i16], rate: u32) -> u64 {
        let left: Vec<i16> = frames
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[left, _]| left)
            .collect();
        let rising = left
            .windows(2)
            .filter(|pair| pair.first() < Some(&0) && pair.last() >= Some(&0))
            .count();
        u64::try_from(rising).unwrap() * u64::from(rate) * 100
            / u64::try_from(frames.len() / 2).unwrap()
    }

    #[test]
    fn tables_match_their_curves() {
        assert_eq!(scale_by_cents(1 << 20, 1200), 2 << 20);
        assert_eq!(scale_by_cents(1 << 20, -1200), 1 << 19);
        assert_eq!(scale_by_cents(1 << 20, 0), 1 << 20);
        // 2^(700/1200) = 1.498307
        assert_eq!(scale_by_cents(1_000_000, 700), 1_498_307);
        assert_eq!(timecents_frames(0, Rate::hz(44_100)), 44_100);
        assert_eq!(timecents_frames(-1200, Rate::hz(44_100)), 22_050);
        assert_eq!(timecents_frames(0, OUTPUT_RATE), 44_092);
        // -12 000 timecents is 1/1024 s.
        assert_eq!(timecents_frames(-12_000, Rate::hz(102_400)), 100);
        assert_eq!(attenuation_gain(0), 65_536);
        // 6 dB: half amplitude; 20 dB: a tenth.
        assert_eq!(attenuation_gain(60 << 16), 32_846);
        assert_eq!(attenuation_gain(200 << 16), 6_554);
        assert_eq!(attenuation_gain(960 << 16), 0);
        assert_eq!(gain_attenuation(32_846), 60 << 16);
        assert_eq!(gain_attenuation(65_536), 0);
        assert_eq!(gain_attenuation(0), 960 << 16);
        let halfway = gain_attenuation(32_768);
        assert!((60 << 16..61 << 16).contains(&halfway), "{halfway:#x}");
        assert_eq!(concave_centibels(127), 0);
        assert_eq!(concave_centibels(64), 119);
        assert_eq!(concave_centibels(0), 960);
        assert_eq!(pan_gains(0), (23_170, 23_170));
        assert_eq!(pan_gains(-500), (32_768, 0));
        assert_eq!(pan_gains(500), (0, 32_768));
        // A sample at the output rate played at its root steps one point.
        assert_eq!(phase_step(44_100, 0, Rate::hz(44_100)), 1 << 32);
        assert_eq!(phase_step(22_050, 1200, Rate::hz(44_100)), 1 << 32);
    }

    #[test]
    fn envelope_runs_its_stages() {
        let mut envelope = Envelope::new(EnvelopeParameters {
            delay: 100,
            attack: 100,
            hold: 100,
            decay: 800,
            sustain: 200,
            release: 400,
        });
        assert_eq!(envelope.stage(), Stage::Delay);
        envelope.advance(100);
        assert_eq!(envelope.stage(), Stage::Attack);
        assert_eq!(envelope.gain(), 0);
        envelope.advance(50);
        assert_eq!(envelope.gain(), 32_768);
        envelope.advance(50);
        assert_eq!(envelope.stage(), Stage::Hold);
        assert_eq!(envelope.gain(), 65_536);
        envelope.advance(100);
        assert_eq!(envelope.stage(), Stage::Decay);
        // 800 cB over 800 frames: 1 cB a frame, sustain (200 cB) after 200.
        envelope.advance(100);
        assert_eq!(envelope.gain(), attenuation_gain(100 << 16));
        envelope.advance(100);
        assert_eq!(envelope.stage(), Stage::Sustain);
        assert_eq!(envelope.gain(), 6_554);
        envelope.advance(10_000);
        assert_eq!(envelope.stage(), Stage::Sustain);
        envelope.release();
        assert_eq!(envelope.stage(), Stage::Release);
        // 800 cB over 400 frames: 2 cB a frame, from 200 to 960 in 380.
        envelope.advance(379);
        assert_eq!(envelope.stage(), Stage::Release);
        envelope.advance(1);
        assert!(envelope.is_finished());
    }

    #[test]
    fn envelope_released_during_attack_falls_from_its_level() {
        let mut envelope = Envelope::new(EnvelopeParameters {
            delay: 0,
            attack: 100,
            hold: 0,
            decay: 100,
            sustain: 0,
            release: 100,
        });
        envelope.advance(50);
        envelope.release();
        assert_eq!(envelope.stage(), Stage::Release);
        assert!(envelope.gain().abs_diff(32_768) <= 4, "{}", envelope.gain());
        let mut delayed = Envelope::new(EnvelopeParameters {
            delay: 10,
            attack: 10,
            hold: 0,
            decay: 10,
            sustain: 0,
            release: 10,
        });
        delayed.release();
        assert!(delayed.is_finished());
    }

    #[test]
    fn zones_are_chosen_by_key_and_velocity() {
        let font = soundfont(
            0,
            &[
                constant_sample(8000, 1000),
                constant_sample(-8000, 1000),
                constant_sample(2000, 1000),
            ],
            &[
                TestZone {
                    generators: vec![(generator::KEY_RANGE, u16::from_le_bytes([0, 59])), LOOP],
                    sample: 0,
                },
                TestZone {
                    generators: vec![
                        (generator::KEY_RANGE, u16::from_le_bytes([60, 127])),
                        (generator::VEL_RANGE, u16::from_le_bytes([64, 127])),
                        LOOP,
                    ],
                    sample: 1,
                },
                TestZone {
                    generators: vec![
                        (generator::KEY_RANGE, u16::from_le_bytes([60, 127])),
                        (generator::VEL_RANGE, u16::from_le_bytes([0, 63])),
                        LOOP,
                    ],
                    sample: 2,
                },
            ],
            &[],
        );
        let bank = Bank::new(&font).unwrap();
        let mut seen = Vec::new();
        bank.regions(0, 50, 100, &mut |_, zone, _| seen.push(zone.target));
        bank.regions(0, 70, 100, &mut |_, zone, _| seen.push(zone.target));
        bank.regions(0, 70, 10, &mut |_, zone, _| seen.push(zone.target));
        assert_eq!(seen, [0, 1, 2]);

        let mut synth = Synth::new(bank, Rate::hz(44_100));
        synth.message(0x90, 50, 127);
        let low = render(&mut synth, 2000);
        synth.message(0x80, 50, 0);
        render(&mut synth, 2000);
        synth.message(0x90, 70, 127);
        let high = render(&mut synth, 2000);
        assert!(
            low[2000] > 0 && high[2000] < 0,
            "{} {}",
            low[2000],
            high[2000]
        );
    }

    #[test]
    fn preset_generators_add_to_instrument_values() {
        let font = soundfont(
            0,
            &[constant_sample(16_000, 1000)],
            &[TestZone {
                generators: vec![LOOP, (generator::INITIAL_ATTENUATION, 100)],
                sample: 0,
            }],
            // 150 more centibels at preset level: 250 in all, 100 counted.
            &[(generator::INITIAL_ATTENUATION, 150)],
        );
        let bank = Bank::new(&font).unwrap();
        let mut value = 0;
        bank.regions(0, 60, 127, &mut |preset_zone, instrument_zone, _| {
            value = wood_synth::bank::voice_value(
                preset_zone,
                instrument_zone,
                generator::INITIAL_ATTENUATION,
            );
        });
        assert_eq!(value, 250);
        let mut synth = Synth::new(bank, Rate::hz(44_100));
        synth.message(0x90, 60, 127);
        let out = render(&mut synth, 4000);
        // 16 000 at 100 cB (0.4 of 250) plus channel volume 100 (42 cB),
        // centre pan (-3 dB) and the master's half.
        assert_eq!(concave_centibels(100), 42);
        let gain = i32::try_from(attenuation_gain(142 << 16)).unwrap();
        let expected = 16_000 * gain / 65_536 * 23_170 / 32_768 / 2;
        let level = i32::from(out[6000]);
        assert!((level - expected).abs() <= 40, "{level} vs {expected}");
    }

    fn sine_font(root: u8) -> Vec<u8> {
        // One period of 100 points at 44 000 Hz: 440 Hz at its root.
        let points = (0..100)
            .map(|index| {
                let phase = (index * 4) % 400;
                // A triangle is enough for zero crossings.
                let value = if phase < 200 {
                    phase - 100
                } else {
                    300 - phase
                };
                i16::try_from(value * 200).unwrap()
            })
            .collect();
        soundfont(
            0,
            &[TestSample {
                points,
                rate: 44_000,
                root,
                loop_points: (0, 100),
            }],
            &[TestZone {
                generators: vec![LOOP],
                sample: 0,
            }],
            &[],
        )
    }

    #[test]
    fn pitch_follows_key_tuning_and_bend() {
        let font = sine_font(69);
        let rate = OUTPUT_RATE;
        let frames = 44_092;
        let measure = |messages: &[[u8; 3]]| {
            let mut synth = Synth::new(Bank::new(&font).unwrap(), rate);
            for message in messages {
                synth.message(message[0], message[1], message[2]);
            }
            frequency(&render(&mut synth, frames), 44_092)
        };
        // In hundredths of Hz; one crossing in the second is 1 Hz.
        let a4 = measure(&[[0x90, 69, 100]]);
        assert!((43_900..=44_100).contains(&a4), "{a4}");
        let a5 = measure(&[[0x90, 81, 100]]);
        assert!((87_900..=88_100).contains(&a5), "{a5}");
        // Full bend up, default range two semitones: B4, 493.88 Hz.
        let bent = measure(&[[0xe0, 0x7f, 0x7f], [0x90, 69, 100]]);
        assert!((49_300..=49_500).contains(&bent), "{bent}");
        // Range set to 12 semitones through registered parameter 0.
        let octave = measure(&[
            [0xb0, 101, 0],
            [0xb0, 100, 0],
            [0xb0, 6, 12],
            [0xe0, 0x7f, 0x7f],
            [0x90, 69, 100],
        ]);
        assert!((87_900..=88_100).contains(&octave), "{octave}");
        // The sample's root key 57 sounds 440 Hz an octave up at key 69.
        let font_low = sine_font(57);
        let mut synth = Synth::new(Bank::new(&font_low).unwrap(), rate);
        synth.message(0x90, 69, 100);
        let up = frequency(&render(&mut synth, frames), 44_092);
        assert!((87_900..=88_100).contains(&up), "{up}");
    }

    fn envelope_font() -> Vec<u8> {
        soundfont(
            0,
            &[constant_sample(16_000, 1000)],
            &[TestZone {
                generators: vec![
                    LOOP,
                    // 0.1 s delay, attack and hold; decay 1 s; sustain -20 dB;
                    // release 0.5 s.
                    (generator::DELAY_VOL_ENV, timecents(-3986)),
                    (generator::ATTACK_VOL_ENV, timecents(-3986)),
                    (generator::HOLD_VOL_ENV, timecents(-3986)),
                    (generator::DECAY_VOL_ENV, 0),
                    (generator::SUSTAIN_VOL_ENV, 200),
                    (generator::RELEASE_VOL_ENV, timecents(-1200)),
                ],
                sample: 0,
            }],
            &[],
        )
    }

    #[test]
    fn voice_envelope_shapes_the_output() {
        let font = envelope_font();
        let mut synth = Synth::new(Bank::new(&font).unwrap(), Rate::hz(44_100));
        synth.message(0x90, 60, 127);
        let out = render(&mut synth, 3 * 44_100);
        let level = |seconds_x100: usize| i32::from(out[seconds_x100 * 441 * 2]);
        let full = level(25);
        // 16 000 at volume 100 (42 cB), centre pan and the master's half.
        assert!((3480..3500).contains(&full), "{full}");
        assert_eq!(level(5), 0);
        assert!((level(15) - full / 2).abs() < full / 20, "{}", level(15));
        // Decay from 0.3 s at 800 cB/s: at 0.45 s 120 cB down (0.251), at
        // the sustain level (200 cB, 0.1) from 0.55 s.
        assert!(
            (level(45) - full * 251 / 1000).abs() < full / 100,
            "{}",
            level(45)
        );
        assert!((level(100) - full / 10).abs() < full / 50, "{}", level(100));
        synth.message(0x80, 60, 0);
        assert_eq!(synth.active_voices(), 1);
        // From 200 to 960 cB at 1600 cB/s: 0.475 s.
        render(&mut synth, 20_000);
        assert_eq!(synth.active_voices(), 1);
        render(&mut synth, 1_200);
        assert_eq!(synth.active_voices(), 0);
    }

    fn loop_font(mode: u16) -> Vec<u8> {
        soundfont(
            0,
            &[constant_sample(8000, 1000)],
            &[TestZone {
                generators: vec![
                    (generator::SAMPLE_MODES, mode),
                    (generator::RELEASE_VOL_ENV, timecents(2786)),
                ],
                sample: 0,
            }],
            &[],
        )
    }

    #[test]
    fn loop_modes() {
        // No loop: 1000 points at the output rate, the voice ends with them.
        let font = loop_font(0);
        let mut synth = Synth::new(Bank::new(&font).unwrap(), Rate::hz(44_100));
        synth.message(0x90, 60, 127);
        render(&mut synth, 960);
        assert_eq!(synth.active_voices(), 1);
        render(&mut synth, 64);
        assert_eq!(synth.active_voices(), 0);

        // Continuous: still sounding after a second, released or not.
        let font = loop_font(1);
        let mut synth = Synth::new(Bank::new(&font).unwrap(), Rate::hz(44_100));
        synth.message(0x90, 60, 127);
        let out = render(&mut synth, 44_100);
        assert!(peak(&out[80_000..], 0) > 1000);
        synth.message(0x80, 60, 0);
        render(&mut synth, 44_100);
        assert_eq!(synth.active_voices(), 1);

        // Until release: loops while held, then plays out the rest of the
        // sample (under 1000 points) long before its 5 s release ends.
        let font = loop_font(3);
        let mut synth = Synth::new(Bank::new(&font).unwrap(), Rate::hz(44_100));
        synth.message(0x90, 60, 127);
        render(&mut synth, 44_100);
        assert_eq!(synth.active_voices(), 1);
        synth.message(0x80, 60, 0);
        render(&mut synth, 1_024);
        assert_eq!(synth.active_voices(), 0);
    }

    #[test]
    fn controllers_change_level_pan_and_pedal() {
        let font = loop_font(1);
        let new_synth = || Synth::new(Bank::new(&font).unwrap(), Rate::hz(44_100));

        let mut synth = new_synth();
        synth.message(0x90, 60, 127);
        let loud = peak(&render(&mut synth, 4000)[4000..], 0);
        synth.message(0xb0, 7, 0);
        let quiet = peak(&render(&mut synth, 4000)[4000..], 0);
        assert!(loud > 1000 && quiet == 0, "{loud} {quiet}");
        // Volume 127 and expression 64 (119 cB) against volume 100 (42 cB).
        synth.message(0xb0, 7, 127);
        synth.message(0xb0, 11, 64);
        let expressed = peak(&render(&mut synth, 4000)[4000..], 0);
        let expected = loud * i32::try_from(attenuation_gain(77 << 16)).unwrap() / 65_536;
        assert!((expressed - expected).abs() < 30, "{expressed} {expected}");

        // Pan hard left.
        let mut synth = new_synth();
        synth.message(0xb0, 10, 0);
        synth.message(0x90, 60, 127);
        let out = render(&mut synth, 4000);
        assert!(peak(&out[4000..], 0) > 1000);
        assert_eq!(peak(&out[4000..], 1), 0);

        // The pedal holds a released note until it lifts.
        let font = envelope_font();
        let mut synth = Synth::new(Bank::new(&font).unwrap(), Rate::hz(44_100));
        synth.message(0xb0, 64, 127);
        synth.message(0x90, 60, 127);
        synth.message(0x80, 60, 0);
        render(&mut synth, 44_100);
        assert_eq!(synth.active_voices(), 1);
        synth.message(0xb0, 64, 0);
        render(&mut synth, 44_100);
        assert_eq!(synth.active_voices(), 0);

        // All sound off stops at once; reset keeps volume.
        let mut synth = new_synth();
        synth.message(0x90, 60, 127);
        synth.message(0x90, 64, 127);
        assert_eq!(synth.active_voices(), 2);
        synth.message(0xb0, 120, 0);
        assert_eq!(synth.active_voices(), 0);
        synth.message(0xb0, 7, 90);
        synth.message(0xe0, 0, 0x60);
        synth.message(0xb0, 121, 0);
        assert_eq!(synth.channel(0).volume, 90);
        assert_eq!(synth.channel(0).bend, 0);
    }

    /// The output does not depend on how the frames are split into render
    /// calls: the envelope and gains move on a grid of blocks from the reset.
    #[test]
    fn output_is_independent_of_the_render_split() {
        let font = envelope_font();
        let midi = midi_file(
            70,
            &[
                (3, vec![0x90, 60, 100]),
                (40, vec![0xb0, 7, 64]),
                (7, vec![0xe0, 0, 0x50]),
                (30, vec![0x80, 60, 0]),
            ],
            100,
        );
        let song = Song::parse(&midi).unwrap();
        let frames = 60_000;
        let render_in = |pieces: &[usize]| {
            let mut sequencer = Sequencer::new(Synth::new(Bank::new(&font).unwrap(), OUTPUT_RATE));
            sequencer.play(song.clone(), false);
            let mut out = vec![0_i16; 2 * frames];
            let mut done = 0;
            for &piece in pieces.iter().cycle() {
                if done == frames {
                    break;
                }
                let count = piece.min(frames - done);
                sequencer.render(&mut out[2 * done..2 * (done + count)]);
                done += count;
            }
            out
        };
        let whole = render_in(&[frames]);
        assert!(whole.iter().any(|&sample| sample != 0));
        assert_eq!(render_in(&[512]), whole);
        assert_eq!(render_in(&[1]), whole);
        assert_eq!(render_in(&[37, 100, 3]), whole);
    }

    #[test]
    fn exclusive_class_and_polyphony() {
        let font = soundfont(
            128,
            &[constant_sample(8000, 1000)],
            &[TestZone {
                generators: vec![LOOP, (generator::EXCLUSIVE_CLASS, 1)],
                sample: 0,
            }],
            &[],
        );
        let mut synth = Synth::new(Bank::new(&font).unwrap(), Rate::hz(44_100));
        // Channel 10 plays the percussion bank.
        synth.message(0x99, 42, 127);
        render(&mut synth, 100);
        synth.message(0x99, 46, 127);
        assert_eq!(synth.active_voices(), 2);
        render(&mut synth, 200);
        assert_eq!(synth.active_voices(), 1);
        // A melodic channel finds no bank-0 preset.
        synth.message(0x90, 60, 127);
        assert_eq!(synth.active_voices(), 1);

        let font = loop_font(1);
        let mut synth = Synth::new(Bank::new(&font).unwrap(), Rate::hz(44_100));
        for key in 0..40 {
            synth.message(0x90, key, 127);
        }
        assert_eq!(synth.active_voices(), wood_synth::synth::POLYPHONY);
    }

    /// A one-voice bank whose voices end 1 ms after their note-off.
    fn click_font() -> Vec<u8> {
        soundfont(
            0,
            &[constant_sample(8000, 1000)],
            &[TestZone {
                generators: vec![LOOP],
                sample: 0,
            }],
            &[],
        )
    }

    /// Render one frame at a time and return the frames at which the number
    /// of sounding voices grew.
    fn note_starts(sequencer: &mut Sequencer<'_>, frames: u64) -> Vec<u64> {
        let mut starts = Vec::new();
        let mut voices = 0;
        let mut frame = [0_i16; 2];
        for index in 0..frames {
            sequencer.render(&mut frame);
            let now = sequencer.synth().active_voices();
            if now > voices {
                starts.push(index);
            }
            voices = now;
        }
        starts
    }

    #[test]
    fn sequencer_events_land_on_their_frames() {
        let font = click_font();
        // Doom's MIDI: 70 ticks per quarter at 500 000 us, 140 ticks/s.
        let midi = midi_file(
            70,
            &[
                (0, tempo(500_000)),
                (10, vec![0x90, 60, 100]),
                (5, vec![0x80, 60, 0]),
                (10, vec![0x90, 62, 100]),
                // Twice the tempo from tick 30.
                (5, tempo(250_000)),
                (10, vec![0x80, 62, 0]),
                (0, vec![0x90, 64, 100]),
                (5, vec![0x80, 64, 0]),
            ],
            5,
        );
        let song = Song::parse(&midi).unwrap();
        assert_eq!(song.end_tick, 50);
        let per_tick = frames_per_tick(OUTPUT_RATE, 500_000, 70);
        // 314.9408 frames per tick.
        assert_eq!(per_tick >> 32, 314);
        let fast = frames_per_tick(OUTPUT_RATE, 250_000, 70);
        let frame = |time: u64| time.div_ceil(1 << 32);
        let expected = [
            frame(10 * per_tick),
            frame(25 * per_tick),
            frame(30 * per_tick + 10 * fast),
        ];
        assert_eq!(expected, [3150, 7874, 11_023]);

        let mut sequencer = Sequencer::new(Synth::new(Bank::new(&font).unwrap(), OUTPUT_RATE));
        sequencer.play(song.clone(), false);
        assert_eq!(note_starts(&mut sequencer, 14_000), expected);
        assert!(!sequencer.is_playing());

        // Looping: the second pass starts where the first ended, exactly.
        let end = 30 * per_tick + 20 * fast;
        let mut sequencer = Sequencer::new(Synth::new(Bank::new(&font).unwrap(), OUTPUT_RATE));
        sequencer.play(song, true);
        // Two passes: the third would start at frame 28 345.
        let starts = note_starts(&mut sequencer, 25_000);
        assert_eq!(&starts[..3], expected);
        assert_eq!(
            &starts[3..],
            [
                frame(end + 10 * per_tick),
                frame(end + 25 * per_tick),
                frame(end + 30 * per_tick + 10 * fast),
            ]
        );
        assert!(sequencer.is_playing());

        // Pausing holds the clock; rendering in blocks lands the same.
        let midi = midi_file(70, &[(10, vec![0x90, 60, 100])], 100);
        let mut sequencer = Sequencer::new(Synth::new(Bank::new(&font).unwrap(), OUTPUT_RATE));
        sequencer.play(Song::parse(&midi).unwrap(), false);
        let mut block = vec![0_i16; 2 * 3000];
        sequencer.render(&mut block);
        sequencer.pause();
        sequencer.render(&mut block);
        assert!(block.iter().all(|&sample| sample == 0));
        assert_eq!(sequencer.frames(), 3000);
        sequencer.resume();
        // 300 frames: the event falls at frame 3150, the 151st of them.
        sequencer.render(&mut block[..600]);
        assert_eq!(sequencer.synth().active_voices(), 1);
        let first = block[..600]
            .as_chunks::<2>()
            .0
            .iter()
            .position(|&[left, _]| left != 0)
            .unwrap();
        // Note on before frame 3150 (index 150 of this block), audible from
        // the next (the attack starts at zero).
        assert!((150..=152).contains(&first), "{first}");
    }

    #[test]
    fn smf_reader_handles_running_status_and_two_tracks() {
        // Running status: 0x90 once, three notes; a second track with a
        // program change at tick 0 sorts after track one's tick-0 events.
        let mut midi = midi_file(
            96,
            &[
                (0, vec![0x90, 60, 100]),
                (4, vec![62, 100]),
                (4, vec![64, 0]),
            ],
            0,
        );
        midi[9] = 1; // format 1
        midi[11] = 2; // two tracks
        let second = [0x00, 0xc0, 5, 0x00, 0xff, 0x2f, 0x00];
        midi.extend_from_slice(b"MTrk");
        midi.extend_from_slice(&u32::try_from(second.len()).unwrap().to_be_bytes());
        midi.extend_from_slice(&second);
        let song = Song::parse(&midi).unwrap();
        assert_eq!(song.division, 96);
        assert_eq!(song.end_tick, 8);
        let ticks: Vec<u32> = song.events.iter().map(|event| event.tick).collect();
        assert_eq!(ticks, [0, 0, 4, 8]);
        assert!(matches!(
            song.events[1].kind,
            wood_synth::smf::EventKind::Channel {
                status: 0xc0,
                data1: 5,
                ..
            }
        ));
        // SMPTE division (25 frames per second, 40 ticks per frame).
        assert_eq!(
            Song::parse(b"MThd\0\0\0\x06\0\0\0\x01\xe7\x28").unwrap_err(),
            wood_synth::smf::SmfError::SmpteDivision
        );
    }

    /// No floating point in the library's source: no `f32`/`f64` and no
    /// float literal outside comments (the tables are built by `build.rs`).
    #[test]
    fn library_source_has_no_floating_point() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut checked = 0;
        for entry in fs::read_dir(source).unwrap() {
            let path = entry.unwrap().path();
            let text = fs::read_to_string(&path).unwrap();
            for (number, line) in text.lines().enumerate() {
                let code = line.split("//").next().unwrap();
                let bytes = code.as_bytes();
                let is_ident = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
                for (index, window) in bytes.windows(3).enumerate() {
                    let float_type = window == b"f32" || window == b"f64";
                    let before_ok = index == 0 || !is_ident(bytes[index - 1]);
                    let after_ok = bytes.get(index + 3).is_none_or(|&byte| !is_ident(byte));
                    assert!(
                        !(float_type && before_ok && after_ok),
                        "{}:{}: {line}",
                        path.display(),
                        number + 1
                    );
                    let &[first, middle, last] = window else {
                        continue;
                    };
                    let literal = first.is_ascii_digit() && middle == b'.' && last.is_ascii_digit();
                    assert!(!literal, "{}:{}: {line}", path.display(), number + 1);
                }
            }
            checked += 1;
        }
        assert!(checked >= 10);
    }
}

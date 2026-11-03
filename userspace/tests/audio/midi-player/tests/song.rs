//! Host tests of the MIDI test player's library: the start-up lines' numbers
//! for the test song (`userspace/midi-synth/fixtures/wtk1-prelude1.mid`), the
//! song position against the sequencer's own clock, and the level meter.

#[cfg(test)]
mod tests {
    use {
        midi_player::{LevelMeter, Note, NoteList, Position, SongInfo},
        vesper_synth::{OUTPUT_RATE, Song},
    };

    /// The test song: Bach's Prelude in C, BWV 846, from the Mutopia Project.
    const PRELUDE: &[u8] = include_bytes!("../../../../midi-synth/fixtures/wtk1-prelude1.mid");

    fn info() -> SongInfo {
        SongInfo::of(&Song::parse(PRELUDE).unwrap(), OUTPUT_RATE)
    }

    #[test]
    fn prelude_timing_and_first_notes() {
        let info = info();
        // 384 ticks per quarter at 60 bpm (384 ticks per second), one tempo
        // event at the start.
        assert_eq!(info.division, 384);
        assert_eq!(info.tempo, 1_000_000);
        assert_eq!(info.beats_per_minute(), 60);
        assert_eq!(info.tempo_changes, 1);
        assert_eq!(info.notes, 549);
        assert_eq!(info.end_tick, 53_760);
        assert_eq!(info.first_notes.len(), 8);
        assert_eq!(info.first_notes[0].to_string(), "ch0 C4(60) v90 @0");
        assert!(
            info.first_notes
                .array_windows::<2>()
                .all(|[earlier, later]| earlier.tick <= later.tick)
        );
        // A pass lasts the end tick at 384 ticks per second (140 s), at
        // 44 091.710 Hz.
        let expected = u64::from(info.end_tick) * 44_091_710 / 384_000;
        assert!(
            info.pass_frames().abs_diff(expected) <= 1,
            "{} frames, expected {expected}",
            info.pass_frames()
        );
        println!(
            "prelude: {} events, {} notes, end tick {}, {} frames a pass; first notes: {}",
            info.events,
            info.notes,
            info.end_tick,
            info.pass_frames(),
            NoteList(&info.first_notes)
        );
    }

    #[test]
    fn notes_show_name_octave_and_tick() {
        let note = Note {
            tick: 12,
            channel: 9,
            key: 60,
            velocity: 100,
        };
        assert_eq!(note.to_string(), "ch9 C4(60) v100 @12");
        let low = Note {
            tick: 0,
            channel: 0,
            key: 40,
            velocity: 127,
        };
        assert_eq!(
            NoteList(&[low, note]).to_string(),
            "ch0 E2(40) v127 @0, ch9 C4(60) v100 @12"
        );
    }

    #[test]
    fn position_follows_ticks_and_loops() {
        let info = info();
        let pass = info.pass_frames();
        assert_eq!(
            info.position(0),
            Position {
                pass: 0,
                tick: 0,
                frame: 0
            }
        );
        // One second in: 384 ticks.
        let second = info.position(44_092);
        assert_eq!((second.pass, second.tick), (0, 384));
        // Past the end the song starts over.
        let next = info.position(pass + 44_092);
        assert_eq!(next.pass, 1);
        assert!((383..=384).contains(&next.tick), "tick {}", next.tick);
        assert!(info.position(pass - 1).tick <= info.end_tick);
        assert_eq!(info.position(3 * pass + 5).pass, 3);
    }

    #[test]
    fn level_meter_peak_and_rms() {
        let mut meter = LevelMeter::EMPTY;
        assert_eq!((meter.peak(), meter.rms()), (0, 0));
        meter.add(&[[1000, -1000], [-1000, 1000]]);
        assert_eq!((meter.peak(), meter.rms()), (1000, 1000));
        meter.add(&[[i16::MIN, 0], [0, 0]]);
        assert_eq!(meter.peak(), 32_768);
        // sqrt((4 * 1000^2 + 32768^2) / 8) = sqrt(134 717 728), rounded down
        assert_eq!(meter.rms(), 11_606);
        let taken = meter.take();
        assert_eq!(taken.peak(), 32_768);
        assert_eq!(meter, LevelMeter::EMPTY);
    }
}

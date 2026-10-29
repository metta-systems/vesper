//! Host tests of the MIDI test player's library: the start-up lines' numbers
//! for `D_E1M1` as `just wood-midi` exports it, the song position against
//! the sequencer's own clock, and the level meter.

#[cfg(test)]
mod tests {
    use {
        midi_player::{LevelMeter, Note, NoteList, Position, SongInfo},
        room::engine::sound::mus2midi::mus2midi,
        std::{fs, path::Path},
        wood_synth::{OUTPUT_RATE, Song},
    };

    /// `D_E1M1` of the shareware IWAD as MIDI, converted as the game does.
    fn e1m1() -> Vec<u8> {
        let wad = fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../wood/wad/doom1.wad"),
        )
        .unwrap();
        let dword = |offset: usize| {
            usize::try_from(u32::from_le_bytes(
                wad[offset..offset + 4].try_into().unwrap(),
            ))
            .unwrap()
        };
        let record = (0..dword(4))
            .map(|entry| dword(8) + entry * 16)
            .find(|&record| wad[record + 8..record + 16].starts_with(b"D_E1M1\0"))
            .unwrap();
        let (position, size) = (dword(record), dword(record + 4));
        mus2midi(&wad[position..position + size]).unwrap()
    }

    fn info() -> SongInfo {
        SongInfo::of(&Song::parse(&e1m1()).unwrap(), OUTPUT_RATE)
    }

    #[test]
    fn e1m1_timing_and_first_notes() {
        let info = info();
        // mus2midi: 70 ticks per quarter at 120 bpm, one tick per MUS tick
        // (1/140 s), one tempo event at the start.
        assert_eq!(info.division, 70);
        assert_eq!(info.tempo, 500_000);
        assert_eq!(info.beats_per_minute(), 120);
        assert_eq!(info.tempo_changes, 1);
        assert!(info.notes > 1000, "{} notes", info.notes);
        assert_eq!(info.first_notes.len(), 8);
        assert!(
            info.first_notes
                .array_windows::<2>()
                .all(|[earlier, later]| earlier.tick <= later.tick)
        );
        // A pass lasts the end tick at 140 ticks per second, at
        // 44 091.710 Hz.
        let expected = u64::from(info.end_tick) * 44_091_710 / 140_000;
        assert!(
            info.pass_frames().abs_diff(expected) <= 1,
            "{} frames, expected {expected}",
            info.pass_frames()
        );
        println!(
            "D_E1M1: {} events, {} notes, end tick {}, {} frames a pass; first notes: {}",
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
        // One second in: 140 ticks.
        let second = info.position(44_092);
        assert_eq!((second.pass, second.tick), (0, 140));
        // Past the end the song starts over.
        let next = info.position(pass + 44_092);
        assert_eq!(next.pass, 1);
        assert!((139..=140).contains(&next.tick), "tick {}", next.tick);
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

# The MIDI synthesiser's test song and bank

The midi-synth session tests (`tests/session.rs`), midi-test's player
(`userspace/tests/audio/midi-player/tests/song.rs`) and the midi-test image
(`kernel/tests/midi-test/image.toml`) play one song with one bank:

| File | What | Source | License |
|---|---|---|---|
| `wtk1-prelude1.mid` | J. S. Bach, *Das Wohltemperierte Clavier I, Praeludium I*, BWV 846: 384 ticks per quarter, 60 bpm, 549 notes, 140 s | [Mutopia Project](https://www.mutopiaproject.org/cgibin/piece-info.cgi?id=5), Mutopia-2011/09/12-5, maintained by Tobias Erbsland; unchanged | Public Domain (CC0) |
| `wtk1-prelude1.sf2` | [GeneralUser GS](https://www.schristiancollins.com/generaluser) v2.0.3 trimmed to what the song plays: 1 preset (Grand Piano), 1 instrument, 17 samples | `GeneralUser-GS.sf2` from [its repository](https://github.com/mrbumpy409/GeneralUser-GS) at commit `97049183643d5fc5a9322a69c5b09efb667c6c3a`, 32 319 396 bytes, SHA-256 `9575028c7a1f589f5770fccc8cff2734566af40cd26ed836944e9a5152688cfe` | GeneralUser GS License v2.0: free to use, modify and redistribute |

## Making the bank again

The bank is the song's trim of GeneralUser GS. After
changing the song, or the trim, make it again on the host:

```sh
just _midi-test-soundfont              # the song in this directory
just _midi-test-soundfont other.mid    # copy other.mid over it first
```

The recipe downloads the full SoundFont once (from the pinned commit, into
`target/soundfonts/`, checked by its SHA-256) and trims it with `sf2-trim`
to the song.

`sf2-trim`'s `test_bank_is_trimmed_to_the_test_song` fails while the bank
and the song disagree. A new song or bank changes the start hash
`just _test-midi` checks: update `midi_test_hash` in the Justfile and
`start_hash_of_the_midi_test_song` in `tests/session.rs` together, and the
song's numbers in `_test-midi` and in midi-player's `tests/song.rs`.

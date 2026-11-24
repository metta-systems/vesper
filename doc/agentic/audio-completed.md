# Audio completed

Short markers of what is done. Open work stays in
[audio-implementation-plan.md](audio-implementation-plan.md); the design, with each
decision and the alternatives weighed, is in [audio-design.md](audio-design.md).

## Completed decisions

- [x] D1 to D8 (2026-10-27): the recommended option of each; `audio-design.md`, section
  Decisions.
- [x] D9 with D9.1 to D9.6 (2026-10-28): an integer SoundFont synthesiser in a separate
  music component, the SC-55 bank trimmed by a host tool, the full output rate, the
  renderer per platform; D9.2 and D9.3 do not apply. `audio-design.md`, sections
  Decisions and Music.
- [x] D10 (2026-10-28): Wood pulls the synthesiser component's `midi_synth_v1`, whose
  io-channel carries MIDI one way and rendered audio the other, mixes locally and stays
  the one `audio_v1` client. `audio-design.md`, section Decisions.
- [x] D9.7 (2026-11-21): an f64 engine behind `fp-mode`, chosen at run time, the
  integer-only build kept; `audio-design.md`, section Decisions.

## Completed work

- [x] Integer pitch fix: `phase_step` scales the whole 32.32 ratio, so pitches between
  octaves no longer play at the sample's root (44.1 kHz samples); `midi_test_hash`
  changed, Wood's hashes did not. Exercised by the `tables` host test and `just _test-midi`.
- [x] D9.7 f64 engine: `userspace/synth/src/float.rs`, the engine packet (command 7) of
  `midi_synth_v1`, the `midi-synth-fp` FP component (`fp_binaries`). Exercised by the
  fp-mode host tests (43.5 dB from the integer engine on the test song), `just
  _synth-fp-vectorized` and `just _test-midi-fp` (three runs, the host's hash each time).
- [x] Exact rate handoff: `audio_v1.rate` returns `(numerator, denominator)` Hz (the PWM
  clock over the range), `midi_synth_v1.open(numerator, denominator)` renders at it
  (`unsupported_rate` outside 8 kHz to 192 kHz), `midi_synth_v1.rate` and
  `vesper_synth::OUTPUT_RATE` are gone. Exercised by `just _test-vesper-synth` (rates,
  Room's music hashes), `just _test-audio`, `just _test-midi` and `just _test-wood`.

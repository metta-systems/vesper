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

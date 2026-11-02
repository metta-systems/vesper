# Audio open decisions

Parking lot for audio decisions the maintainer has not answered. Each entry stays open
until the maintainer answers it. Recommendations are marked as such; they are not
decisions. Record an answer only when the maintainer gave it explicitly; answered items
move to [audio-design.md](audio-design.md), and tasks stay in
[audio-implementation-plan.md](audio-implementation-plan.md).

Facts behind these questions (pins, registers, QEMU support, music research with
sources and measurements) are in the design doc.

D1 to D8 are answered (2026-10-27, the recommended option of each), D9 with D9.1 to
D9.6 (2026-10-28: an integer SoundFont synthesiser in a separate music component, the
SC-55 trimmed by a host tool, the full output rate, the renderer per platform; D9.2 and
D9.3 do not apply) and D10 (2026-10-28: Wood pulls the synthesiser component's
`midi_synth_v1`, whose io-channel carries MIDI one way and rendered audio the other,
mixes locally and stays the one `audio_v1` client), all recorded in
[audio-design.md](audio-design.md), section Decisions.

No audio decision is open.

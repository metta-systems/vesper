# Audio open decisions

Parking lot for audio decisions the maintainer has not answered. Each entry stays open
until the maintainer answers it. Recommendations are marked as such; they are not
decisions. Record an answer only when the maintainer gave it explicitly; answered items
move to [audio-design.md](audio-design.md), and tasks stay in
[audio-implementation-plan.md](audio-implementation-plan.md).

Facts behind these questions (pins, registers, QEMU support, music research with
sources and measurements) are in the design doc.

D1 to D8 are answered (2026-10-27, the recommended option of each) and D9 with D9.1 to
D9.6 (2026-10-28: an integer SoundFont synthesiser in a separate music component, the
SC-55 trimmed by a host tool, the full output rate, the renderer per platform; D9.2 and
D9.3 do not apply), all recorded in [audio-design.md](audio-design.md), section
Decisions. What remains:

## D10. How the music component's output reaches the jack (blocks A5c)

D9.1 put the synthesiser in a component of its own. `audio_v1` takes one client
(`open` answers `busy` to a second), and Wood already is that client for the sound
effects (D5: mixing in Wood's playback Thread). Some component has to add the two
streams before the audio driver.

- **a. Wood pulls music from the music component and mixes it.** The music component
  exports a `music_v1` interface: control calls (`play` with the MIDI bytes, `stop`,
  `pause`, `resume`, `set_volume`, `is_playing`) and an io-channel of rendered s16
  stereo buffers that Wood's playback Thread takes one per period, adds the effect
  voices on top of and sends on to `audio_v1` (one clip, one clock: the playback
  Thread's demand paces the renderer). Wood stays the only `audio_v1` client; no audio
  driver change. The engine Thread's `music_*` hooks become `music_v1` calls (or are
  handed to the playback Thread through atomics, as `SharedVoices` does, so the engine
  never waits on another component).
- **b. A mixing service** between the clients and the driver: exports `audio_v1` (or a
  `mixer_v1`) to any number of clients, mixes them and is the driver's one client. Wood
  and the music component are both its clients. The general answer (D5 named it future
  work), a new component and a hop for every stream.
- **c. The audio driver mixes several clients**: `audio_v1` accepts more than one
  `open`, the driver sums the queued streams while filling the ring. Puts mixing into
  the driver that D5 kept a plain PCM sink.

Recommendation: a. It needs only the new interface the separate component implies,
keeps D5's mixing in Wood, and gives the renderer the audio clock (it renders what the
playback Thread asks for); b when a second program wants to play sound.

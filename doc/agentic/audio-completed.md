# Audio completed

Short markers of what is done. Open work stays in
[audio-implementation-plan.md](audio-implementation-plan.md); the design, with each
decision and the alternatives weighed, is in [audio-design.md](audio-design.md).

## P-AUDIO-1: clock and a polled test tone

- [x] `drivers/cprman-bcm2835` exports `clock_v1` (`set_rate`, `enable`, `disable`, `rate`, `status`) for the PWM clock (binding 30), picking PLLD per or the 19.2 MHz oscillator within 0.1 % and programming §6.3 table 6-34's sequence (host tests `tests/cprman.rs`); the audio driver routes its pins through `gpio_v1` and sets the PWM clock to 100 MHz (AUDIO-2, AUDIO-4); the QEMU test tree `targets/overlays/audio-test-pwm.dts`; image `kernel/tests/audio-test`. `just _test-audio` (2026-10-27): 100 MHz from PLLD per / 5, an unknown clock and an unreachable rate refused.

## P-AUDIO-2: DMA

- [x] `drivers/dma-bcm2835` exports `dma_v1` (AUDIO-1: `claim`, `release`, `copy`, `start_ring`, `position`, `stop`, `errors`, an io-channel per client advancing once per finished control block), channels from `brcm,dma-channel-mask` (`DriverInit::dma_channel_mask`), host tests `tests/dma.rs`; Kickstart issues a repeated interrupt line once (`distinct_interrupts`); the audio driver imports `dma_v1`. `just _test-dma` (2026-10-27): mask `0x7f35`, 13 lines, both test copies arrive.

## P-AUDIO-3: audio driver and `audio_v1`

- [x] `audio_v1` (`badged;`, `channel 4;`, `rate`, `open`, `close`, `underruns`) served from a ring of 4 periods × 512 frames on DREQ 5 with ramps, s16 conversion and underrun silence (AUDIO-6, AUDIO-7; `src/stream.rs`, host tests `tests/stream.rs`); the test client `userspace/tests/audio/tone`; capture mode under `qemu`. `just _test-audio` (2026-10-28): both streams play as expected, `busy` and `not_open` refused.

## P-AUDIO-4: Wood sound effects

- [x] Room's `no_std` mixer (`room::engine::sound::mixer`: DMX lumps, eight voices in 32.32 fixed point, Chocolate Doom's pan law, `SharedVoices`, `SoundBank`; 14 host tests) and Wood's playback Thread (`userspace/wood/vesper/src/sound.rs`, AUDIO-5) as the one `audio_v1` client; wood-test with the gpio, cprman, dma and audio drivers (boot table 2048 entries); the driver's capture check opt-in by test pattern. `just _test-wood` (2026-10-28): 512 audible stream periods, 0 underruns.

## P-AUDIO-5: music

- [x] `userspace/synth` (`vesper-synth`, AUDIO-9): SoundFont reader, 16 channels, 32 voices, loops, volume envelope, controllers, exclusive classes, voice stealing, the MIDI sequencer; `sf2-trim` and `just _wood-soundfont` (AUDIO-17: `target/wood/sc55-doom1.sf2`, 38 presets); Room's opt-in `vesper-synth` feature (AUDIO-19). `just _test-vesper-synth` (2026-10-28): golden hashes, loudness within 2.5 dB of `rustysynth`; `just _synth-no-float`.
- [x] `userspace/midi-synth` exporting `midi_synth_v1` (AUDIO-10: `badged;`, `channel 16;`, `rate`, `open`, `close`, `service`, `playing`; host tests `tests/session.rs`) and Wood's `src/music.rs` pulling and mixing under the effects (`Mixer::mix_shared_onto`); render-split-independent ramps; wood-test bundling `midi_synth` with the SoundFont resource. `just _test-wood` (2026-10-28): the songs play with the host's hashes, 0 underruns.
- [x] Proposal 4 of [audio-profiling.md](audio-profiling.md), the bit-exact voice-loop rewrite (2026-10-29): 55.7 → 21.6 aarch64 instructions per voice-frame, the synthesiser −45 % on `midi-test`, hashes unchanged.
- [x] Integer pitch fix: `phase_step` scales the whole 32.32 ratio, so pitches between octaves no longer play at the sample's root (44.1 kHz samples); `midi_test_hash` changed, Wood's hashes did not. Exercised by the `tables` host test and `just _test-midi`.
- [x] AUDIO-20 f64 engine: `userspace/synth/src/float.rs`, the engine packet (command 7) of `midi_synth_v1`, the `midi-synth-fp` FP component (`fp_binaries`). Exercised by the fp-mode host tests (43.5 dB from the integer engine on the test song), `just _synth-fp-vectorized` and `just _test-midi-fp` (three runs, the host's hash each time).
- [x] Exact rate handoff: `audio_v1.rate` returns `(numerator, denominator)` Hz (the PWM clock over the range), `midi_synth_v1.open(numerator, denominator)` renders at it (`unsupported_rate` outside 8 kHz to 192 kHz), `midi_synth_v1.rate` and `vesper_synth::OUTPUT_RATE` are gone. Exercised by `just _test-vesper-synth` (rates, Room's music hashes), `just _test-audio`, `just _test-midi` and `just _test-wood`.

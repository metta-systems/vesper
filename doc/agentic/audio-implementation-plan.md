# Audio implementation plan

Design: [audio-design.md](audio-design.md) (with the answered decisions D1 to D9). Open
decisions: [audio-decisions.md](audio-decisions.md). This file holds only open work;
nothing in a stage starts until the decisions it lists are answered. Mark an item `- [x]`
only after its exit check has passed.

## Dependencies

- D1 to D8 are answered (2026-10-27): A1 to A4 are built. D8 (no special
  scheduling) is revisited if A4 shows underruns on hardware.
- Board status: the maintainer reported sound output on the board worked, 2026-10-28;
  detailed hardware exits not yet re-run (the checkboxes of A1 to A4 below stay open).
  The board is unavailable for now: work continues blind, with host tests and QEMU,
  and the board checks come later.
- D9 is answered (2026-10-28): an integer SoundFont synthesiser (A), in a separate music
  component (D9.1 b), the SC-55 trimmed by a host tool (D9.4 b), at the output rate
  (D9.5 a), the host keeping `rustysynth` (D9.6 a); D9.2 and D9.3 do not apply. D10 is
  answered the same day: the component exports `midi_synth_v1`, whose io-channel
  carries MIDI one way and rendered audio the other; Wood pulls, mixes and stays the one
  `audio_v1` client. A5b and A5c are built (host tests and QEMU).
- Existing pieces used: `gpio_v1` (pins), `libs/io-channel` (transport), Meddle
  `channel N;` provisioning in Kickstart, `Frame.CleanData`, `DriverInit::memory_bus`.

## A1: clock and a polled test tone

Done in QEMU (2026-10-27), hardware check pending:

- `drivers/cprman-bcm2835` for `brcm,bcm2835-cprman` exports `clock_v1` (`set_rate`,
  `enable`, `disable`, `rate`), serving the PWM clock (binding number 30) only. The
  library picks the source and integer divisor (PLLD per 500 MHz or the 19.2 MHz
  oscillator, within 0.1 %) and programs them with the sequence of §6.3 table 6-34;
  host tests `tests/cprman.rs` (the design table's rates, the write sequence, `BUSY`
  never written).
- `drivers/audio-pwm-bcm2835` for `brcm,bcm2835-pwm` (a skeleton until A3): routes the
  pins of its node's pin group through `gpio_v1`, sets the PWM clock to 100 MHz through
  `clock_v1` (linked by the node's `clocks`) and reads it back; on the board it sets
  `RNG1 = RNG2 = 2268`, both channels on the FIFO with `MSEN=0`, clears the FIFO and
  plays a polled 440 Hz sine on the left, the right and both sides, three rounds,
  traced on the UART with the underruns and `STA` errors of each side (replaced in A3 by
  the `audio_v1` test client). Under `qemu` it checks `clock_v1`'s refusals instead and
  never touches the PWM block. Host tests `tests/pwm.rs`.
- QEMU test tree: `targets/overlays/audio-test-pwm.dts`, applied to the rpi3 tree with
  `fdtoverlay` by `just test-audio` (needs `dtc` and `fdtoverlay`). Board:
  `dtoverlay=pwm-2chan,pin=40,func=4,pin2=41,func2=4` in `config.txt`; chainboot hands
  the firmware's tree to the kernel unchanged.
- Image `kernel/tests/audio-test` (gpio, cprman, audio with the serial channel, PL011,
  root dispatcher). QEMU exit passed: `just test-audio` (log in
  `target/audio-test.log`): the PWM clock reads back 100 MHz from PLLD per / 5, and an
  unknown clock and an unreachable rate are refused.

- [ ] Hardware exit (`just boot-audio-test` on the Pi 3 B with the overlay line): since
  A3 the polled tone is replaced by the `audio_v1` test client, so this is checked with
  A3's hardware exit: the tone is heard on the left, then the right, then both (pins,
  clock, range and the swap). Until then A1 stays here.
- [ ] Follow-up: Kickstart passes the clock number of the `clocks` link (the cell after
  the phandle), so `cprman` can refuse clocks the caller's node does not name; until
  then it serves the PWM clock only and the audio driver names clock 30 itself.

## A2: DMA

Done in QEMU (2026-10-27), hardware check pending:

- `drivers/dma-bcm2835` for `brcm,bcm2835-dma` exports `dma_v1` (D1;
  `interfaces/dma_v1.if`, design doc section `dma_v1`): `claim`, `release`, `copy`,
  `start_ring`, `position`, `stop`, `errors`, and an io-channel per client whose
  EventCount advances once per finished control block. Channels come from
  `brcm,dma-channel-mask`, which Kickstart now passes in `DriverInit::dma_channel_mask`
  (`libdevicetree::brcm_dma_channel_mask`), lowest first; `LITE` is read from `DEBUG`.
  Control blocks live in a page of the driver's own memory, eight slots per channel,
  cleaned before a channel loads them. The pure library has the typed `CS`, `TI` and
  `DEBUG` registers and the chain building; host tests `tests/dma.rs` (`TI`/`CS`/`DEBUG`
  bits, 32-byte alignment, linked and circular `NEXTCONBK`, Lite length limit, channel
  mask allocation, the interrupt order, counting finished blocks from `CONBLK_AD`).
- Kickstart issues each interrupt line of a node once, at its first position
  (`libdevicetree::interrupts::distinct_interrupts`, host test on the rpi3 tree: the DMA
  node's 16 entries give 13 lines, 27 once). Before, the repeated line was refused by
  `IRQControl.Issue` (`IrqLineActive`) and skipped with a "taken" message.
- `drivers/audio-pwm-bcm2835` imports `dma_v1` (through the controller's compatible
  string: the PWM node names no `dmas`). Under `qemu` it copies half a page into the
  other half with a chain of 4 linked blocks on channel 0, claims every other granted
  channel (0, 2, 4, 5, 8 to 14, then `no_channel`), copies on channel 14 (the line 11 to
  14 share), checks the bytes, one EventCount advance per block, `position` and `errors`,
  and the refusals. On the board, after the A1 tone, it plays the tone on both sides for
  5 s from a ring of 4 periods × 512 frames fed to `FIF1` by DREQ 5, refilling and
  cleaning each period behind the DMA, and traces the period rate measured against
  `CNTVCT_EL0`, its distance from 44 091.710 Hz, late refills, the `STA` flags and
  `dma_v1.errors` (in A3 the ring became the `audio_v1` output, which traces the same
  at each close). Host tests in `tests/pwm.rs` (ring constants, `DMAC`, rate
  tolerance).
- QEMU exit passed: `just test-dma` (log in `target/dma-test.log`, the audio-test image;
  in `just test`): the DMA driver gets mask `0x7f35` and 13 interrupt lines; channel 0's
  interrupt shows `INT_STATUS 0x0001`, `END` and `INT` set, 4 blocks finished; channel
  14's shows `INT_STATUS 0x4000` on the shared line; both copies arrive. `just
  test-audio` still passes on the same image.

- [ ] Hardware exit (`just boot-audio-test` on the Pi 3 B with the overlay line): since
  A3 the driver's own ring tone is replaced by the `audio_v1` test client, which plays
  from the same ring; checked with A3's hardware exit: the tone is heard without gaps,
  and each close traces a measured rate "within 0.1 %" of 44 091.710 Hz (this also
  checks the assumed 500 MHz PLLD), no late periods, no `WERR1`, `GAPO1`, `GAPO2` or
  `BERR`, and DMA errors 0. Until then A2 stays here.
- [ ] Follow-up: limit a client to the DREQs its node names (`dmas`) or its manifest
  grants; today any client may claim any DREQ of the driver's table (memory and PWM).

## A3: audio driver and `audio_v1`

Done in QEMU (2026-10-28), hardware check pending:

- `drivers/audio-pwm-bcm2835/interfaces/audio_v1.if` (crates `audio`, `audio-client`):
  `badged;`, `channel 4;`, `rate`, `open` (`busy`), `close` (`not_open`), `underruns`.
  Design doc section `audio_v1` has the channel semantics, the threading (one refill
  Thread; the calls hand over through atomics, a doorbell and a "closed" EventCount),
  the refill path and open/close (a period of ramp up; `close` plays out what is queued,
  at most 8 periods, then a period of ramp down from the last frame, and stops).
- The driver keeps the PWM and clock set-up, claims a DMA channel for DREQ 5 at start
  and runs a ring of 4 periods × 512 frames in its own memory while a client has the
  output open, refilling each period `dma_v1`'s EventCount reports finished, converting
  s16 stereo into FIFO words right first (range 2268), cleaning it, silence and an
  underrun count when too few frames are queued (D7). The pure part is
  `src/stream.rs` (`Stream`, `Ring`, conversion, ramps, the capture pattern); host tests
  `tests/stream.rs` (scaling and clipping, little-endian frames, channel order, packets
  in order across records and periods, completion timing, refusals, underrun counting,
  draining, cutting at close, ramps, the ring's refill and late counting). The A1 polled
  tone and the A2 ring tone are gone from the driver: the test client replaces them.
- Test client `userspace/tests/audio/tone` (`audio-test-client`, a program in the
  audio-test image importing `audio_v1`): on the board a 440 Hz tone, 3 s left, 3 s
  right, 3 s both, 60 s both; under `qemu` the capture test.
- Capture mode (`qemu`): each period is played by a one-block `dma_v1.copy` into a
  capture buffer, paced at the period's length; the driver checks every captured period
  against what filled it, stream periods against the client's pattern.
- QEMU exit passed: `just test-audio` (log in `target/audio-test.log`; in `just test`):
  the first stream (6 buffers of the pattern and a refused one, queued before `open`)
  gives "1 ramp up, 8 stream periods with 3072 client frames of the pattern in order, 2
  of them silent at the end, 1 ramp down", 7 packets back in order (the refused one
  empty), 2 underruns counted once the client stopped; the second (4 buffers, `open`
  then `close` at once) plays out 2048 frames with 0 underruns, every packet back whole;
  `busy` and `not_open` refused. `just test-dma` still passes on the same image.

- [ ] Hardware exit (`just boot-audio-test` on the Pi 3 B with the overlay line): the
  tone is heard left, then right, then both, then both for a minute with no gaps or
  clicks (open and close included), and every close traces 0 underruns, 0 late, no
  `STA` errors, DMA errors 0 and a rate "within 0.1 %". This also completes the hardware
  exits of A1 and A2.

## A4: Wood sound effects

Done in QEMU (2026-10-28), hardware check pending:

- Room: `room::engine::sound::mixer` (`no_std`, `userspace/wood/room/src/engine/sound/
  mixer.rs`): `dmx_samples` reads a DMX lump as the host's `decode_doom_sfx` does;
  `Mixer` plays eight voices, each stepped through its sound in 32.32 fixed point at
  the lump's rate over the output rate, linearly interpolated, with the gains of
  `gains` (the host's `gains_from` law in 16.16 fixed point), summed in 32 bits and
  clipped to s16 stereo. `SharedVoices` is the lock-free hand-over: per channel a
  command word (sequence number and sound), a levels word and a "finished" word, and a
  `SoundBank` that keeps one copy of each distinct sound for good (the engine's lumps
  live in its purgeable zone cache). The centre of `sep` is settled: the engine centres
  at 128 (`NORM_SEP`), the pan law stays Chocolate Doom's, symmetric about 127, so a
  centred sound plays at 126 : 128 (0.14 dB), as vanilla's linuxdoom mixer also leans
  right at 128 (`CENTRE_SEPARATION`). Host tests in the module (14: header, the rate
  ratio, a sound's length at the output rate, interpolation, the pan law against the
  host formula, volume, mixing and clipping, a voice's end, the bank, start, update,
  stop, restart and finish through `SharedVoices`); `cd userspace/wood/room && just
  test` passes (the demo playthrough, golden save and frame hashes included) and the
  library builds for `aarch64-unknown-none-softfloat`.
- Wood (`userspace/wood/vesper/src/sound.rs`): the platform's `sound_init` releases the
  playback Thread (`driver_threads!`, started by Kickstart, 16 KiB stack; the mixer
  writes straight into the channel's data pages) through Wood's `EventCount` and waits
  for its answer; that Thread alone calls `audio_v1`: `rate`, `open`, then it keeps four
  buffers of 512 frames queued, mixing the next into each one that comes back. No driver
  or `busy` (or no io-channel) logs once and the engine runs silent. `sound_shutdown`
  has it stop refilling and `close` right after the last packet, and waits for it.
  `sound_start`, `sound_stop`, `sound_update` and `sound_is_playing` only touch
  `SharedVoices`. Music came later, in A5c. Wood logs the buffers mixed, those with
  sound and the driver's underrun count at powers of two.
- wood-test: the gpio, cprman, dma and audio drivers (the compositor keeps the serial
  channel, so the audio driver traces on semihosting only); Wood imports `audio_v1`;
  `just test-wood` and `just qemu-wood` boot the audio test tree (`_audio-test-dtb`).
  The boot thread expects Wood's four `audio_v1` imports only when the PWM node is
  enabled, so a board without the overlay boots with Wood silent. Capacity: the image
  has 14 instances (`MAX_DRIVER_INSTANCES` raised from 12 to 16) and takes about 1230
  boot-table slots (each exporter's stack pool is 64 Frames), so the boot table has
  2048 entries (`BOOT_TABLE_SIZE_BITS` 11, 21-bit guards: the boot and test table
  guards changed). The alternative was 8 instead of 16 export stacks per exporter.
- The driver's capture check is opt-in by the client: a stream that starts with 16
  frames of `test_pattern` (`starts_with_test_pattern`) is checked against the pattern
  as before (`just test-audio` unchanged); any other stream's periods are checked
  against the ring slot they were copied from and counted when not silent
  (`is_audible`), traced at powers of two. Ramps and silence are always checked. Host
  tests in `tests/stream.rs`.
- QEMU exit passed: `just test-wood` (log in `target/wood-test.log`): Wood opens the
  output at 44 091.710 Hz, the driver starts its stream, the demo's effects reach the
  capture ("audio: capture: 512 audible stream periods of 1129", "wood: INFO audio:
  1126 buffers mixed, 512 with sound, 0 underruns"); the key checks pass as before.
  `just test-audio` and `just test-dma` still pass.

- [ ] Hardware exit (`just boot-wood-test` on the Pi 3 B with the overlay line in
  `config.txt` and headphones in the jack): Doom's effects heard during the demos, in
  the right places left and right, without clicks. The underrun count cannot be read
  there yet: Wood's log is semihosting only, and the audio driver's trace waits in its
  backlog because the compositor holds the UART; reporting it (for example through the
  compositor's serial channel) is open. If underruns show under load, D8 option B comes
  into play.

## A5: music

D9 and D10 answered 2026-10-28 (design doc, section Decisions): an integer SoundFont
synthesiser (`wood-synth`) in a separate component exporting `midi_synth_v1`, the SC-55
trimmed per IWAD, rendered at the output rate; Wood pulls the rendered music and mixes
it with the effects; the host keeps `rustysynth`.

### A5a: measure the budget on the board

- The MIDI synthesiser component logs the ARM clock (`mailbox_v1.get_clock_rate`, ARM clock) and times a
  fixed render workload per 512-frame buffer against `CNTVCT_EL0`: `wood-synth` holding
  24 voices of the trimmed bank, rendered into a scratch buffer, while the effects play
  as in A4. Wood already logs the time each `service` call takes (the render of one
  period plus the call), at powers of two of the periods with music; under QEMU that is
  130 to 300 µs, which says nothing about the board.
- Needs the board (unavailable for now) and a way to read the result there (Wood's log
  is semihosting only; the open "report through the compositor's serial channel" item of
  A4, or the audio test image's UART).
- Exit: the render share of a buffer's 11.6 ms and the ARM clock are recorded in the
  design doc; it fits with margin (below a quarter of the core at the clock measured), or
  D9.5 (half rate) is revisited with the number.

### A5b: the synthesiser and the trimmed bank (host-tested)

Done on the host (2026-10-28); runs on Vesper from A5c on. Design doc section "The
synthesiser as built (A5b)".

- `userspace/wood/synth` (`wood-synth`, `no_std` + `alloc`, in the Vesper workspace):
  SoundFont reader in place, zones with merged generators, 16 MIDI channels, 32 voices
  with 32.32 linear-interpolated playback, the loop modes, the volume envelope,
  velocity/volume/expression/pan/bend/sustain, exclusive classes, voice stealing, the
  MIDI file sequencer clocked by frames with looping and pause; tables from `build.rs`;
  no filter, LFOs, modulation envelope, modulators, chorus or reverb yet.
- `userspace/wood/tools/sf2-trim` and `just wood-soundfont`: the SC-55 trimmed to the
  shareware music into `target/wood/sc55-doom1.sf2` (4 299 638 bytes: 38 presets, 51
  instruments, 223 samples; 37 programs and 23 drum notes).
- Room: `mus2midi` is `pub`; the opt-in feature `wood-synth` renders the host's music
  with the new synthesiser (default stays `rustysynth`).
- Exits passed: `just test-wood-synth` (in `just test-host`): tables, zone selection,
  preset offsets, envelope stages, pitch (440 Hz at its root, octaves, bend and its
  range), loop modes, controllers, exclusive class and polyphony, event frames with a
  tempo change and across loops, pause, the MIDI reader, no float in the source; on the
  shareware data, the programs and drums found, the trimmed file's validity, `rustysynth`
  loading it, golden hashes of the first 10 s of `D_E1M1` and `D_INTRO` at the output
  rate, loudness against `rustysynth` (correlation 0.88 to 0.96, within 2.5 dB). `just
  synth-no-float` and `just clippy-wood-synth` pass.
- Board status of A5 as a whole: nothing of A5 has run on the board yet.

### A5c: the MIDI synthesiser component, `midi_synth_v1` and mixing in Wood

Done in QEMU (2026-10-28, D10), hardware check pending (A5d). Design doc section
"`midi_synth_v1` and the synthesiser component".

- `userspace/wood/midi-synth` (component `midi-synth`, instance `midi_synth`):
  `interfaces/midi_synth_v1.if` (crates `midi-synth-server`, `midi-synth-client`):
  `badged;`, `channel 16;`, `rate`, `open` (`busy`), `close` (`not_open`), `service`
  (`not_open`), `playing`. The io-channel carries both directions: song packets (MIDI
  bytes, several for a large song) from Wood, render packets that come back holding the
  next frames, and stop, pause, resume and volume packets, in one ordered FIFO; the
  synthesiser handles them during `service`, on Wood's Thread. The pure part (protocol,
  data page layout, `Session` over `wood-synth`, the start hash) is the crate's library,
  host-tested in `tests/session.rs`; the component carves a 2 MiB heap, loads the
  SoundFont resource and exports the interface.
- Wood: `src/music.rs`. The `music_*` platform methods queue commands (a spin-locked
  queue; the MIDI bytes copied into Wood's heap); the playback Thread sends them, then a
  render packet, calls `service` once per period, copies the rendered period into the
  audio buffer and mixes the effects on top with Room's new `Mixer::mix_shared_onto` (one
  clip, host test `voices_mix_onto_what_the_frames_hold`). `music_is_playing` follows the
  synthesiser's `playing`, asked every 32 periods. Without the synthesiser the music is
  silence and the commands are dropped.
- `wood-synth` change found on the way: the envelope and gain ramps move on a grid of 64
  frames counted from the reset, so the output no longer depends on how frames are split
  into render calls (host test `output_is_independent_of_the_render_split`; the A5b
  golden hashes were re-recorded).
- Image: `kernel/tests/wood-test/image.toml` bundles `midi_synth` (4 MiB RAM) with the
  `soundfont` resource `target/wood/sc55-doom1.sf2`; `build-wood` builds the component
  and runs `just wood-soundfont` first; the test kernel loads it (15 instances of 16).
- QEMU exit passed: `just test-wood` (log in `target/wood-test.log`): the synthesiser
  loads the 4 299 638-byte SoundFont, Wood takes it at 44 091 710 mHz, the title's
  `D_INTROA` (878 bytes of MIDI) and the first demo's `D_E1M5` (13 101 bytes, looping)
  play at volume 64, the first 64 periods of each hash as the host test computes them
  (`wood_music_hashes`), 1 024 of 1 035 periods pulled have music (the first 11 before
  the title song), 1 024 of 1 038 captured stream periods are audible, 0 underruns.
  `just synth-no-float` finds no soft-float routine in the linked component.

### A5d: hardware

- [ ] Hardware exit (`just boot-wood-test` on the Pi 3 B with the overlay line and
  headphones): the title music, then the demos' level music with the effects on top, no
  clicks or dropouts, the music looping; the render share per buffer as measured in A5a
  and no underruns (read as decided in A5a).

### A5e: follow-ups (not needed for the exit)

- A cheap integer low-pass filter (`initialFilterFc`) if listening shows the brighter
  instruments matter; the modulation envelope and vibrato LFO likewise.
- A registered or Doom II IWAD: `just wood-soundfont` with that IWAD (the tool takes
  `--wad`); GeneralUser GS trimmed the same way if images are to be published (D9.4 c).
- Listening comparison on the host with Room's `wood-synth` feature.

## Verification

- Host tests for each pure library (clock math, control blocks, sample conversion,
  mixer, the music synthesiser: `just test-wood-synth`).
- `just clippy-wood-synth` (the synthesiser and the MIDI synthesiser component for the
  target, with and without `qemu`, their host tests, the trimming tool; the known
  `metta-meddle` `map_unwrap_or` breakage stops it until fixed) and `just
  synth-no-float` (no soft-float routine in the library or the linked component).
- `just test-wood` for the music end to end in QEMU.
- QEMU tests per stage as above (`just test-audio` for A1 and A3, `just test-dma` for
  A2);
  `just test-mailbox`, `just test-driver`, `just test-wood` for regressions in shared
  code.
- `just clippy-audio`: the audio crates (with the `dma` driver, the interface crates and
  the audio test client) with and without `qemu`, and their host tests (`pwm`,
  `stream`).
- Hardware checks on the Pi 3 B per stage; QEMU cannot check pacing, rate, pops,
  levels or channel order.
- `just fmt-check` and focused clippy on the touched crates.

## Related

- [audio-decisions.md](audio-decisions.md): open decisions.
- [audio-design.md](audio-design.md): hardware facts with sources, architecture, QEMU
  support.
- [capabilities-decisions.md](capabilities-decisions.md): the isochronous scheduler in
  Future work.
- `userspace/wood/README.md`: Phase 3.

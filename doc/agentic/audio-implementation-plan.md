# Audio implementation plan

Design: [audio-design.md](audio-design.md), with the decisions AUDIO-1 to AUDIO-20;
no audio decision is open. This file holds only open work; mark an item `- [x]` only
after its exit check has passed, and move it to [audio-completed.md](audio-completed.md).
Done: P-AUDIO-1 to P-AUDIO-4 (the clock, DMA, the audio driver and Wood's sound
effects) and P-AUDIO-5 (music), each checked in QEMU; their board checks are below.

## Dependencies

- AUDIO-8 (no special scheduling) is revisited if P-AUDIO-8 shows underruns on
  hardware.
- Board status: the maintainer reported sound output on the board worked, 2026-10-28;
  the detailed hardware exits are not yet re-run. The board is unavailable for now:
  work continues blind, with host tests and QEMU, and the board checks come later.
- Existing pieces used: `gpio_v1` (pins), `libs/io-channel` (transport), Meddle
  `channel N;` provisioning in Kickstart, `Frame.CleanData`, `DriverInit::memory_bus`.

## P-AUDIO-6: the clock number from the `clocks` link

- [ ] Kickstart passes the clock number of the `clocks` link (the cell after the
  phandle), so `cprman` can refuse clocks the caller's node does not name; until then
  it serves the PWM clock only and the audio driver names clock 30 itself.

## P-AUDIO-7: DREQs per client

- [ ] Limit a client to the DREQs its node names (`dmas`) or its manifest grants;
  today any client may claim any DREQ of the driver's table (memory and PWM).

## P-AUDIO-8: the board checks of P-AUDIO-1 to P-AUDIO-4

- [ ] `just _boot-audio-test` on the Pi 3 B with the overlay line: the `audio_v1` test
  client's tone is heard left, then right, then both (pins, clock, range and the
  swap), then both for a minute with no gaps or clicks (open and close included), and
  every close traces 0 underruns, 0 late, no `STA` errors (no `WERR1`, `GAPO1`,
  `GAPO2` or `BERR`), DMA errors 0 and a measured rate "within 0.1 %" of 44 091.710 Hz
  (this also checks the assumed 500 MHz PLLD).
- [ ] `just boot-wood-test` on the Pi 3 B with the overlay line in `config.txt` and
  headphones in the jack: Doom's effects heard during the demos, in the right places
  left and right, without clicks. The underrun count reaches the UART through the log
  server ([logging-implementation-plan.md](logging-implementation-plan.md),
  P-LOGGING-1): Wood's `music: summary` line every 256 periods and the
  `audio: … underruns` reports. If underruns show under load, AUDIO-8's isochronous
  option comes into play.

## P-AUDIO-9: measure the music budget on the board

- The MIDI synthesiser component logs the ARM clock (`mailbox_v1.get_clock_rate`, ARM
  clock) and times a fixed render workload per 512-frame buffer against `CNTVCT_EL0`:
  `vesper-synth` holding 24 voices of the trimmed bank, rendered into a scratch buffer,
  while the effects play. Wood already logs the time each `service` call takes (the
  render of one period plus the call), at powers of two of the periods with music;
  under QEMU that is 130 to 300 µs, which says nothing about the board.
- Needs the board (unavailable for now). Reading the result there works since the log
  server (P-LOGGING-1): wood-test logs the ARM and core clocks at music start and the
  `service` time min/avg/max every 256 periods on the UART.
- [ ] Exit: the render share of a buffer's 11.6 ms and the ARM clock are recorded in the
  design doc; it fits with margin (below a quarter of the core at the clock measured), or
  AUDIO-18 (full rate) is revisited with the number.

## P-AUDIO-10: music on the board

- [ ] Hardware exit (`just boot-wood-test` on the Pi 3 B with the overlay line and
  headphones): the title music, then the demos' level music with the effects on top, no
  clicks or dropouts, the music looping; the render share per buffer as measured in
  P-AUDIO-9 and no underruns.
- Music alone, to debug it on the board: `just _boot-midi-test` (image
  `kernel/tests/midi-test`) plays its test song (Bach's Prelude in C, with GeneralUser
  GS trimmed to it, in `userspace/midi-synth/fixtures`; Doom's music stays with Wood in
  the vesper-doom submodule) looping at volume 64 through the MIDI synthesiser and the
  jack, with nothing else running (no Wood, effects, compositor or USB). Its player
  (`userspace/tests/audio/midi-player`) logs Wood's music summary every 256 periods plus
  the level, the synthesiser's peak voices (`midi_synth_v1.voices`) and the song
  position. `just _test-midi` runs it under QEMU: the start hash as on the host
  (`midi_test_hash`), nearly every period audible, 0 underruns.

## Future work

Not needed for any exit.

- **N-AUDIO-1: a cheap integer low-pass filter** (`initialFilterFc`) if listening shows
  the brighter instruments matter; the modulation envelope and vibrato LFO likewise.
- **N-AUDIO-2: other IWADs.** A registered or Doom II IWAD: `just _wood-soundfont` with
  that IWAD (the tool takes `--wad`); GeneralUser GS trimmed the same way if images are
  to be published (AUDIO-17's alternative).
- **N-AUDIO-3: a listening comparison** on the host with Room's `vesper-synth` feature.
- **N-AUDIO-4: the remaining performance proposals** of
  [audio-profiling.md](audio-profiling.md) (2026-10-29): Wood pre-filling its queue
  before `open` and a deeper queue (since done as AUDIO-14 and AUDIO-15), dispatcher
  changes (an AUDIO-8 revisit). The synthesiser costs about 25 aarch64 instructions per
  voice-frame; Wood's 20–31 ms service maxima are round-robin preemption (a 10 ms
  `SLICE_NS` and three CPU-bound peers), not render work.

## Verification

- Host tests for each pure library (clock math, control blocks, sample conversion,
  mixer, the music synthesiser: `just _test-vesper-synth`).
- `just _clippy-vesper-synth` (the synthesiser and the MIDI synthesiser component for the
  target, with and without `qemu`, their host tests, the trimming tool; the known
  `metta-meddle` `map_unwrap_or` breakage stops it until fixed) and `just
  _synth-no-float` (no soft-float routine in the library or the linked component).
- `just _test-wood` for the music end to end in QEMU.
- `just _test-audio` and `just _test-dma`; `just _test-mailbox`, `just _test-driver`,
  `just _test-wood` for regressions in shared code.
- `just _clippy-audio`: the audio crates (with the `dma` driver, the interface crates and
  the audio test client) with and without `qemu`, and their host tests (`pwm`,
  `stream`).
- Hardware checks on the Pi 3 B; QEMU cannot check pacing, rate, pops, levels or
  channel order.
- `just fmt-check` and focused clippy on the touched crates.

## Related

- [audio-design.md](audio-design.md): decisions, hardware facts with sources,
  architecture, QEMU support.
- [scheduling-decisions.md](scheduling-decisions.md): the isochronous scheduler in
  Future work.
- `userspace/wood/README.md`: Phase 3.

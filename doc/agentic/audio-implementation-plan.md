# Audio implementation plan

Design: [audio-design.md](audio-design.md) (with the answered decisions D1 to D8). Open
decisions: [audio-decisions.md](audio-decisions.md). This file holds only open work;
nothing in a stage starts until the decisions it lists are answered. Mark an item `- [x]`
only after its exit check has passed.

## Dependencies

- D1 to D8 are answered (2026-10-27): A1 to A4 are unblocked. D8 (no special
  scheduling) is revisited if A4 shows underruns on hardware.
- D9 blocks A5; it is decided after A4.
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

- Room: a `no_std` mixer (DMX decode, eight voices, fixed-point resampling to the
  reported rate, `gains_from` panning, clipping), with host tests on synthetic lumps.
  Settle the centre value of `sep` (127 or 128).
- Wood: a playback Thread waiting on the `audio_v1` channel EventCount, the game Thread
  updating voices lock-free; `Platform::sound_init`, `sound_start`, `sound_stop`,
  `sound_update`, `sound_is_playing` in `userspace/wood/vesper/src/platform.rs`; the
  `audio_v1` import in Wood's `imports.if`; the driver in the wood-test image.
- QEMU exit: `just test-wood` sees mixed, non-silent buffers reach the capture buffer
  when the demo plays sounds.
- Hardware exit: Doom's effects heard during a demo; underruns counted and reported. If
  they are not zero under load, D8 option B comes into play.

## A5: music (later)

- Per D9: the synthesiser, the SoundFont or instrument resource, music voices in the
  mixer, `music_*` platform methods. Measure the render cost per second of audio.
- Exit: E1M1 music plays with effects on hardware without underruns.

## Verification

- Host tests for each pure library (clock math, control blocks, sample conversion,
  mixer).
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

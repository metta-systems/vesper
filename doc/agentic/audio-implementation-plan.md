# Audio implementation plan

Design: [audio-design.md](audio-design.md). Open decisions:
[audio-decisions.md](audio-decisions.md). This file holds only open work; nothing in a
stage starts until the decisions it lists are answered. Mark an item `- [x]` only after
its exit check has passed.

The plan follows the recommended options; a different answer changes the stages it
blocks.

## Dependencies

- D2, D3 and D4 block A1.
- D1 blocks A2 and A3. D6 and D7 block A3.
- D5 blocks A4; D8 decides whether A4 needs the isochronous scheduler on hardware.
- D9 blocks A5.
- Existing pieces used: `gpio_v1` (pins), `libs/io-channel` (transport), Meddle
  `channel N;` provisioning in Kickstart, `Frame.CleanData`, `DriverInit::memory_bus`.

## A1: clock and a polled test tone

- `cprman` driver for `brcm,bcm2835-cprman` exporting `clock_v1` (`set_rate`, `enable`,
  `disable`), with the `BUSY`/password sequence of the BCM2835 datasheet §6.3 (design doc, PWM clock); a pure library for the
  divider and range math (host tests: the table in the design doc).
- `audio` driver skeleton for `brcm,bcm2835-pwm`: pins from `pinctrl-0` through
  `gpio_v1`, clock through `clock_v1`, `RNG1 = RNG2`, `CTL` for both channels on the
  FIFO, a polled 440 Hz sine (left, right, both).
- A device tree with the PWM node enabled for QEMU, and the `config.txt` overlay line for
  the board (D3).
- QEMU exit: a new `just test-clock` recipe sets the PWM clock on QEMU's CPRMAN model and reads back the
  source and divider; the audio driver loads and does not touch the PWM registers.
- Hardware exit: the tone is heard on the side it should be, `STA` shows no `WERR1`,
  `BERR` or gaps while the FIFO is kept full.

## A2: DMA

- `dma` driver for `brcm,bcm2835-dma` exporting `dma_v1` (D1): channel allocation from
  `brcm,dma-channel-mask`, `LITE` read from `DEBUG`, control block building in a pure
  library (host tests: `TI` bits, 32-byte alignment, `NEXTCONBK` ring, Lite length limit).
- Kickstart: the DMA node lists line 27 four times; merge duplicate interrupt lines
  before issuing `IRQHandler`s, or document that only channels with their own line are
  used.
- QEMU exit: a finite memory-to-memory chain (DREQ 0) copies the expected bytes, raises
  the channel interrupt for each `INTEN` block, sets `INT_STATUS` and `CS.END`. No
  circular chain in QEMU: its DMA model runs a chain to the end inside the starting
  write, so a ring hangs it.
- Hardware exit: the A1 tone played from a ring of 4 periods through `FIF1` with DREQ 5;
  the period interrupt rate measured against `CNTVCT_EL0` is within 0.1 % of the
  computed rate (this also checks the assumed 500 MHz PLLD).

## A3: audio driver and `audio_v1`

- Write `audio_v1.if` (draft in the design doc): `channel`, `rate`, `open`, `close`,
  `underruns`.
- The ring in the driver's own memory, `CleanData` per period, the s16-to-FIFO-word
  conversion with the left/right swap, silence on underrun (D7), the open/close ramp.
- A capture test mode: the chain targets a RAM buffer instead of `FIF1` and runs finite,
  so QEMU can check the words.
- QEMU exit: a test client submits known samples; the captured words equal the
  converted samples in the right order, packets come back, the EventCount advances, an
  underrun is counted when the client stops.
- Hardware exit: a test client's tone through the interface with no gaps or clicks for
  a minute; underrun count zero.

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
- QEMU tests per stage as above; `just test-mailbox`, `just test-driver`, `just
  test-wood` for regressions in shared code.
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

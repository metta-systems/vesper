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
  traced on the UART with the underruns and `STA` errors of each side. Under `qemu` it
  checks `clock_v1`'s refusals instead and never touches the PWM block. Host tests
  `tests/pwm.rs`.
- QEMU test tree: `targets/overlays/audio-test-pwm.dts`, applied to the rpi3 tree with
  `fdtoverlay` by `just test-audio` (needs `dtc` and `fdtoverlay`). Board:
  `dtoverlay=pwm-2chan,pin=40,func=4,pin2=41,func2=4` in `config.txt`; chainboot hands
  the firmware's tree to the kernel unchanged.
- Image `kernel/tests/audio-test` (gpio, cprman, audio with the serial channel, PL011,
  root dispatcher). QEMU exit passed: `just test-audio` (log in
  `target/audio-test.log`): the PWM clock reads back 100 MHz from PLLD per / 5, and an
  unknown clock and an unreachable rate are refused.

- [ ] Hardware exit (`just boot-audio-test` on the Pi 3 B with the overlay line): the
  tone is heard on the side the trace names, and every side traces "no underruns, no
  errors" (no `WERR1`, `BERR` or FIFO underruns while it is kept full). Until then A1
  stays here.
- [ ] Follow-up: Kickstart passes the clock number of the `clocks` link (the cell after
  the phandle), so `cprman` can refuse clocks the caller's node does not name; until
  then it serves the PWM clock only and the audio driver names clock 30 itself.

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
- QEMU tests per stage as above (`just test-audio` for A1); `just test-mailbox`, `just
  test-driver`, `just test-wood` for regressions in shared code.
- `just clippy-audio`: the audio crates with and without `qemu`, and their host tests.
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

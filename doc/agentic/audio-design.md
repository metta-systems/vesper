# Audio design (Raspberry Pi 3, PWM jack output with DMA)

Status: D1 to D8 answered 2026-10-27 (section Decisions below); stage A1 (the `cprman`
driver with `clock_v1` and a polled test tone) is built and checked in QEMU, its
hardware check is pending. The one open decision (D9, music) is in
[audio-decisions.md](audio-decisions.md). Stages are in
[audio-implementation-plan.md](audio-implementation-plan.md). Interfaces named here are
drafts until written; once written they are prototypes and are edited in place.

## Decisions

Answered by the maintainer on 2026-10-27, each with the option the decisions file
recommended. The options that were weighed are summarised after each answer.

- **D1. Who programs the DMA controller: a separate `dma` driver** for the
  `brcm,bcm2835-dma` node exporting `dma_v1`, the sole holder of the DMA register page and
  the channel interrupts. The audio driver names its ring by bus address for now (B1a);
  lending Frames checked with `Frame.GetAddress` (B1b) comes when an untrusted client
  needs DMA. Not chosen: the audio driver owning the DMA page itself; a `dma` driver
  handing out one channel's registers (impossible, the channels share one page).
- **D2. How the PWM clock is set: a `cprman` driver** for `brcm,bcm2835-cprman` exporting
  `clock_v1`, linked from the PWM node's `clocks` property. Not chosen: a typed
  `mailbox_v1.set_clock_rate` (undocumented whether the firmware programs the PWM clock);
  the audio driver mapping the CPRMAN pages (every clock generator and PLL).
- **D3. The audio driver binds to the PWM node** (`brcm,bcm2835-pwm`). On the board the
  node is enabled with `dtoverlay=pwm-2chan,pin=40,func=4,pin2=41,func2=4` in
  `config.txt`; QEMU uses a test device tree with the node enabled. Not chosen: the
  firmware audio node (no `reg`); a program with fixed addresses.
- **D4. 44.1 kHz nominal from 100 MHz** (PLLD per / 5, integer divider), `RNG` 2268
  (44 092 Hz, about 11 bits), `MSEN=0`. If 11 bits sound noisy on hardware, 250 MHz
  (PLLD per / 2, `RNG` 5669) is the next thing to try. Not chosen: 22.05 kHz (audible
  carrier); exact 44 100 Hz with a MASH divider (jitter).
- **D5. Mixing in Wood's playback Thread**: a no_std mixer in Room, run by a second Wood
  Thread; the driver is a plain PCM sink. A mixing service component stays future work.
- **D6. An io-channel of signed 16-bit interleaved stereo** declared in `audio_v1`; the
  driver copies and converts into its DMA ring. Not chosen: zero copy into the ring.
- **D7. Underrun: mid-scale silence and a counter**; the DMA ring keeps running. Not
  chosen: repeating the last period; stopping the DMA.
- **D8. No special scheduling at first**: a ring of 4 periods and an ordinary Time
  budget, underruns measured on hardware; the isochronous scheduler only if they show.
- **D9. Music** is decided after the sound effects (A4); see
  [audio-decisions.md](audio-decisions.md).

Goal: sound effects (and later music) for Wood, the Room Doom port
(`userspace/wood/README.md`, Phase 3: PWM DMA audio, SoundFont MIDI player, mixer,
isochronous playback thread), through the 3.5 mm jack of a Raspberry Pi 3.

## Sources

Hardware documents first. Secondary sources are used only where the primary ones are
silent, and are marked as secondary where they are cited.

Primary:

- **[P1]** Broadcom, *BCM2835 ARM Peripherals*, 6 February 2012,
  <https://datasheets.raspberrypi.com/bcm2835/bcm2835-peripherals.pdf> (now redirects to
  `pip.raspberrypi.com/documents/RP-008249-DS-bcm2835-peripherals.pdf`). Sections used:
  §1.2.3 and §1.2.4 (ARM physical and bus addresses, footnote 1 on the L2 cache), §4.1,
  §4.2, §4.2.1.1 (control blocks), §4.2.1.2 (register map, TI fields, `INT_STATUS`
  `0xfe0`, `ENABLE` `0xff0`), §4.2.1.3 (DREQ table), §4.5 (DMA Lite engines), §6.2
  (alternate functions), §6.3 (general purpose clocks, tables 6-32 to 6-35), chapter 9
  (PWM, §9.1 to §9.6).
- **[P2]** Raspberry Pi 3 Model B reduced schematic, rev 1.2,
  <https://datasheets.raspberrypi.com/rpi3/raspberry-pi-3-b-reduced-schematics.pdf>,
  audio block (U11, R16 to R19, C58 to C62, R60, R61).
- **[P3]** The device tree in the repository, `targets/bcm2710-rpi-3-b-plus.dtb`
  (decompiled with `dtc`): nodes `pwm@7e20c000`, `dma@7e007000`, `cprman@7e101000`,
  `audio_pins`, `bcm2835_audio`. And, for the Pi 3 B, `bcm2710-rpi-3-b.dts` in
  <https://github.com/raspberrypi/linux> (branch `rpi-6.6.y`), node `audio_pins`.
- **[P4]** Raspberry Pi documentation on the BCM2837: the peripherals are those of the
  BCM2835 with the ARM physical base at `0x3F00_0000` instead of `0x2000_0000`
  (<https://www.raspberrypi.com/documentation/computers/processors.html>).

Secondary (implementations, not specifications):

- **[S1]** Linux `drivers/clk/bcm/clk-bcm2835.c`: `CM_PWMCTL` `0x0a0`, `CM_PWMDIV`
  `0x0a4`, 12 fractional divider bits. NetBSD `sys/arch/arm/broadcom/bcm2835_cm.h`: the
  same offsets, PLLD per as 500 MHz.
- **[S2]** QEMU 11.1.2 (installed) and its sources: `hw/dma/bcm2835_dma.c`,
  `hw/arm/bcm2835_peripherals.c`, `include/hw/misc/bcm2835_cprman_internals.h`.
- **[S3]** Circle (<https://github.com/rsta2/circle>), `lib/sound/pwmsoundbasedevice.cpp`
  and `lib/machineinfo.cpp`: a working bare-metal PWM DMA sound driver.
- The Broadcom note *BCM2835 Audio & PWM clocks* (G.J. van Loo, 2013) is the vendor's
  description of the PCM and PWM clock registers. It is referenced by the community
  datasheet errata page (elinux.org, which refused automated access); it was not
  retrieved for this design. The register layout below rests on [P1] §6.3 plus [S1] and
  [S2], which agree with each other.

## Hardware summary

### The jack

- Pins. GPIO40 ALT0 is PWM0 and GPIO41 ALT0 is PWM1 ([P1] §9.5 and §6.2). The Pi 3 B
  and the Pi 3 B+ both route the jack from GPIO40 and GPIO41 (`audio_pins`:
  `brcm,pins = <40 41>`, `brcm,function = <4>`, which is ALT0 in the pin controller's
  numbering; [P3] for both boards). GPIO45 (also PWM1 on ALT0) was the right channel on
  the earlier B+ and Pi 2 boards, not on the Pi 3.
- Left and right are swapped relative to the channel numbers. On the Pi 3 B schematic
  ([P2]) `AUD_PWM1` drives buffer input A1, whose output goes to `AUDIO_L`, and
  `AUD_PWM0` drives A2, whose output goes to `AUDIO_R`. So **PWM channel 2 (GPIO41) is
  left and PWM channel 1 (GPIO40) is right**. Circle agrees: its `ArePWMChannelsSwapped()`
  is true for the 3 B and 3 B+ ([S3], secondary). The Pi 3 B+ schematic was not checked.
  The first hardware test plays one side at a time to confirm.
- Output stage ([P2]). U11 is an NC7WZ16 dual buffer powered from a separate `AUD_3V3`
  rail. Each side then has 220 Ω in series, 100 nF and 100 Ω to ground, a 47 µF coupling
  capacitor and 1.8 kΩ to ground at the jack. Worked out from those values (not
  measured): the low-pass corner is about 23 kHz (220 Ω ∥ 100 Ω = 68.75 Ω with 100 nF),
  and a full-scale signal is about 1 V peak-to-peak unloaded (3.3 V × 100/320), roughly a
  third of that into 32 Ω headphones. The filter is first order, so a PWM carrier at
  44.1 kHz is only about 6 dB down at the jack. That is out of hearing range, but a
  carrier at 11 or 22 kHz would be audible or close to it.

### The PWM block ([P1] chapter 9)

- Base: bus address `0x7E20_C000` (ARM physical `0x3F20_C000` on the BCM2837, [P4]);
  the datasheet gives only offsets, and the base comes from the device tree node
  `pwm@7e20c000` ([P3]).
- Registers (§9.6): `CTL` `0x00`, `STA` `0x04`, `DMAC` `0x08`, `RNG1` `0x10`, `DAT1`
  `0x14`, `FIF1` `0x18`, `RNG2` `0x20`, `DAT2` `0x24`.
- `CTL` bits: `PWEN1` 0, `MODE1` 1, `RPTL1` 2, `SBIT1` 3, `POLA1` 4, `USEF1` 5, `CLRF1` 6,
  `MSEN1` 7, and the same for channel 2 at 8 to 13 and `MSEN2` at 15.
- `STA` bits: `FULL1` 0, `EMPT1` 1, `WERR1` 2, `RERR1` 3, `GAPO1..4` 4 to 7, `BERR` 8,
  `STA1..4` 9 to 12. The error and gap flags are cleared by writing 1.
- `DMAC`: `ENAB` 31, `PANIC` 15:8 (default 7), `DREQ` 7:0 (default 7), both FIFO-level thresholds.
- One FIFO of eight 32-bit words (§9.1) feeds both channels. With `USEF` set on both, the
  words written to `FIF1` alternate between the channels: channel 1 takes words 0, 2, 4…
  and channel 2 words 1, 3, 5… Both channels must then use the same range, and
  `RPTL` must be 0 on both (§9.6, `FIF1`). After the set of channels sharing the FIFO
  changes, clear it (`CLRF1`).
- Modes (§9.4). PWM mode with `MSEN=0` (the default) spreads N high cycles evenly over a
  period of M = `RNG` cycles (the algorithm of §9.3), which pushes the carrier energy
  up towards the clock rate. `MSEN=1` sends one pulse of N cycles per period
  (classic mark/space). The serialiser mode (`MODE=1`) shifts out the word's bits and is
  not useful here. Circle uses `MSEN=0` ([S3]).
- DMA pacing. "PWM DMA is mapped to DMA channel 5" (§9.5) means DREQ 5 in the DMA
  controller's peripheral map (§4.2.1.3), the `PERMAP` value of the control block.
- Datasheet slips seen while reading (not in the errata page because it was not
  retrieved): in the `CTL` table bits 8 to 13 are labelled "Channel 1" but belong to
  channel 2; `CLRF1` is typed `RO` but is a write-one action; §9.4 refers to "section
  1.3" for the algorithm, which is §9.3.

### Sample rate and resolution

In PWM mode a channel takes one FIFO word per `RNG` clock cycles. So the sample rate is
`fs = f_pwm / RNG`, and a sample is a value from 0 to `RNG`, about `log2(RNG)` bits. A
stereo frame is two FIFO words: right (channel 1) first, then left (channel 2).

The PWM clock is "nominally 100 MHz, but can be varied by the clock manager" (§9.1).
Candidate setups, all with an integer divider (no MASH jitter):

| Clock | Source / divider | 44.1 kHz | 22.05 kHz | 11.025 kHz |
|---|---|---|---|---|
| 19.2 MHz | oscillator / 1 | `RNG` 435 → 44 138 Hz, 8.8 bits | 871 → 22 044 Hz, 9.8 bits | 1741 → 11 028 Hz, 10.8 bits |
| 100 MHz | PLLD per (500 MHz) / 5 | 2268 → 44 092 Hz, 11.1 bits | 4535 → 22 051 Hz, 12.1 bits | 9070 → 11 025.4 Hz, 13.1 bits |
| 250 MHz | PLLD per / 2 (Circle's choice on Pi ≤ 3, [S3]) | 5669 → 44 100.2 Hz, 12.5 bits | 11338 → 22 049.7 Hz | 22676 → 11 024.9 Hz |

The carrier is at the sample rate, so 11.025 kHz and 22.05 kHz outputs put it in or at
the edge of the audible band, above the jack filter's 23 kHz corner only at 44.1 kHz.
Doom's 11 025 Hz effects are therefore upsampled by the mixer (×4 to 44.1 kHz), not
played at their native rate. An exact 44 100 Hz is possible with a fractional (MASH)
divider, at the cost of the divider jitter §6.3 describes; a 0.02 % rate error is
inaudible, and the mixer resamples to whatever rate the driver reports. Clock and rate
are D4: 100 MHz and `RNG` 2268.

PLLD per = 500 MHz on the Pi 3 is from [S1] and Circle; the datasheet does not give PLL
frequencies. Stage A2 measures the real rate (period interrupts against the generic
timer), so a wrong assumption shows up as a rate error.

### The PWM clock ([P1] §6.3, [S1], [S2])

- The PWM clock generator is in the clock manager ("CPRMAN", `cprman@7e101000`, two
  pages, [P3]), at `CM_PWMCTL` `0x7E10_10A0` and `CM_PWMDIV` `0x7E10_10A4`. §6.3 documents
  only the general purpose clocks `CM_GP0..2`; the PWM registers have the same format
  according to [S1] and [S2]:
  - `CTL`: `PASSWD` 31:24 (must be `0x5a` on every write), `MASH` 10:9, `FLIP` 8, `BUSY` 7,
    `KILL` 5, `ENAB` 4, `SRC` 3:0 (0 GND, 1 oscillator 19.2 MHz, 2 and 3 test, 4 PLLA per,
    5 PLLC per, 6 PLLD per, 7 HDMI auxiliary, 8 to 15 GND; Table 6-34).
  - `DIV`: `PASSWD` 31:24, `DIVI` 23:12, `DIVF` 11:0 (Table 6-35). Table 6-32 divides
    `DIVF` by 1024 although the field is 12 bits; Linux uses 4096 ([S1]).
  - Sequence (Table 6-34's rules): clear `ENAB`, wait for `BUSY` to clear, write `DIV`,
    write `SRC` and `MASH` with `ENAB` clear, then set `ENAB`. Never change source, divider
    or MASH while `BUSY` is set.
- The same pages hold every other clock generator (UART, EMMC, PCM, the GP clocks) and,
  at `0x7E10_2000`, the PLL controls. Whoever maps them can stop the UART or the SD
  card. That is why the clock is not simply given to the audio driver (D2).
- The firmware alternative. `mailbox_v1` already names a `pwm` clock (firmware clock id
  10) and has `get_clock_rate`. Setting it would need `SET_CLOCK_RATE` (`0x0003_8002`),
  which `drivers/mailbox` refuses on purpose ("tags that change clocks or power, which
  have typed functions or wait for per-client policy"), so it would be a new typed
  function. Whether the firmware actually programs the PWM clock on that request is not
  documented: Linux drives the PWM clock through CPRMAN directly ([S1]), not through the
  firmware. It would have to be tried on hardware, and QEMU cannot show it.
- The capability contract already expects a `cprman` component as a firmware-service
  component (contract, device memory section: "later `cprman`, `axiperf` and `vcsm`").

### The DMA controller ([P1] chapter 4)

- 16 channels. Channels 0 to 14 are at bus `0x7E00_7000 + 0x100 × n`, all in one 4 KiB
  page together with the global `INT_STATUS` (`0xfe0`) and `ENABLE` (`0xff0`); channel 15
  is at `0x7EE0_5000` (§4.2). One page holding every channel means a component that
  maps it can program every channel (D1).
- Which channels the ARM may use: the device tree's `brcm,dma-channel-mask = <0x7f35>`
  ([P3]): channels 0, 2, 4, 5, 8 to 14. The firmware uses the others. The firmware
  may edit the mask at boot, so it is read from the tree, not hard-coded. Circle uses
  `0x1f35` on Pi 1 to 3 (it leaves 13 and 14 alone; [S3]).
- Lite channels (§4.5): half the bandwidth, no 2D mode, a 16-bit length (65 536 bytes
  per control block). Which channels are Lite is not listed in §4.5; the `DEBUG`
  register's `LITE` bit (28) says so per channel at run time (Linux and Circle treat 7 to
  14 as Lite). A Lite channel is plenty for 353 KB/s of PWM words.
- Interrupts ([P3]): channels 0 to 10 have their own lines (GPU interrupts 16 to 26);
  channels 11 to 14 share line 27; line 28 is "dma-shared-all". The node lists line 27
  four times, which matters for Kickstart (see Risks).
- Control block (§4.2.1.1): eight words, 32-byte aligned: `TI`, `SOURCE_AD`, `DEST_AD`,
  `TXFR_LEN`, `STRIDE`, `NEXTCONBK`, two reserved. The channel is started by writing the
  block's bus address to `CONBLK_AD` and setting `CS.ACTIVE`. When a block finishes the
  channel loads `NEXTCONBK`; a zero there stops it. `NEXTCONBK` may be changed only
  while the channel is paused.
- `TI` fields for memory-to-PWM (§4.2.1.2): `INTEN` 0, `WAIT_RESP` 3, `DEST_INC` 4 (clear:
  the FIFO address is fixed), `DEST_DREQ` 6, `SRC_INC` 8, `PERMAP` 20:16 = 5,
  `NO_WIDE_BURSTS` 26. §4.2.1.3: writes to a peripheral must be paced by DREQ, and
  several writes can be in flight before the peripheral drops DREQ; `WAIT_RESP` keeps one
  write in flight, which is the safe setting for an eight-word FIFO with DREQ threshold 7.
  Circle uses `DMAC = ENAB | PANIC 7 | DREQ 7` ([S3]).
- Bus addresses (§1.2.3, §1.2.4): DMA uses bus addresses. Peripherals are at
  `0x7Ennnnnn`; RAM is seen at the `0xC000_0000` alias, which bypasses the VideoCore L2
  cache (footnote 1: "accesses to memory are routed either via or around the L2 cache
  depending on senior two bits of the bus address"). The repo's device tree says the same
  in `/soc`'s `dma-ranges = <0xc0000000 0x0 0x3f000000>`, and Kickstart already passes a
  driver the bus address of its memory (`DriverInit::memory_bus`).
- Coherency: the DMA controller does not snoop the Cortex-A53 caches. Samples and control
  blocks written through a cacheable mapping must be cleaned to the point of coherency
  before the DMA reads them. In Vesper that is `Frame.CleanData` (contract: Frame cache
  maintenance, 2026-10-12), as the USB host controller does. RAM Frames map `Cacheable`
  only (contract: Frame memory attributes), so the non-cacheable route is closed; one
  `CleanData` per period (about 86 a second at 512 frames per period) costs little.

### Completion: how the player learns a period finished

Each period's control block has `INTEN`, so the channel raises its interrupt as it moves
to the next block; the interrupt Thread clears `CS.INT` (write 1), reads `CONBLK_AD` to
see which block is now playing, refills the periods behind it, cleans them and advances
an EventCount for whoever waits. `INT_STATUS` gives all channels at once, for the
shared line. Without an interrupt the same position can be polled from `CONBLK_AD` on a
timer, which is the fallback if the interrupt wiring (Risks) is not ready.

## Doom's sound data and what the mixer must produce

- DMX effects (`DSxxxx` lumps): format 3, rate (almost always 11 025 Hz, some 22 050 Hz in
  PWADs), sample count, unsigned 8-bit mono samples centred on 128
  (`userspace/wood/room/src/platform/host/audio/sfx.rs`, `decode_doom_sfx`). Chocolate
  Doom also skips 16 padding bytes at each end of the sample data (secondary, not
  checked here).
- Eight logical channels (`Platform::sound_start(lump, channel 0..8, vol 0..=127, sep 0..=254)`,
  `sound_stop`, `sound_update`, `sound_is_playing` in
  `userspace/wood/room/src/engine/platform.rs`). Pan law from `gains_from`:
  `left = vol·(254 − sep)/(127·254)`, `right = vol·sep/(127·254)`. (The platform comment
  says 128 is centred, `gains_from` uses 127; a room detail to settle when the mixer is
  ported.)
- Music: the host renders the MIDI the engine converts from MUS with `rustysynth` and an
  SF2 SoundFont (`platform/host/audio/music.rs`). That crate uses `std`; Room itself is
  `no_std` with `alloc` now. Music is a later stage (D9).
- What reaches the hardware: interleaved stereo FIFO words, each a value 0..`RNG`,
  right first. The mixer should not know that. The proposal: clients send signed 16-bit
  interleaved stereo at the rate the driver reports, and the driver converts while
  copying into its DMA ring (`word = ((s + 32768) · RNG) >> 16`, swapping the pair so
  right goes first). Silence is the mid value `RNG/2`.

## Proposed architecture

```
 Wood (program)                    audio (driver, pwm node)          dma (driver, dma node)
 ┌──────────────────────┐  audio_v1 ┌──────────────────────┐ dma_v1 ┌────────────────────┐
 │ game Thread:         │ io-channel│ PWM regs, pins       │ calls  │ DMA page, channel  │
 │  sound_start/update  │──────────▶│ ring of periods      │───────▶│ IRQs, CB building  │
 │ playback Thread:     │◀──────────│ s16 → FIFO words     │◀───────│ per-period events  │
 │  mixer, 8 voices     │ EventCount│ underrun → silence   │EventCnt└────────────────────┘
 └──────────────────────┘           └──────────┬───────────┘
                                               │ clock_v1 (via the node's `clocks` link)
                                    cprman (driver, cprman node): CM_PWMCTL/DIV
                                               │ gpio_v1 (pins 40/41 ALT0 from pinctrl-0)
                                    gpio (existing)
```

The decided options are drawn; each box below names the decision it rests on.

- **`audio` driver** (`drivers/audio-pwm-bcm2835`, new). Binds to the PWM node
  (`brcm,bcm2835-pwm`, D3). Owns the PWM register page. Sets its pins through
  `gpio_v1` from the node's `pinctrl-0` (as `sdhost-bcm2835` does; Kickstart passes the
  pins in `DriverInit::pins`). Gets the PWM clock through `clock_v1` (D2). Owns the ring
  memory, carved from its own `DriverInit.memory`, so it knows each period's bus address
  (`memory_bus` plus offset) without asking the kernel. Configures `RNG1 = RNG2`,
  `CTL = PWEN1|USEF1|PWEN2|USEF2` with `MSEN` per D4, `CLRF1` first, `DMAC = ENAB | PANIC 7 |
  DREQ 7`. Exports `audio_v1`.
- **`dma` driver** (`drivers/dma-bcm2835`, new, D1). Binds to `brcm,bcm2835-dma`, the
  only holder of the DMA page and of the channel interrupts. Hands out channels from
  `brcm,dma-channel-mask`. For audio it builds a circular chain of control blocks over
  buffers named by bus address, each `INTEN`, `TI = SRC_INC | DEST_DREQ | WAIT_RESP |
  PERMAP(dreq)`, destination the peripheral FIFO, and reports progress. Because the
  BCM2837 has no IOMMU, any DMA programmer can write all of RAM: the `dma` driver is in
  the trusted base like the USB host controller; how narrow the authority handed to
  clients is was D1: clients name memory by bus address for now (B1a).
- **`cprman` driver** (new, D2). Binds to `brcm,bcm2835-cprman`, exports `clock_v1`
  (`set_rate(clock, hz)`, `enable`, `disable`) for the clocks its clients' nodes name in
  `clocks` (`pwm` is `<&clocks 30>`), refusing others. Linked from the PWM node with
  `uses clock_v1 via device clocks { … }`, the same `via` form `fb` uses for `firmware,
  mboxes`.
- **Client transport** (D6). An io-channel in transmit mode (`libs/io-channel`): Wood
  submits packets of full PCM buffers from the channel's data pages and gets each back
  when the driver has copied it into the ring. The driver is paced by the DMA, not by
  the client: at each period interrupt it takes as many queued bytes as the period needs
  and completes the packets it emptied, advancing the channel EventCount the client
  waits on. The interface declares the channel (`channel N;`), and Kickstart provisions
  it per client, as for `input_v1`.
- **Underrun** (D7). A period with no client data is filled with mid-scale silence and an
  underrun counter goes up; the ring never stops while the output is open. Start and stop
  ramp from 0 to `RNG/2` over a few milliseconds to avoid the turn-on pop.
- **Mixing** (D5). In Wood, in a second Thread (the README's "isochronous playback
  thread"): eight voices of 8-bit DMX data, a fixed-point step resampler (11 025 Hz to the
  reported rate), `gains_from` panning, 32-bit accumulation, clipping to 16 bits. The game
  Thread only changes voice state (start, stop, volume, separation) through a small
  lock-free table; the playback Thread waits on the channel EventCount and keeps two or
  three buffers queued. The mixer is pure `no_std` code in Room, shared with nothing
  platform-specific, so it is host-testable and could also replace the rodio voice path
  on the host later.
- **Scheduling** (D8). Ring depth covers scheduling jitter: 4 periods of 512 frames is
  about 46 ms at 44.1 kHz, more than a Doom tic (28.6 ms). The isochronous scheduler in
  the Future work of [capabilities-decisions.md](capabilities-decisions.md) is not needed
  to start; underrun counts on hardware decide whether it is.

### `audio_v1` (draft)

```
## PCM output through the jack. The client posts full buffers of signed 16-bit
## interleaved stereo frames (left first) on the io-channel in transmit mode; each
## packet comes back once the driver has copied it into the DMA ring.
local interface audio_v1
{
    ## The channel the samples travel on: a control page and four data pages.
    channel 4;

    exception busy {}            ## another client has the output open
    exception not_running {}

    ## The output rate in millihertz (44 091 711 for 100 MHz / 2268).
    rate() returns (card32 millihertz);

    ## Start the output (ramp to silence, start DMA); stop it (ramp down).
    open() raises (busy);
    close() raises (not_running);

    ## Periods filled with silence because no samples were queued, since open.
    underruns() returns (card32 count);
}
```

One word of return value and six argument words fit Meddle's limits.

### `dma_v1` (draft, D1)

- `claim(dreq) returns (handle)`: a channel for the caller's peripheral. The `dma`
  driver keeps a table from DREQ number to the FIFO bus address it may target (PWM
  `0x7E20_C018`); the client never names a peripheral address, and a client is limited to
  the DREQs its node names (`dmas` property, D1) or that the manifest grants.
- `start_cyclic(handle, ring bus address, period bytes, periods)`: memory side named by
  bus address. Checking that the range is RAM the caller owns needs either the lent
  Frames (with `GRANT`, `Frame.GetAddress`) or trust in the caller; D1 chose trust in
  the (driver) caller for now.
- `position(handle) returns (period index)`, `stop(handle)`, and an EventCount the
  `dma` driver advances per finished period.

## What QEMU can and cannot test

Checked against QEMU 11.1.2 (`info mtree` on `-M raspi3b`) and its sources ([S2]):

- **PWM: not emulated.** Nothing is mapped at `0x3F20_C000`, not even an unimplemented
  stub (`bcm2835_peripherals.c` has nine `create_unimp` stubs, none for PWM). An access
  falls into a hole of the peripheral container, which QEMU treats as an unassigned
  access; on Arm that is expected to raise a synchronous external abort (not run). No
  audio backend is wired to any raspi device (`-audio` drivers exist, `none`, `dbus`,
  `oss`, `wav`, but only PCI, HDA, USB and virtio sound devices use them).
- **CPRMAN: emulated**, including the PWM clock mux (`CM_PWMCTL` `0x0a0`, `CM_PWMDIV`
  `0x0a4`, sources as in Table 6-34, 12 integer and 12 fractional divider bits,
  password `0x5a`). A clock driver can be tested there, without any consumer of the
  clock.
- **DMA: emulated, unpaced.** `bcm2835_dma.c` runs a whole chain at once inside the
  register write that starts it: `PERMAP`, `SRC_DREQ`, `DEST_DREQ` and `WAIT_RESP` are
  ignored, abort is a no-op ("we always run to completion"). It does follow
  `NEXTCONBK`, does 2D mode, sets `INT` and raises the channel interrupt when `INTEN` is
  set, and keeps `INT_STATUS` and `ENABLE`. Consequences: a **circular chain never ends
  and hangs the emulator**, so QEMU tests use finite chains; and a write to the PWM FIFO
  address goes nowhere.
- So in QEMU: memory-to-memory DMA tests (chains, interrupts, `INT_STATUS`, `CS.END`,
  error bits), clock register tests, all of the interface and io-channel path, and the
  mixer. The `audio` driver gets a test feature that points its chain at a RAM capture
  buffer instead of `FIF1` and runs it finite, so an end-to-end test can check that the
  words reaching "the FIFO" are the client's samples converted. Everything about pacing,
  the real rate, pops, levels and left/right needs the board.

## Hardware testing

The board is a Pi 3 B (memory: chainboot on hardware). The device tree on hardware is
the firmware's, edited by `config.txt`, and chainboot passes it on to the kernel
unchanged; the PWM node is `status = "disabled"` by default and is enabled (D3) with

```
dtoverlay=pwm-2chan,pin=40,func=4,pin2=41,func2=4
```

in `config.txt`, which also gives the node a pin group of GPIO40 and GPIO41 on function 4
(ALT0) and a 100 MHz assigned clock rate. QEMU tests use the same change made by
`targets/overlays/audio-test-pwm.dts` (`just test-audio`). Order of tests (the first is
`just boot-audio-test`):

1. A test tone, polled: 440 Hz sine on the left only, then the right only, then both, by
   writing `FIF1` while `STA.FULL1` is clear. Confirms pins, clock, range and the swap.
2. The same tone from a DMA ring, with the period interrupt counted against the generic
   timer (`CNTVCT_EL0`) to measure the real sample rate, and `STA` checked for `WERR1`,
   `GAPO1`, `GAPO2` and `BERR`.
3. Underrun behaviour: stop feeding and listen for clicks; check the ramp.
4. Wood effects.

## Risks

- **Interrupts of the DMA node.** It lists 16 interrupts with line 27 four times
  ([P3]); Kickstart issues one `IRQHandler` per listed interrupt (`DriverInit` holds at
  most 16), and issuing the same line twice is untested and may fail. Kickstart needs to
  merge duplicate lines, or the `dma` driver uses a channel with its own line.
- **Kickstart loads only enabled nodes.** The PWM node is disabled in both the repo's
  tree and the firmware's default. QEMU tests that load the driver need a tree with the
  node enabled (and must not touch the registers), D3.
- **The firmware's clock id for PWM** might also be changed by the firmware itself if
  its own audio service ran; Vesper never starts VCHIQ, so it should not.
- **PLLD frequency** is from secondary sources; measured in stage A2.
- **Bandwidth and arbitration.** The PANIC threshold raises the DMA's AXI priority when
  the FIFO runs low (§4.1); with the framebuffer and USB DMA active, gaps (`GAPO`) are
  possible. Checked on hardware.
- **Cache maintenance on the period being written.** The driver writes only periods the
  DMA has left (behind `CONBLK_AD`), cleans each before it comes round again. A ring of
  fewer than three periods leaves no margin for the interrupt latency.
- **Turn-on pop.** Enabling PWM steps the output from 0 to the mid level. The ramp
  reduces it; whether the coupling capacitor still thumps is checked by ear.
- **Music cost.** A SoundFont synthesiser at 44.1 kHz on a Cortex-A53 at 1.2 GHz, in
  floating point, is the heaviest part of the plan; rendering music at 22 050 Hz and
  upsampling halves it. Measured in the music stage.

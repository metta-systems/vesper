# Audio design (Raspberry Pi 3, PWM jack output with DMA)

Status: D1 to D8 answered 2026-10-27 (section Decisions below); stages A1 (the `cprman`
driver with `clock_v1`), A2 (the `dma` driver with `dma_v1`), A3 (the audio driver
serving `audio_v1` from a DMA ring, with a test client) and A4 (Wood's sound effects,
mixed by Room's mixer on a playback Thread) are built and checked in QEMU, their
hardware checks are pending. The one open decision (D9, music) is in
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
  four times; Kickstart issues a line the node repeats once, at its first position, so
  the `dma` driver receives 13 interrupts: channels 0 to 10 at their own number, the
  line of 11 to 14 at 11, `dma-shared-all` at 12 (never enabled: it would also fire for
  the firmware's channels).
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
to the next block. The `dma` driver's interrupt Thread clears `CS.INT` (write 1), reads
`CONBLK_AD` to see which block is now playing and advances the client's `dma_v1`
EventCount by the periods finished; the client (the audio driver) waits on it, refills
the periods behind the playing one and cleans them. `INT_STATUS` gives all channels at
once, for the shared line. `dma_v1.position` reads the same position from `CONBLK_AD`
without an interrupt.

## Doom's sound data and what the mixer must produce

- DMX effects (`DSxxxx` lumps): format 3, rate (almost always 11 025 Hz, some 22 050 Hz in
  PWADs), sample count, unsigned 8-bit mono samples centred on 128
  (`userspace/wood/room/src/platform/host/audio/sfx.rs`, `decode_doom_sfx`). Chocolate
  Doom also skips 16 padding bytes at each end of the sample data (secondary, not
  checked here).
- Eight logical channels (`Platform::sound_start(lump, channel 0..8, vol 0..=127, sep 0..=254)`,
  `sound_stop`, `sound_update`, `sound_is_playing` in
  `userspace/wood/room/src/engine/platform.rs`). Pan law from `gains_from`:
  `left = vol·(254 − sep)/(127·254)`, `right = vol·sep/(127·254)`. Settled in A4: the
  engine centres a sound at 128 (`NORM_SEP` of `s_sound.c`, `128 − swing·sin`, 32 to
  224); the law, Chocolate Doom's `i_sdlsound.c` one, is symmetric about 127, so a
  centred sound plays at 126 : 128 (0.14 dB). Vanilla's own mixer (linuxdoom
  `i_sound.c`, `sep + 1` against `257 − sep`, squared) is symmetric about 127.5 and
  also plays 128 a little right (0.746 : 0.75). The mixer keeps the law unshifted, so
  host and Vesper pan alike (`mixer::CENTRE_SEPARATION`).
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

- **`audio` driver** (`drivers/audio-pwm-bcm2835`, written in A3). Binds to the PWM node
  (`brcm,bcm2835-pwm`, D3). Owns the PWM register page. Sets its pins through
  `gpio_v1` from the node's `pinctrl-0` (as `sdhost-bcm2835` does; Kickstart passes the
  pins in `DriverInit::pins`). Gets the PWM clock through `clock_v1` (D2). Owns the ring
  memory, carved from its own `DriverInit.memory`, so it knows each period's bus address
  (`memory_bus` plus offset) without asking the kernel. Configures `RNG1 = RNG2`,
  `CTL = PWEN1|USEF1|PWEN2|USEF2` with `MSEN` per D4, `CLRF1` first, `DMAC = ENAB | PANIC 7 |
  DREQ 7`. Exports `audio_v1` (section `audio_v1` below: its threading, refill path and
  open/close behaviour as written).
- **`dma` driver** (`drivers/dma-bcm2835`, D1, written in A2). Binds to
  `brcm,bcm2835-dma`, the only holder of the DMA page and of the channel interrupts.
  Because the BCM2837 has no IOMMU, any DMA programmer can write all of RAM: the `dma`
  driver is in the trusted base like the USB host controller; clients name memory by
  bus address for now (B1a). As written:
  - *Channels.* Kickstart passes the node's `brcm,dma-channel-mask` in
    `DriverInit::dma_channel_mask`; `claim` hands out the lowest granted, unclaimed
    channel whose interrupt the driver holds, resets it (`CS.RESET`), clears its `DEBUG`
    error flags, sets its `ENABLE` bit and reads `DEBUG.LITE`, which caps a block at
    65 532 bytes (16-bit length, whole words) instead of 30 bits.
  - *Control blocks* live in one page of the driver's own memory, eight 32-byte slots per
    channel at 256 bytes × the channel's number. The driver writes a chain there,
    cleans the slots (`Frame.CleanData`) and starts the channel by writing the first
    slot's bus address (`memory_bus` plus its offset) to `CONBLK_AD` and `CS = ACTIVE |
    WAIT_FOR_OUTSTANDING_WRITES`, priority 1, panic priority 15 (Circle's values,
    secondary; the datasheet recommends none). A copy block is `INTEN | WAIT_RESP |
    SRC_INC | DEST_INC`, unpaced (PERMAP 0); a ring period is `INTEN | WAIT_RESP |
    DEST_DREQ | SRC_INC | NO_WIDE_BURSTS | PERMAP(dreq)` with the peripheral's fixed
    FIFO address, and the last period links back to the first. Building and checking
    the chains is the pure library (`src/lib.rs`, host tests `tests/dma.rs`).
  - *Peripheral addresses* come from a table in the driver keyed by DREQ (5: PWM `FIF1`,
    bus `0x7E20_C018`); a client never names one. DREQ 0 is memory. A client may claim
    any DREQ of the table: the PWM node names no `dmas`, so limiting a client to the
    DREQs its node names is a follow-up.
  - *Interrupts and completions.* The driver's first Thread waits on its Notification,
    takes a turn of the component's turnstile (calls take turns with it), reads
    `INT_STATUS` and, for each channel it handed out with `INT` set, reads `CS` and
    `CONBLK_AD`, clears `INT` and `END` (keeping `ACTIVE` while a ring runs), works out
    from `CONBLK_AD` how many blocks finished since the last time, and advances the
    owner's io-channel `EventCount` by that many. Counting from `CONBLK_AD` rather than
    per interrupt survives a late interrupt and QEMU, which raises one interrupt for a
    whole chain. It then acknowledges the handlers that delivered. A handler is first
    enabled when a channel on its line is claimed, so the lines of the firmware's
    channels stay masked. `INT_STATUS` tells channels 11 to 14 apart on their shared
    line.
  - *Stopping* pauses the channel (`ACTIVE` clear), waits up to 1 ms for its writes to be
    answered, and resets it.
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
  waits on. The interface declares the channel (`channel 4;`), and Kickstart provisions
  it per client, as for `input_v1`. As written in A3: section `audio_v1`.
- **Underrun** (D7). A period with too few client frames is filled up with mid-scale
  silence and an underrun counter goes up; the ring never stops while the output is open.
  Opening ramps from 0 to `RNG/2` over one period, closing from the last frame to 0 over
  one period, against the turn-on pop.
- **Mixing** (D5). In Wood, in a second Thread (the README's "isochronous playback
  thread"): eight voices of 8-bit DMX data, a fixed-point step resampler (11 025 Hz to the
  reported rate), `gains_from` panning, 32-bit accumulation, clipping to 16 bits. The game
  Thread only changes voice state (start, stop, volume, separation) through a small
  lock-free table; the playback Thread waits on the channel EventCount and keeps two or
  three buffers queued. The mixer is pure `no_std` code in Room, shared with nothing
  platform-specific, so it is host-testable and could also replace the rodio voice path
  on the host later. As written in A4:
  - *Mixer* (`room::engine::sound::mixer`). `Mixer` holds eight voices, each a sound,
    a 32.32 position and step (`step = rate·2³²·1000 / output mHz`, 0.250 04 for
    11 025 Hz at 44 091.71 Hz) and two 16.16 gains (`gains`, the law above); a frame is
    each voice's sample interpolated linearly between its two neighbours, times its
    gains, summed in 32 bits and clipped to 16. A voice ends when its position leaves
    the last sample. Lumps are read as the host reads them (`dmx_samples`: no format
    check, the 16 padding bytes at each end played).
  - *Sharing* (`SharedVoices`). The engine's lump lives in its zone cache and may be
    purged, so `start` copies each distinct sound once into a `SoundBank` (atomic
    pointers, never freed; 55 lumps, 535 kB in the shareware IWAD) and stores the
    channel's levels, then its command word (a sequence number and the sound's bank
    index), with release. `Mixer::mix_shared` loads each command with acquire, starts
    or stops voices whose sequence changed, applies the current levels to the rest,
    mixes, and stores the sequence of every idle voice as "finished".
    `is_playing` is a command with a sound whose sequence is not finished, so it is
    true from `start` on, false from `stop` on, and false one buffer after the sound
    ran out. No lock, no wait on the game Thread.
  - *Playback Thread* (Wood's `sound.rs`). A further Thread of the program
    (`driver_threads!`), on the 16 KiB stack Kickstart gives it; the mixer writes
    straight into the io-channel's data pages. It waits on Wood's `EventCount` for
    `sound_init`, then calls `rate` and `open` (it is the only Thread calling
    `audio_v1`, whose import area serves one Thread at a time) and answers; on a
    missing driver or `busy` it logs once and the engine runs silent. It keeps 4 buffers
    of 512 frames (46 ms) queued: each packet back is mixed into and put again. On
    `sound_shutdown` it stops refilling, calls `close` right after the last packet, takes
    the rest back and answers again.
- **Scheduling** (D8). Ring depth covers scheduling jitter: 4 periods of 512 frames is
  about 46 ms at 44.1 kHz, more than a Doom tic (28.6 ms). The isochronous scheduler in
  the Future work of [capabilities-decisions.md](capabilities-decisions.md) is not needed
  to start; underrun counts on hardware decide whether it is.

### `audio_v1` (written in A3)

`drivers/audio-pwm-bcm2835/interfaces/audio_v1.if`, a prototype edited in place: `local`,
`badged;` (the driver tells clients and their io-channels apart by the badge of their
keys) and `channel 4;` (a control page and four data pages, 4096 frames, per client).

- `rate() returns (card32 millihertz)`: 44 091 710 (100 MHz / 2268).
- `open() raises (busy)`: the caller gets the output; `busy` while another client has
  it, or it is still closing. Resets the underrun count.
- `close() raises (not_open)`: plays out what the caller queued, ramps down, stops;
  returns once the output is off and every packet is back. `not_open` for a caller that
  does not have the output open.
- `underruns() returns (card32 periods)`: periods since the last `open` that were filled
  up with silence, counted from the first packet after `open` until `close`.

Channel semantics (transmit mode). A packet names 1 to 8 buffers of the data pages, each
a whole number of 4-byte frames (left then right, signed 16-bit little-endian) at a
4-byte aligned address; the driver refuses anything else by handing the packet back at
once with every record cut to 0 bytes. The client puts packets and never rings the
driver: the refill Thread looks at the submit FIFO once per period. A packet comes back
on the completion FIFO, with the channel's EventCount advanced, once the driver copied
its last frame into the ring, so a packet back is a buffer free to refill; records are
cut to the bytes taken (whole, except what `close` cuts off). Latency from a packet's
completion to its sound is at most the ring, four periods (46 ms). The control page's
FIFOs have 64 slots each (`stream::CHANNEL_SLOTS`): 32 single-buffer packets in flight.

Threading. One Thread, the driver's first, is the refill Thread; it alone touches the
PWM block, the ring and `dma_v1`. The `audio_v1` calls run on their callers' Threads (PPC)
and only hand requests over through atomics and two driver-private EventCounts: `open`
claims the output (compare-and-swap of the owner) and advances a doorbell the refill
Thread sleeps on while the output is closed; `close` sets a closing flag and waits on a
"closed" EventCount the refill Thread advances once the output is off. While the output
is open the refill Thread sleeps on `dma_v1`'s EventCount only (one advance per finished
period), and sees `close` at the next period, within 12 ms. No Turnstile is needed:
nothing the calls touch is shared beyond those atomics.

Refill path (`src/stream.rs`, host tests `tests/stream.rs`). The ring is 4 periods of 512
frames, one page each. At `open` every slot is filled: period 0 ramps from 0 up to
`RNG/2`, periods 1 to 3 come from the client's queue; then the PWM is set up and
`dma_v1.start_ring` started (§9.6: channels stopped, `RNG1 = RNG2`, `CLRF1`, `DMAC`, then
`PWEN`/`USEF`). When the EventCount says `n` periods finished in all, `stream::Ring`
names the periods to write: the slots the DMA left take the periods after the ones
already written, never the slot playing; if the DMA came round to a slot first, the
periods it replayed are counted late. Each period is filled by `Stream::fill`: packets in
order, records in order, each frame converted to `[word(right), word(left)]` with
`word = ((s + 32768) · RNG) >> 16` (0 for -32768, `RNG/2` for 0, `RNG - 1` for 32767),
then `CleanData` on the slot. Too few frames: the rest of the period is silence (`RNG/2`)
and, once a packet has come since `open`, it counts as an underrun (D7).

Open and close. `close` does not cut the stream: the refill Thread keeps taking the
queued packets (`Stream::drain`, the end of the stream is no underrun), at most 8 periods
(the frames the data pages hold, so a client that keeps sending cannot hold `close`
forever; packets after that come back cut). The first period that finds nothing queued
gets the ramp from the last frame's words down to 0, the periods after it zeros; once
the DMA has finished the ramp the driver stops the channel (`dma_v1.stop`), the PWM
channels and `DMAC`, hands back anything put meanwhile, and traces the stream (frames,
underruns, late periods, packets back; on the board the rate measured from the period
completions against `CNTVCT_EL0`, the `STA` flags and `dma_v1.errors`). A client that
wants its tail played closes right after its last packet; one that waits for every
packet first leaves a silent period behind it, which counts as an underrun.

### `dma_v1` (written in A2, D1)

`drivers/dma-bcm2835/interfaces/dma_v1.if`, a prototype edited in place. It is `local`
and `badged;` (the driver tells clients apart by the badge of their keys) and declares
`channel 1;`: Kickstart provisions an io-channel per client, and the driver advances its
`EventCount` by one for every finished control block on the client's channels (a period
of a ring, a block of a copy); the io-channel's FIFOs carry nothing yet. The functions
(`dma_channel` is the channel's number; `channel` is a Meddle keyword):

- `claim(dreq) returns (dma_channel)`: a channel for the peripheral of `dreq` (5 is
  PWM) or for memory (0); raises `no_channel` or `unknown_request`.
- `copy(dma_channel, source, destination, block_bytes, blocks)`: a finite chain of up to
  eight linked blocks, unpaced, each with its interrupt (QEMU tests, and the capture
  mode of A3).
- `start_ring(dma_channel, ring, period_bytes, periods)`: a circular chain of up to
  eight periods into the channel's peripheral FIFO, paced by its DREQ; raises
  `unknown_request` on a memory channel.
- `position(dma_channel) returns (block)`: the block the channel is on, or the chain's
  length once it has ended or was stopped.
- `stop(dma_channel)`, `release(dma_channel)`, and `errors(dma_channel) returns (bits)`:
  the `DEBUG` error flags (bits 0 to 2) and `CS.ERROR` (bit 3) since the chain started.
- Every function raises `not_owner` for a channel the caller did not claim; `copy` and
  `start_ring` raise `busy` while a chain runs and `bad_transfer` for a length that is
  zero, not whole words or over the channel's limit, an unaligned or wrapping address,
  or more than eight blocks.

Coherency rules for clients (the controller does not snoop the CPU caches): clean what
the DMA reads (`Frame.CleanData`) before starting the chain or, for a ring, before the
period comes round again; clean and invalidate (`Frame.CleanInvalidateData`) what the
DMA writes before starting, so no dirty line is written back over its bytes, and
invalidate it again before reading it. Memory is named by its bus address,
`DriverInit::memory_bus` plus the offset into the client's memory.

Checking that a client's ranges are RAM it owns (lent Frames with `GRANT` and
`Frame.GetAddress`, B1b) waits for an untrusted client.

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
  address goes nowhere. Seen while writing A2: QEMU stores a `DEBUG` write as it is
  (the hardware clears the error flags written with 1), so the `dma` driver writes back
  only the flags it read as set; and `DEBUG.LITE` reads 0 there, so every channel looks
  like a full one. One interrupt covers a whole chain, which the driver's counting from
  `CONBLK_AD` turns into one EventCount advance per block.
- So in QEMU: memory-to-memory DMA tests (chains, interrupts, `INT_STATUS`, `CS.END`,
  error bits), clock register tests, all of the interface and io-channel path, and the
  mixer. As written in A3, the `audio` driver under `qemu` replaces the ring by a
  capture: each period is played by a one-block `dma_v1.copy` from its ring slot into
  the same slot of a 4-page capture buffer, started at the period's nominal time (11.6 ms
  apart, so the stream runs at about the real pace), and the copy's completion is the
  EventCount advance a ring period makes on the board; the same refill code then runs.
  After each copy the driver invalidates the capture slot and compares it word by word
  with what the period was filled with: the ramps, and for stream periods the capture
  test's pattern (`stream::test_pattern`, which the test client sends) converted, from the
  stream position the period started at, then silence. The pattern check is the
  client's to ask for (A4): a stream whose first period starts with 16 frames of the
  pattern (`stream::starts_with_test_pattern`) is checked against it; any other stream,
  such as Wood's mixed effects, is checked against the ring slot each period was copied
  from, then silence, and its periods that are not silent are counted and traced at
  powers of two (`stream::is_audible`; `just test-wood` asserts them). So the end-to-end test
  (`just test-audio`) checks that the client's samples reach "the FIFO" converted, right
  first, in order, through the DMA and the cache maintenance. Everything about pacing,
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
`targets/overlays/audio-test-pwm.dts` (`just test-audio`). Order of tests:

1. `just boot-audio-test` (since A3; the polled tone of A1 and the driver's own ring
   tone of A2 were replaced by it): the test client (`userspace/tests/audio/tone`)
   streams a 440 Hz sine at a quarter of full scale through `audio_v1`: 3 s on the left
   only, 3 s on the right only, 3 s on both, then 60 s on both, each its own
   `open`/`close`, 0.7 s apart. Confirms pins, clock, range and the swap (by ear: the
   order is fixed), and the DMA ring: at each close the driver traces the rate measured
   from the period completions against the generic timer (`CNTVCT_EL0`) and its distance
   from 44 091.710 Hz in ppm ("within 0.1 %"), the underruns and late periods, the `STA`
   flags (`WERR1`, `RERR1`, `GAPO1`, `GAPO2`, `BERR`) and `dma_v1.errors`.
2. Underrun behaviour: stop feeding and listen for clicks; check the ramps.
3. Wood effects (since A4): `just boot-wood-test` with the same overlay line in
   `config.txt` and headphones in the jack. The demos fire weapons and open doors:
   each effect should be heard, panned with its source (a sound on the player's left
   in the left ear), without clicks at starts and ends. Without the overlay the image boots and Wood runs
   silent. The underrun count is not visible on the board yet (Wood logs on
   semihosting only, and the compositor holds the UART).

## Risks

- **Interrupts of the DMA node** (resolved in A2). It lists 16 interrupts with line 27
  four times ([P3]). Kickstart used to issue one `IRQHandler` per listed interrupt; the
  second issue of a line failed with `IrqLineActive` and was skipped with a misleading
  "taken" message. It now issues each line once, at its first position
  (`libdevicetree::interrupts::distinct_interrupts`), and the `dma` driver maps channels
  to interrupts by that order.
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

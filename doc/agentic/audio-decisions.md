# Audio open decisions

Parking lot for audio decisions the maintainer has not answered. Each entry stays open
until the maintainer answers it. Recommendations are marked as such; they are not
decisions. Record an answer only when the maintainer gave it explicitly; answered items
move to [audio-design.md](audio-design.md), and tasks stay in
[audio-implementation-plan.md](audio-implementation-plan.md).

Facts behind these questions (pins, registers, QEMU support) are in the design doc, with
sources.

## D1. Who programs the DMA controller (blocks A2, A3)

DMA channels 0 to 14 share one 4 KiB register page with the global `INT_STATUS` and
`ENABLE`, so whoever maps it can drive every channel, and with no IOMMU on the BCM2837 a
DMA programmer can write any RAM.

- **A. The audio driver owns the DMA page itself.** Kickstart gives it the page (as a
  shared device Frame, the path it already has for pages several nodes touch) and the
  interrupt of one channel. Least code; but Kickstart needs a way to hand one driver
  resources of a second node (for example by following a `dmas` link, which the stock
  PWM node does not have, or a manifest key), and the next DMA user (SD host, SPI)
  shares the page with it.
- **B. A `dma` driver for the `brcm,bcm2835-dma` node exporting `dma_v1`.** It alone maps
  the page and takes the channel interrupts, allocates channels from
  `brcm,dma-channel-mask`, builds the control blocks and lets a client target only the
  FIFO of its own DREQ. Reusable by later drivers; one more component and interface, and
  a call plus an EventCount between it and the audio driver. Sub-question B1: the memory
  side is (B1a) a bus address the trusted audio driver names, or (B1b) Frames the client
  lends with `GRANT`, checked with `Frame.GetAddress` against the RAM ranges.
- **C. A `dma` driver that only allocates channels and hands back one channel's
  registers.** Not possible: the channels are 0x100 apart inside one page, and mapping is
  page-granular.

Recommendation: B with B1a for now (the audio driver is a driver in the trusted base,
and B1b can come when an untrusted client needs DMA).

## D2. How the PWM clock is set (blocks A1)

- **A. A `cprman` driver for the clock manager node exporting `clock_v1`**, linked from
  the PWM node's `clocks = <&clocks 30>` with `uses clock_v1 via device clocks`. It
  refuses clocks the caller's node does not name. Emulated by QEMU, so testable there;
  matches the contract's expected `cprman` component.
- **B. A typed `mailbox_v1.set_clock_rate(pwm, hz)`** for firmware clock id 10. Small; but
  whether the firmware programs the PWM clock this way is undocumented (Linux uses
  CPRMAN directly), it can only be checked on hardware, and the clock source and divider
  are the firmware's choice.
- **C. The audio driver maps the CPRMAN pages itself.** Gives it every clock generator
  and the PLL controls (UART, SD card, …).

Recommendation: A.

## D3. Which device tree node the audio driver binds to, and how it is enabled (blocks A1)

Kickstart loads drivers only for enabled nodes, and the PWM node is disabled by default.

- **A. The PWM node, `brcm,bcm2835-pwm`.** On hardware enabled by
  `dtoverlay=pwm-2chan,pin=40,func=4,pin2=41,func2=4` in `config.txt`, which also sets
  `pinctrl-0` to the jack pins; for QEMU a test tree or overlay with the node enabled
  (the driver must then not touch the unemulated registers outside its capture test
  mode).
- **B. The firmware audio node, `brcm,bcm2835-audio`** (`dtparam=audio=on`). Its
  `pinctrl-0` names the jack pins, but it has no `reg` (the firmware drives it through
  VCHIQ), so the driver would get no PWM registers without Kickstart special-casing it.
- **C. A program (`[[component]]`, no node)** with fixed addresses. Against the
  device-tree-driven loading every other driver uses.

Recommendation: A. Sub-question: who edits the board's `config.txt` and which tree the
QEMU tests use (the repo's `targets/bcm2710-rpi-3-b-plus.dtb` with an overlay applied at
build time, or a second checked-in tree).

## D4. Output rate, clock and PWM mode (blocks A1)

- **A. 44.1 kHz nominal from 100 MHz (PLLD per / 5, integer divider), `RNG` 2268,
  44 092 Hz, about 11 bits, `MSEN=0`.** The datasheet's nominal PWM clock; carrier above
  hearing; the mixer resamples to the reported rate.
- **B. As A but 250 MHz (PLLD / 2), `RNG` 5669, 44 100.2 Hz, about 12.5 bits.** Circle's
  setup on Pi 1 to 3; above the datasheet's nominal clock.
- **C. 22.05 kHz.** Half the data and half the music rendering, but the carrier sits at
  the jack filter's corner and is audible to some listeners.
- **D. Exact 44 100 Hz with a MASH fractional divider.** Exact rate, divider jitter.

Also: `MSEN=0` (the datasheet's spreading algorithm, default) versus `MSEN=1` (one pulse
per period). Recommendation: A with `MSEN=0`; try B on hardware if 11 bits sound noisy.

## D5. Where mixing happens (blocks A4)

- **A. In Wood: a no_std mixer in Room, run by a second Wood Thread (the playback
  Thread)**; the driver is a plain PCM sink. Policy stays in the game, one client.
- **B. In Wood, but mixed on the game Thread at each tic** (1/35 s of audio per call).
  No second Thread; any stall in the game (level load, disk) is an underrun.
- **C. A mixing service component** between clients and the driver (voices or streams
  from several programs). Needed only once a second program plays sound.
- **D. In the audio driver** (voices as an interface). Puts game policy into a driver.

Recommendation: A, keeping C as future work.

## D6. How a client hands samples to the driver (blocks A3)

- **A. An io-channel in transmit mode** declared in `audio_v1` (`channel N;`), signed
  16-bit interleaved stereo; the driver copies and converts into its DMA ring. One copy
  of 176 KB/s; the client never sees hardware formats or bus addresses.
- **B. Zero copy: the client writes FIFO words straight into the DMA ring**, mapped into
  it. No copy, but the client must know `RNG`, the left/right swap and the ring position,
  and the ring memory and its cache maintenance cross a component boundary.

Recommendation: A.

## D7. Underrun policy (blocks A3)

- **A. Fill with mid-scale silence, count underruns, keep the DMA ring running.**
- **B. Repeat the last period.** Hides short gaps in tones, buzzes on longer ones.
- **C. Stop the DMA on underrun and restart on the next submit.** Each stop and start
  risks a pop.

Recommendation: A.

## D8. Scheduling of the playback Thread (blocks A4 on hardware)

- **A. No special scheduling at first**: a ring of 4 periods (about 46 ms) and an
  ordinary Time budget; measure underruns on hardware.
- **B. Build the isochronous scheduler first** (Future work in
  [capabilities-decisions.md](capabilities-decisions.md)).

Recommendation: A; B if hardware shows underruns under load.

## D9. Music synthesis (blocks A5)

- **A. Port the SoundFont path** (`rustysynth` or a no_std rewrite) with an SF2 file as
  a Kickstart `[[resource]]`. Matches the host; the SoundFont's size goes into the image
  and the synthesis cost is the largest of the plan.
- **B. An OPL2/OPL3 emulator** playing Doom's GENMIDI instruments, as the original game
  did. Small data, cheaper; a different sound from the host build.
- **C. No music on Vesper for now.**

Recommendation: decide after A4, with the CPU headroom measured; leaning A at 22 050 Hz
if it fits.

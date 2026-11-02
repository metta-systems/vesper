// Host builds only check that the crate builds; the component itself runs on
// Vesper.
#![cfg_attr(target_os = "vesper", no_std, no_main)]

//! EL0 driver component for the BCM2835 PWM block, the Raspberry Pi 3's
//! headphone jack, serving `audio_v1` (see `interfaces/audio_v1.if` and
//! `doc/agentic/audio-design.md`).
//!
//! Kickstart loads it for the `brcm,bcm2835-pwm` node, which is enabled by the
//! `pwm-2chan` overlay on the board and by the test tree under QEMU (decision
//! D3), and wires its imports (see `interfaces/imports.if`): `clock_v1` through
//! the node's `clocks` link to the clock manager, `gpio_v1`, through which it
//! routes the pins of its node's pin group, and `dma_v1` of the DMA controller
//! component (decision D1), which runs the ring of control blocks that feeds
//! the FIFO from the driver's own memory, named by bus address.
//!
//! At start it routes the pins, has the PWM clock set to 100 MHz and reads
//! the rate back, claims a DMA channel for the PWM's DREQ and carves a ring of
//! four periods of 512 frames from its memory. Then its one Thread, the
//! refill Thread, sleeps until a client opens the output. While the output is
//! open it waits on `dma_v1`'s `EventCount`, which advances once per period
//! the DMA finished, and refills each period the DMA left: from the client's
//! io-channel, converting signed 16-bit stereo into FIFO words, right first,
//! and with silence (counted as an underrun) when too few frames are queued;
//! it cleans the period (`Frame.CleanData`) and hands back the packets it
//! took. The output opens with a period ramping up to silence; `close` plays
//! out what the client queued, then a period ramping down from the last
//! frame, and stops the DMA and the channels. The `audio_v1` calls run on
//! their callers' Threads and only hand requests to the refill Thread. It
//! traces each stream on its serial channel when it closes.
//!
//! QEMU does not emulate the PWM block (an access faults), and its DMA model
//! runs a whole chain inside the write that starts it (a ring would never
//! end), so under the `qemu` feature the driver checks `clock_v1` and
//! `dma_v1` at start, never touches the PWM registers, and plays each period
//! by a one-block `dma_v1.copy` into a capture buffer, paced at the period's
//! length, checking the captured words against what filled the period: the
//! capture test's pattern for a stream that starts with it, the ring's own
//! words (and how many periods are not silent) for any other client.

#[cfg(target_os = "vesper")]
mod component;

#[cfg(not(target_os = "vesper"))]
fn main() {}

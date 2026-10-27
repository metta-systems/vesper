// Host builds only check that the crate builds; the component itself runs on
// Vesper.
#![cfg_attr(target_os = "vesper", no_std, no_main)]

//! EL0 driver component for the BCM2835 PWM block, the Raspberry Pi 3's
//! headphone jack (stages A1 and A2 of `doc/agentic/audio-implementation-plan.md`).
//!
//! Kickstart loads it for the `brcm,bcm2835-pwm` node, which is enabled by the
//! `pwm-2chan` overlay on the board and by the test tree under QEMU (decision
//! D3), and wires its imports (see `interfaces/imports.if`): `clock_v1` through
//! the node's `clocks` link to the clock manager, `gpio_v1`, through which it
//! routes the pins of its node's pin group, and `dma_v1` of the DMA controller
//! component (decision D1), which runs the chains of control blocks that feed
//! the FIFO from the driver's own memory, named by bus address.
//!
//! For now it is a test: it routes the pins, has the PWM clock set to 100 MHz
//! and reads the rate back, then plays a 440 Hz sine on the left side, the
//! right side and both, a few seconds each, by writing the FIFO while it has
//! room; then the same tone on both sides from a DMA ring of four periods,
//! paced by the PWM's DREQ, timing the period completions against the generic
//! timer to measure the real sample rate. It traces what it plays on its
//! serial channel, signals the report key `driver_protocol::REPORT_SLOT` if
//! its loader installed one, and parks. Stage A3 replaces the tone with
//! `audio_v1`.
//!
//! QEMU does not emulate the PWM block (an access faults), and its DMA model
//! runs a whole chain inside the write that starts it (a ring would never
//! end), so under the `qemu` feature the driver checks the clock through
//! `clock_v1`, copies memory through `dma_v1` with finite chains, and never
//! touches the PWM registers.

#[cfg(target_os = "vesper")]
mod component;

#[cfg(not(target_os = "vesper"))]
fn main() {}

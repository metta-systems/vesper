// Host builds only check that the crate builds; the component itself runs on
// Vesper.
#![cfg_attr(target_os = "vesper", no_std, no_main)]

//! EL0 driver component for the BCM2835 PWM block, the Raspberry Pi 3's
//! headphone jack (stage A1 of `doc/agentic/audio-implementation-plan.md`).
//!
//! Kickstart loads it for the `brcm,bcm2835-pwm` node, which is enabled by the
//! `pwm-2chan` overlay on the board and by the test tree under QEMU (decision
//! D3), and wires its imports (see `interfaces/imports.if`): `clock_v1` through
//! the node's `clocks` link to the clock manager, and `gpio_v1`, through which
//! it routes the pins of its node's pin group.
//!
//! For now it is a test: it routes the pins, has the PWM clock set to 100 MHz
//! and reads the rate back, then plays a 440 Hz sine on the left side, the
//! right side and both, a few seconds each, by writing the FIFO while it has
//! room (no DMA). It traces what it plays on its serial channel, signals the
//! report key `driver_protocol::REPORT_SLOT` if its loader installed one, and
//! parks. Stage A3 replaces the tone with `audio_v1`.
//!
//! QEMU does not emulate the PWM block (an access faults), so under the `qemu`
//! feature the driver checks the clock through `clock_v1` and never touches the
//! PWM registers.

#[cfg(target_os = "vesper")]
mod component;

#[cfg(not(target_os = "vesper"))]
fn main() {}

// Host builds only check that the crate builds; the program itself runs on
// Vesper.
#![cfg_attr(target_os = "vesper", no_std, no_main)]

//! `midi-player`: midi-test's program. It plays its bundled song (Doom's
//! `D_E1M1`, written by `just wood-midi`) through the MIDI synthesiser
//! component and the PWM audio driver, looping, at a fixed volume, the way
//! Wood's playback Thread plays music, with nothing else running: it sends
//! the song on `midi_synth_v1`'s io-channel, then for each period submits a
//! render buffer, calls `service`, copies the frames into a buffer of
//! `audio_v1`'s io-channel and queues it, keeping eight buffers queued.
//!
//! It logs (through the log server, to the UART) the song's timing and first
//! notes at start, the ARM and core clocks, and every 256 periods (about 3 s)
//! Wood's music summary (`service` min/avg/max, packets, periods with sound,
//! playing, the longest refill, underruns), the level (peak and RMS), the
//! synthesiser's peak voice count, and the song position.
//!
//! On the board it plays for good. Under QEMU it plays a bounded number of
//! periods, closes, and signals the report key midi-test installs
//! (`driver_protocol::REPORT_SLOT`), which ends the run.

#[cfg(target_os = "vesper")]
extern crate alloc;

#[cfg(target_os = "vesper")]
mod heap;
#[cfg(target_os = "vesper")]
mod player;

#[cfg(not(target_os = "vesper"))]
fn main() {}

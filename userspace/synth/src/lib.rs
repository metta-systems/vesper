//! Wood's music synthesiser for Vesper (audio decision D9): an integer
//! SoundFont 2 wavetable synthesiser and a Standard MIDI File sequencer.
//!
//! `no_std` with `alloc`, and no floating point anywhere: Vesper builds
//! soft-float and traps FP/SIMD, so every level, pitch and time is fixed
//! point, and the exponential and trigonometric curves are tables generated
//! by `build.rs` on the build host (`tables`). `#![deny(clippy::float_arithmetic)]`
//! and the `no_float` host test keep it that way; `just synth-no-float` checks
//! the target build for soft-float calls.
//!
//! - [`sf2`]: a zero-copy reader of the SoundFont (the sample data is read in
//!   place, from the mapped resource on Vesper).
//! - [`bank`]: presets and instruments resolved into zones, generators merged
//!   (global zone under local, preset values added to instrument values).
//! - [`synth`]: sixteen MIDI channels, [`synth::POLYPHONY`] voices with
//!   stealing, rendered as interleaved stereo s16 in blocks of
//!   [`synth::BLOCK_FRAMES`].
//! - [`voice`]: 32.32 sample playback with linear interpolation and the loop
//!   modes, the volume envelope ([`envelope`]), attenuation and pan.
//! - [`smf`] and [`sequencer`]: the MIDI file and its playback clocked by the
//!   frames rendered, with looping.
//! - [`trim`]: a SoundFont cut down to what a set of songs plays (D9.4).
//!
//! Left out, for now: the low-pass filter (`initialFilterFc`/`Q` are
//! ignored), the modulation envelope, the vibrato and modulation LFOs, the
//! SoundFont's modulator lists (only the default velocity, volume,
//! expression, pan and pitch-wheel behaviour is built in), chorus and reverb,
//! 24-bit samples (`sm24`) and ROM samples.

#![no_std]
#![deny(clippy::float_arithmetic)]
#![deny(clippy::float_cmp)]
#![deny(clippy::cast_precision_loss)]

extern crate alloc;

pub mod bank;
pub mod channel;
pub mod envelope;
pub mod generator;
pub mod sequencer;
pub mod sf2;
pub mod smf;
pub mod synth;
pub mod tables;
pub mod trim;
pub mod voice;

pub use {
    bank::Bank,
    sequencer::Sequencer,
    smf::Song,
    synth::Synth,
    tables::{OUTPUT_RATE, Rate},
};

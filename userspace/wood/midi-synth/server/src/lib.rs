#![no_std]

//! The `midi_synth_v1` interface, generated from
//! `userspace/wood/midi-synth/interfaces/midi_synth_v1.if`: its types, the
//! [`midi_synth_v1::MidiSynthV1`] trait the synthesiser component implements,
//! and [`midi_synth_v1_export!`], which exports an implementation's functions
//! as PPC entries.

include!(concat!(env!("OUT_DIR"), "/midi_synth_v1.rs"));

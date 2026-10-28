#![no_std]

//! Calls to the MIDI synthesiser component, generated from
//! `userspace/wood/midi-synth/interfaces/midi_synth_v1.if`: a
//! [`midi_synth_v1::Client`] takes the synthesiser and has it handle the song
//! and render packets put on its io-channel.

include!(concat!(env!("OUT_DIR"), "/midi_synth_v1.rs"));

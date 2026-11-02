#![no_std]

//! Calls to the `audio_v1` interface, generated from
//! `drivers/audio-pwm-bcm2835/interfaces/audio_v1.if`: an `audio_v1::Client`
//! holds the Invocations its driver was granted and makes typed calls through
//! them.

include!(concat!(env!("OUT_DIR"), "/audio_v1.rs"));

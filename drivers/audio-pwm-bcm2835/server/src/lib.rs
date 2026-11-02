#![no_std]

//! The `audio_v1` interface, generated from
//! `drivers/audio-pwm-bcm2835/interfaces/audio_v1.if`: its types, the
//! [`audio_v1::AudioV1`] trait the PWM audio driver implements, and
//! `audio_v1_export!`, which exports an implementation's functions as PPC
//! entries.

include!(concat!(env!("OUT_DIR"), "/audio_v1.rs"));

// Host builds only check that the crate builds; the component itself runs on
// Vesper.
#![cfg_attr(target_os = "vesper", no_std, no_main)]

//! The MIDI synthesiser component: Wood's music on Vesper (decisions D9 and
//! D10 of `doc/agentic/audio-design.md`). It exports `midi_synth_v1` (see
//! `interfaces/midi_synth_v1.if`): a client sends Standard MIDI Files on the
//! interface's io-channel and gets rendered frames back on it, which it mixes
//! and plays itself.
//!
//! At start it carves a heap from its RAM, loads the SoundFont bundled with it
//! (its `[[resource]]`, the SC-55 trimmed to the IWAD's music by
//! `just wood-soundfont`) with `wood-synth`, and says the exports can be
//! served; its own Thread then sleeps for good. The work happens in
//! `service`, on the caller's Thread: the packets the client put are
//! handled in order by the [`midi_synth::Session`], each handed back.

#[cfg(target_os = "vesper")]
extern crate alloc;

#[cfg(target_os = "vesper")]
mod component;
#[cfg(target_os = "vesper")]
mod heap;

#[cfg(not(target_os = "vesper"))]
fn main() {}

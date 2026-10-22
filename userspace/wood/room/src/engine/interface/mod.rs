//! `i_*` sources: platform-dependent implementation (timing, sound, input,
//! system services) behind the `Platform` trait.

pub mod cdmus;
pub mod endoom;
pub mod input;
pub mod joystick;
pub mod keys;
pub mod scale;
pub mod sound;
pub mod system;
pub mod timer;
pub mod video;

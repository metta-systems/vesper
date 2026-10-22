//! Host platform: a winit window, a wgpu renderer and rodio audio.
//!
//! [`run`] drives the engine from the winit event loop (`app`) on [`Host`],
//! the [`Platform`] implementation below; the sound and music it delegates
//! to live in `audio`.
//!
//! ## Global state
//!
//! The window, GPU, key queue and clock are stored in [`thread_local!`]
//! statics protected by [`std::cell::RefCell`], which [`Host`]'s methods
//! reach.  This is safe because **all** calls originate from the main thread
//! (the winit event loop).
//!
//! This design is an intentional *functional approximation* of the original
//! C globals, to be refactored towards idiomatic Rust in future iterations.

mod app;
mod audio;
mod gpu;
pub mod keys;

pub use app::run;

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    sync::Arc,
    time::Instant,
};

use winit::window::Window;

use room::doom::doomgeneric::Platform;

use gpu::GpuState;

// ---------------------------------------------------------------------------
// Thread-local platform state
// ---------------------------------------------------------------------------

// GPU, window, keyboard and time state used by `Host`.
// Doc comments are deliberately omitted on the macro invocation since
// rustdoc does not process comments attached to `thread_local!` calls.
thread_local! {
    // GPU rendering state.  `None` until `GpuState` is created in `resumed`.
    pub(crate) static GPU: RefCell<Option<GpuState>> = const { RefCell::new(None) };

    // OS window handle used for title updates.
    pub(crate) static WINDOW: RefCell<Option<Arc<Window>>> = const { RefCell::new(None) };

    // Keyboard event queue consumed by `Host::key`.
    // Each entry is `(pressed, doom_key_byte)`.
    pub(crate) static KEY_QUEUE: RefCell<VecDeque<(bool, u8)>> =
        const { RefCell::new(VecDeque::new()) };

    // Absolute time of programme launch, used by `Host::ticks_ms`.
    static START_TIME: Cell<Option<Instant>> = const { Cell::new(None) };

    // Set to `true` when the user asks to close the window.
    pub(crate) static QUIT_REQUESTED: Cell<bool> = const { Cell::new(false) };
}

// ---------------------------------------------------------------------------
// Initialisation helper
// ---------------------------------------------------------------------------

/// Record the programme start time so [`Host::ticks_ms`] can compute
/// elapsed milliseconds.  Must be called once, before `doomgeneric_Create`.
pub fn init_start_time() {
    START_TIME.with(|t| t.set(Some(Instant::now())));
}

// ---------------------------------------------------------------------------
// The platform
// ---------------------------------------------------------------------------

/// The host platform the engine runs on.
pub struct Host;

impl Platform for Host {
    /// The winit event loop creates the window and GPU before
    /// `doomgeneric_Create` is called, so there is nothing left to do.
    fn init(&self) {
        log::debug!("Host::init: platform already initialised");
    }

    /// Upload `frame` (640 × 400 pixels in BGRA8 format) to the wgpu texture
    /// and blit it to the surface.
    fn draw_frame(&self, frame: &[u32]) {
        GPU.with_borrow(|opt| {
            if let Some(gpu) = opt.as_ref() {
                if let Err(e) = gpu.render(bytemuck::cast_slice(frame)) {
                    log::warn!("Host::draw_frame: render error: {e}");
                }
            } else {
                log::warn!("Host::draw_frame: GPU state not ready");
            }
        });
    }

    /// Sleep the calling thread; the engine throttles its loop with it.
    fn sleep_ms(&self, ms: u32) {
        std::thread::sleep(std::time::Duration::from_millis(u64::from(ms)));
    }

    /// Milliseconds since [`init_start_time`].
    fn ticks_ms(&self) -> u32 {
        START_TIME.with(|t| {
            t.get()
                .map(|start| start.elapsed().as_millis() as u32)
                .unwrap_or(0)
        })
    }

    /// Pop the next keyboard event the event loop queued.
    fn key(&self) -> Option<(bool, u8)> {
        KEY_QUEUE.with_borrow_mut(VecDeque::pop_front)
    }

    /// Update the OS window title, e.g. to "The Ultimate DOOM".
    fn set_window_title(&self, title: &str) {
        WINDOW.with_borrow(|w| {
            if let Some(window) = w.as_ref() {
                window.set_title(title);
            }
        });
    }

    fn sound_init(&self) -> bool {
        audio::sound_init()
    }

    fn sound_shutdown(&self) {
        audio::sound_shutdown();
    }

    fn sound_start(&self, lump: &[u8], channel: usize, vol: i32, sep: i32) -> bool {
        audio::sound_start(lump, channel, vol, sep)
    }

    fn sound_stop(&self, channel: usize) {
        audio::sound_stop(channel);
    }

    fn sound_update(&self, channel: usize, vol: i32, sep: i32) {
        audio::sound_update(channel, vol, sep);
    }

    fn sound_is_playing(&self, channel: usize) -> bool {
        audio::sound_is_playing(channel)
    }

    fn music_init(&self) {
        audio::music_init();
    }

    fn music_play(&self, midi: &[u8], looping: bool) {
        audio::music_play(midi, looping);
    }

    fn music_stop(&self) {
        audio::music_stop();
    }

    fn music_pause(&self) {
        audio::music_pause();
    }

    fn music_resume(&self) {
        audio::music_resume();
    }

    fn music_set_volume(&self, volume: i32) {
        audio::music_set_volume(volume);
    }

    fn music_is_playing(&self) -> bool {
        audio::music_is_playing()
    }
}

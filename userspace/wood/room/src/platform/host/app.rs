//! The winit event loop that drives the engine on the host.
//!
//! ```text
//!  run()
//!   └─ EventLoop::run_app(&mut App)
//!       ├─ App::resumed()      → create Window + GpuState, store in TLS
//!       ├─ App::window_event() → push key events into TLS queue
//!       └─ App::about_to_wait()
//!               first call  → doomgeneric_Create(&Host, args)
//!               later calls → doomgeneric_Tick()
//!                                ├─ I_StartTic() → I_GetEvent() → Host::key()
//!                                └─ I_FinishUpdate() → Host::draw_frame()
//! ```
//!
//! [`Host`]'s methods (in the parent module) access window and GPU
//! resources through [`thread_local!`] statics, which is safe because
//! everything runs on the main thread.

use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::PhysicalKey,
    window::{Window, WindowId},
};

use room::engine::doomgeneric::{DOOMGENERIC_RESX, DOOMGENERIC_RESY};

use super::{
    gpu::GpuState, init_start_time, keys::to_doom_key, Host, GPU, KEY_QUEUE, QUIT_REQUESTED, WINDOW,
};

// ---------------------------------------------------------------------------
// App – the winit ApplicationHandler
// ---------------------------------------------------------------------------

/// Application state threaded through the winit event loop.
struct App {
    /// Command-line arguments to hand to `doomgeneric_Create`, which takes
    /// them on the first tick.
    args: Vec<String>,

    /// `true` after `doomgeneric_Create` has returned and the first tick
    /// has been executed, so that subsequent `about_to_wait` calls use
    /// `doomgeneric_Tick` instead.
    doom_initialized: bool,
}

/// Constructor and helpers for [`App`].
impl App {
    /// Build an `App` from the OS command-line arguments.
    fn new() -> Self {
        Self {
            args: std::env::args().collect(),
            doom_initialized: false,
        }
    }
}

/// Winit event-loop integration: window/GPU bring-up, input forwarding, and
/// the per-iteration game tick.
impl ApplicationHandler for App {
    /// Called when the application becomes active (e.g. on launch or resume).
    ///
    /// Creates the OS window, initialises wgpu, and stores both in the
    /// thread-local platform state so [`Host`]'s methods can reach them.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        log::info!(
            "Creating window ({}×{})",
            DOOMGENERIC_RESX,
            DOOMGENERIC_RESY
        );

        let window_attrs = Window::default_attributes()
            .with_title("room")
            .with_inner_size(LogicalSize::new(
                DOOMGENERIC_RESX as u32,
                DOOMGENERIC_RESY as u32,
            ))
            .with_resizable(false);

        let window = match event_loop.create_window(window_attrs) {
            Ok(w) => std::sync::Arc::new(w),
            Err(e) => {
                log::error!("Failed to create window: {e}");
                event_loop.exit();
                return;
            }
        };

        // Initialise the GPU renderer.
        match GpuState::new(window.clone()) {
            Ok(gpu) => {
                GPU.with_borrow_mut(|g| *g = Some(gpu));
                WINDOW.with_borrow_mut(|w| *w = Some(window));
                init_start_time();
                log::info!("GPU initialised");
            }
            Err(e) => {
                log::error!("Failed to initialise GPU: {e}");
                event_loop.exit();
            }
        }
    }

    /// Called for each windowed event (keyboard, resize, close request, …).
    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                log::info!("Close requested – exiting");
                QUIT_REQUESTED.with(|q| q.set(true));
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
                GPU.with_borrow_mut(|g| {
                    if let Some(gpu) = g.as_mut() {
                        gpu.resize(new_size);
                    }
                });
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    // Ignore auto-repeat key presses (key held down).
                    if event.repeat {
                        return;
                    }

                    if let Some(doom_key) = to_doom_key(code) {
                        let pressed = event.state == ElementState::Pressed;
                        KEY_QUEUE.with_borrow_mut(|q| q.push_back((pressed, doom_key)));
                    }
                }
            }

            _ => {}
        }
    }

    /// Called when all pending events have been processed.
    ///
    /// This is the main "game loop" hook: on the first call, the Doom engine
    /// is initialised; on subsequent calls, one game tick is executed.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Skip ticking if the GPU is not yet ready.
        let gpu_ready = GPU.with_borrow(|g| g.is_some());
        if !gpu_ready {
            return;
        }

        if QUIT_REQUESTED.with(|q| q.get()) {
            event_loop.exit();
            return;
        }

        if !self.doom_initialized {
            // First tick: initialise the engine.
            // SAFETY: guarded by `doom_initialized`, so this runs once.
            let args = std::mem::take(&mut self.args);
            unsafe {
                room::engine::doomgeneric::doomgeneric_Create(&Host, args);
            }
            self.doom_initialized = true;
        } else {
            // Subsequent ticks: advance the game by one tick.
            room::engine::doom::main::doomgeneric_Tick();
        }

        // Request a redraw so winit doesn't throttle to zero FPS while idle.
        WINDOW.with_borrow(|w| {
            if let Some(win) = w.as_ref() {
                win.request_redraw();
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run the game on the host.
///
/// Initialises logging, builds the winit event loop, configures continuous
/// polling so the Doom tick can run as fast as possible, constructs the
/// [`App`], and hands control to winit.  On event-loop failure the process
/// exits with status `1`.
pub fn run() {
    // Initialise the logger.  Set `RUST_LOG=debug` for verbose output.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("room – doomgeneric Rust port");

    let event_loop = EventLoop::new().expect("failed to create event loop");

    // Poll continuously so we can tick the game as fast as possible.
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

    let mut app = App::new();
    if let Err(e) = event_loop.run_app(&mut app) {
        log::error!("Event loop error: {e}");
        std::process::exit(1);
    }
}

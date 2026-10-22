//! Platform implementation for the doomgeneric engine.
//!
//! This module provides the six C-callable functions that doomgeneric
//! requires from a platform back-end:
//!
//! | C function | Purpose |
//! |------------|---------|
//! | `DG_Init` | Platform initialisation (window / GPU already set up) |
//! | `DG_DrawFrame` | Present the current frame from `DG_ScreenBuffer` |
//! | `DG_SleepMs` | Sleep for a number of milliseconds |
//! | `DG_GetTicksMs` | Return elapsed milliseconds since startup |
//! | `DG_GetKey` | Pop the next keyboard event from the queue |
//! | `DG_SetWindowTitle` | Update the OS window title |
//!
//! All functions are exported with `#[no_mangle]` so the C linker can
//! resolve the symbols when linking the doomgeneric static library.
//!
//! ## Global state
//!
//! Because these functions are called from C code (which has no concept of
//! Rust ownership), all shared mutable state is stored in [`thread_local!`]
//! statics protected by [`std::cell::RefCell`].  This is safe because
//! **all** calls originate from the main thread (the winit event loop).
//!
//! This design is an intentional *functional approximation* of the original
//! C globals, to be refactored towards idiomatic Rust in future iterations.

pub mod keys;

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ffi::c_char;
use std::ffi::CStr;
use std::sync::Arc;
use std::time::Instant;

use winit::window::Window;

use crate::gpu::GpuState;

// ---------------------------------------------------------------------------
// Thread-local platform state
// ---------------------------------------------------------------------------

// GPU, window, keyboard and time state used by the DG_* callbacks.
// Doc comments are deliberately omitted on the macro invocation since
// rustdoc does not process comments attached to `thread_local!` calls.
thread_local! {
    // GPU rendering state.  `None` until `GpuState` is created in `resumed`.
    pub(crate) static GPU: RefCell<Option<GpuState>> = const { RefCell::new(None) };

    // OS window handle used for title updates.
    pub(crate) static WINDOW: RefCell<Option<Arc<Window>>> = const { RefCell::new(None) };

    // Keyboard event queue consumed by `DG_GetKey`.
    // Each entry is `(pressed, doom_key_byte)`.
    pub(crate) static KEY_QUEUE: RefCell<VecDeque<(bool, u8)>> =
        const { RefCell::new(VecDeque::new()) };

    // Absolute time of programme launch, used by `DG_GetTicksMs`.
    static START_TIME: Cell<Option<Instant>> = const { Cell::new(None) };

    // Set to `true` when the user asks to close the window.
    pub(crate) static QUIT_REQUESTED: Cell<bool> = const { Cell::new(false) };
}

// ---------------------------------------------------------------------------
// Initialisation helper
// ---------------------------------------------------------------------------

/// Record the programme start time so [`DG_GetTicksMs`] can compute elapsed
/// milliseconds.  Must be called once, before `doomgeneric_Create`.
pub fn init_start_time() {
    START_TIME.with(|t| t.set(Some(Instant::now())));
}

// ---------------------------------------------------------------------------
// DG_* callbacks (C-callable)
// ---------------------------------------------------------------------------

/// Initialise the platform back-end.
///
/// Called by doomgeneric once, inside `doomgeneric_Create`, before the first
/// game tick.  Because the window and GPU are created by the winit event loop
/// *before* `doomgeneric_Create` is called, this function is a no-op: all
/// resources are already available.
#[no_mangle]
pub extern "C" fn DG_Init() {
    log::debug!("DG_Init: platform already initialised");
}

/// Present the current Doom frame to the screen.
///
/// Called by the engine's `I_FinishUpdate` function at the end of each
/// rendered frame.  Reads pixels from the C-owned `DG_ScreenBuffer` (640 ×
/// 400 pixels in BGRA8 format) and uploads them to the wgpu texture, then
/// issues a render pass that blits the texture to the surface.
///
/// # Safety
///
/// `DG_ScreenBuffer` must be non-null and point to at least
/// `DOOMGENERIC_RESX * DOOMGENERIC_RESY * 4` bytes of valid memory.
/// This is guaranteed by `doomgeneric_Create`, which allocates the buffer
/// before calling `DG_Init`.
#[no_mangle]
pub extern "C" fn DG_DrawFrame() {
    // SAFETY: DG_ScreenBuffer is allocated by doomgeneric_Create and is
    // valid for DOOMGENERIC_PIXELS * 4 bytes.
    let pixel_bytes = unsafe {
        let ptr = room::doom::doomgeneric::DG_ScreenBuffer as *const u8;
        if ptr.is_null() {
            log::warn!("DG_DrawFrame: DG_ScreenBuffer is null, skipping frame");
            return;
        }
        std::slice::from_raw_parts(ptr, room::doom::doomgeneric::DOOMGENERIC_PIXELS * 4)
    };

    GPU.with_borrow(|opt| {
        if let Some(gpu) = opt.as_ref() {
            if let Err(e) = gpu.render(pixel_bytes) {
                log::warn!("DG_DrawFrame: render error: {e}");
            }
        } else {
            log::warn!("DG_DrawFrame: GPU state not ready");
        }
    });
}

/// Sleep the calling thread for `ms` milliseconds.
///
/// Used by the engine to throttle the game loop when running ahead of
/// the target tick rate.
#[no_mangle]
pub extern "C" fn DG_SleepMs(ms: u32) {
    std::thread::sleep(std::time::Duration::from_millis(u64::from(ms)));
}

/// Return the number of milliseconds elapsed since `doomgeneric_Create`.
///
/// The engine uses this to drive its fixed-rate game loop.
#[no_mangle]
pub extern "C" fn DG_GetTicksMs() -> u32 {
    START_TIME.with(|t| {
        t.get()
            .map(|start| start.elapsed().as_millis() as u32)
            .unwrap_or(0)
    })
}

/// Pop the next keyboard event from the queue.
///
/// Returns `1` if an event was available, `0` if the queue is empty.
/// On success, writes the pressed state (1 = pressed, 0 = released) to
/// `*pressed` and the Doom key byte to `*doom_key`.
///
/// # Safety
///
/// `pressed` and `doom_key` must be valid, non-null pointers to writable
/// memory for their respective types.
#[no_mangle]
pub extern "C" fn DG_GetKey(pressed: *mut i32, doom_key: *mut u8) -> i32 {
    KEY_QUEUE.with_borrow_mut(|q| {
        if let Some((is_pressed, key)) = q.pop_front() {
            // SAFETY: caller guarantees the pointers are valid.
            unsafe {
                *pressed = i32::from(is_pressed);
                *doom_key = key;
            }
            1
        } else {
            0
        }
    })
}

/// Update the OS window title.
///
/// Called by the engine when it discovers the game description from the
/// loaded WAD file (e.g. "The Ultimate DOOM").
///
/// # Safety
///
/// `title` must be a valid, null-terminated C string for the duration of
/// this call.
#[no_mangle]
pub extern "C" fn DG_SetWindowTitle(title: *const c_char) {
    if title.is_null() {
        return;
    }

    // SAFETY: caller guarantees `title` is a valid C string.
    let s = unsafe { CStr::from_ptr(title) };
    let title_str = s.to_string_lossy();

    WINDOW.with_borrow(|w| {
        if let Some(window) = w.as_ref() {
            window.set_title(&title_str);
        }
    });
}

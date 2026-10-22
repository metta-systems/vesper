//! Headless tick, frame, and window-title counters used by test and benchmark
//! harnesses.
//!
//! When the engine runs without a real window or audio device (e.g.
//! integration tests, profiling runs, or the benchmark binary), there is no
//! OS-supplied wall clock to drive `I_GetTime` and no SDL window to receive a
//! title.  The helpers in this module provide a minimal stand-in:
//!
//! - A monotonically increasing virtual millisecond counter.
//! - A frame-presented counter that the GPU layer bumps after each draw.
//! - A latched window title so tests can assert what the engine asked to be
//!   displayed.
//!
//! All state lives in `thread_local!` cells, so each test thread has an
//! independent counter and there is no synchronization cost.

use std::cell::Cell;

thread_local! {
    /// Virtual wall-clock in milliseconds for the current thread, advanced
    /// only by explicit calls to [`bump_virtual_ms`].
    static VIRTUAL_MS: Cell<u32> = const { Cell::new(0) };
    /// Number of frames the GPU layer has reported as presented on this
    /// thread.
    static FRAMES: Cell<u64> = const { Cell::new(0) };
    /// Most recent window title the engine asked to display on this thread,
    /// or `None` if [`note_title`] has never been called.
    static TITLE: Cell<Option<&'static str>> = const { Cell::new(None) };
}

/// One Doom tic measured in milliseconds.
///
/// Doom runs at 35 tics per second, so `1000 / 35 = 28` ms (integer truncated,
/// matching the C engine's `1000/TICRATE` computation).
pub const TICK_MS: u32 = 1000 / 35;

/// Advance the virtual wall-clock for the current thread by `delta`
/// milliseconds.
///
/// Used by tests to fast-forward the engine without sleeping.  No overflow
/// check is performed; callers are expected to keep totals well below
/// `u32::MAX` (~49 days of virtual time).
pub fn bump_virtual_ms(delta: u32) {
    VIRTUAL_MS.with(|v| v.set(v.get() + delta));
}

/// Read the current virtual wall-clock in milliseconds for this thread.
pub fn virtual_ms() -> u32 {
    VIRTUAL_MS.with(|v| v.get())
}

/// Increment the per-thread frame counter by one.
///
/// Called from the GPU layer after each successful present so test harnesses
/// can assert that a given number of frames were produced.
pub fn note_frame() {
    FRAMES.with(|f| f.set(f.get() + 1));
}

/// Read the per-thread count of frames presented so far.
pub fn frame_count() -> u64 {
    FRAMES.with(|f| f.get())
}

/// Latch the window title that the engine requested for this thread.
///
/// The title must outlive the test (hence the `&'static str` bound); the
/// engine only ever passes literals here.
pub fn note_title(s: &'static str) {
    TITLE.with(|t| t.set(Some(s)));
}

/// Read the most-recently-latched window title for this thread, or `None` if
/// no title has been set.
pub fn title() -> Option<&'static str> {
    TITLE.with(|t| t.get())
}

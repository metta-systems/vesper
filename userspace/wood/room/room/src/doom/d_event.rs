//! Rust port of vendor/doomgeneric/d_event.c.
//!
//! Input event handling: the `event_t` struct, a fixed-capacity ring-buffer
//! queue, and the two public functions that post and consume events.
//!
//! Events are generated asynchronously by I/O drivers (`i_input.rs`,
//! `i_video.rs`, etc.) and consumed synchronously each game tic by
//! `G_Responder` / `M_Responder` / `F_Responder` in the main loop.  The
//! queue is intentionally lossy: if 64 events arrive before any are drained
//! the oldest are silently overwritten (ring-buffer wrap-around), matching
//! the behaviour of the original C implementation.
//!
//! Rust differences from C:
//! - The queue state (`EVENTS`, `EVENT_HEAD`, `EVENT_TAIL`) and the capacity
//!   constant (`MAXEVENTS`) are private to this module; C exposed all of them
//!   as file-static locals, which is equivalent visibility.
//! - `event_t.type_` is spelled with a trailing underscore to avoid collision
//!   with the Rust keyword `type`.

#![allow(non_upper_case_globals, non_snake_case)]

use std::ffi::c_int;

/// Maximum number of events that can be queued before the oldest is dropped.
///
/// Corresponds to `#define MAXEVENTS 64` in `d_event.c`.
const MAXEVENTS: usize = 64;

/// A single input event delivered to the game logic.
///
/// Corresponds to `event_t` in `d_event.h`.  The meaning of `data1`-`data4`
/// depends on `type_`:
///
/// | `type_` (evtype_t) | `data1` | `data2` | `data3` | `data4` |
/// |--------------------|---------|---------|---------|---------|
/// | `ev_keydown` / `ev_keyup` | key code (from `doomkeys`) | ASCII char pressed | - | - |
/// | `ev_mouse` | button bitfield (bit 0=left, 1=right, 2=middle) | X delta (turn) | Y delta (fwd/back) | - |
/// | `ev_joystick` | button bitfield | X axis (turn) | Y axis (fwd/back) | Z axis (strafe) |
/// | `ev_quit` | - | - | - | - |
///
/// The `type_` field is typed as `c_int` rather than an enum to preserve
/// exact ABI compatibility with the C `evtype_t` enum (which is an `int` in
/// C).
#[repr(C)]
#[derive(Copy, Clone)]
pub struct event_t {
    /// Event type discriminant; one of the `ev_*` constants from `evtype_t`.
    pub type_: c_int,
    /// Primary event datum; interpretation depends on `type_`.
    pub data1: c_int,
    /// Secondary event datum; interpretation depends on `type_`.
    pub data2: c_int,
    /// Tertiary event datum; interpretation depends on `type_`.
    pub data3: c_int,
    /// Quaternary event datum; used only by `ev_joystick` for the strafe axis.
    pub data4: c_int,
}

/// A zero-initialised `event_t` used as the array fill value.
///
/// Rust requires const-evaluable array initialisers; this sentinel satisfies
/// that constraint without adding runtime cost.
const DEFAULT_EVENT: event_t = event_t {
    type_: 0,
    data1: 0,
    data2: 0,
    data3: 0,
    data4: 0,
};

/// Ring-buffer storage for pending events.  Indexed by `EVENT_HEAD` and `EVENT_TAIL`.
///
/// Corresponds to `static event_t events[MAXEVENTS]` in `d_event.c`.
static mut EVENTS: [event_t; MAXEVENTS] = [DEFAULT_EVENT; MAXEVENTS];

/// Write index into [`EVENTS`]: next slot to be filled by [`D_PostEvent`].
///
/// Corresponds to `static int eventhead` in `d_event.c`.
static mut EVENT_HEAD: usize = 0;

/// Read index into [`EVENTS`]: next slot to be drained by [`D_PopEvent`].
///
/// Corresponds to `static int eventtail` in `d_event.c`.
static mut EVENT_TAIL: usize = 0;

/// Enqueue an input event into the global event ring-buffer.
///
/// Corresponds to `D_PostEvent` in `d_event.c`.  Called by I/O functions
/// (keyboard, mouse, joystick drivers) whenever input is detected.
///
/// The event pointed to by `ev` is copied into the queue at `EVENT_HEAD`;
/// the head index then advances modulo `MAXEVENTS` (64).  If the queue is full,
/// the oldest unread event is silently overwritten (same behaviour as C).
///
/// # Safety
/// `ev` must be a valid, aligned, non-null pointer to an initialised
/// `event_t` for the duration of this call.  This function is called from C
/// code, so the pointer validity guarantee rests with the caller.
#[no_mangle]
pub extern "C" fn D_PostEvent(ev: *const event_t) {
    unsafe {
        EVENTS[EVENT_HEAD] = *ev;
        EVENT_HEAD = (EVENT_HEAD + 1) % MAXEVENTS;
    }
}

/// Dequeue and return the oldest pending input event, or null if the queue is empty.
///
/// Corresponds to `D_PopEvent` in `d_event.c`.  Each call to `D_DoomLoop`
/// drains all available events via successive calls to this function, passing
/// each to the chain of responders (`G_Responder`, `M_Responder`, etc.).
///
/// Returns a mutable pointer into the internal ring-buffer slot.  The pointer
/// remains valid until the next call to [`D_PostEvent`] that overwrites the
/// same slot (i.e., after another full cycle of `MAXEVENTS` (64) posts).
/// Returns `null_mut()` when no events are pending (`EVENT_TAIL == EVENT_HEAD`).
///
/// # Safety
/// The returned pointer aliases internal mutable state.  Callers must not
/// retain the pointer across subsequent calls to [`D_PostEvent`] that could
/// recycle the same slot.  This function is called from C code which observes
/// those constraints.
#[no_mangle]
pub extern "C" fn D_PopEvent() -> *mut event_t {
    unsafe {
        if EVENT_TAIL == EVENT_HEAD {
            return std::ptr::null_mut();
        }
        let result = &mut EVENTS[EVENT_TAIL];
        EVENT_TAIL = (EVENT_TAIL + 1) % MAXEVENTS;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn reset_queue() {
        EVENTS = [DEFAULT_EVENT; MAXEVENTS];
        EVENT_HEAD = 0;
        EVENT_TAIL = 0;
    }

    #[test]
    fn post_then_pop_returns_same_values() {
        unsafe {
            reset_queue();
            let ev = event_t {
                type_: 1,
                data1: 42,
                data2: 99,
                data3: 0,
                data4: 0,
            };
            D_PostEvent(&ev);
            let result = D_PopEvent();
            assert!(!result.is_null());
            let result = &*result;
            assert_eq!(result.type_, 1);
            assert_eq!(result.data1, 42);
            assert_eq!(result.data2, 99);
            assert_eq!(D_PopEvent(), std::ptr::null_mut());
        }
    }

    #[test]
    fn pop_on_empty_returns_null() {
        unsafe {
            reset_queue();
            assert!(D_PopEvent().is_null());
        }
    }

    #[test]
    fn wrap_around_at_maxevents() {
        unsafe {
            reset_queue();
            let max = MAXEVENTS - 1;
            for i in 0..max {
                let ev = event_t {
                    type_: i as c_int,
                    data1: i as c_int,
                    data2: 0,
                    data3: 0,
                    data4: 0,
                };
                D_PostEvent(&ev);
            }
            assert_eq!(EVENT_HEAD, max);
            for i in 0..max {
                let result = D_PopEvent();
                assert!(!result.is_null());
                assert_eq!((*result).type_, i as c_int);
                assert_eq!((*result).data1, i as c_int);
            }
            assert!(D_PopEvent().is_null());
        }
    }
}

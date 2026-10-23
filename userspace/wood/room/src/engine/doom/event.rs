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
//! - The queue state (`events`, `eventhead`, `eventtail`) lives in private
//!   fields of [`EventState`], held in the [`EVENT`] global, and the
//!   capacity constant (`MAXEVENTS`) is private to this module; C kept all
//!   of them file-static, which is equivalent visibility.
//! - `event_t.kind` is spelled with a trailing underscore to avoid collision
//!   with the Rust keyword `type`.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::engine::global::Global;

/// Maximum number of events that can be queued before the oldest is dropped.
///
/// Corresponds to `#define MAXEVENTS 64` in `d_event.c`.
const MAXEVENTS: usize = 64;

/// A single input event delivered to the game logic.
///
/// Corresponds to `event_t` in `d_event.h`.  The meaning of `data1`-`data4`
/// depends on `kind`:
///
/// | `kind` (evtype_t) | `data1` | `data2` | `data3` | `data4` |
/// |--------------------|---------|---------|---------|---------|
/// | `ev_keydown` / `ev_keyup` | key code (from `interface::keys`) | ASCII char pressed | - | - |
/// | `ev_mouse` | button bitfield (bit 0=left, 1=right, 2=middle) | X delta (turn) | Y delta (fwd/back) | - |
/// | `ev_joystick` | button bitfield | X axis (turn) | Y axis (fwd/back) | Z axis (strafe) |
/// | `ev_quit` | - | - | - | - |
///
/// The `kind` field is typed as `i32` rather than an enum, mirroring the C
/// `evtype_t` enum (which is an `int` in C).
#[derive(Copy, Clone)]
pub struct event_t {
    /// Event type discriminant; one of the `ev_*` constants from `evtype_t`.
    pub kind: i32,
    /// Primary event datum; interpretation depends on `kind`.
    pub data1: i32,
    /// Secondary event datum; interpretation depends on `kind`.
    pub data2: i32,
    /// Tertiary event datum; interpretation depends on `kind`.
    pub data3: i32,
    /// Quaternary event datum; used only by `ev_joystick` for the strafe axis.
    pub data4: i32,
}

/// A zero-initialised `event_t` used as the array fill value.
///
/// Rust requires const-evaluable array initialisers; this sentinel satisfies
/// that constraint without adding runtime cost.
const DEFAULT_EVENT: event_t = event_t {
    kind: 0,
    data1: 0,
    data2: 0,
    data3: 0,
    data4: 0,
};

/// Event queue state (C `d_event.c` globals).
pub struct EventState {
    /// Ring-buffer storage for pending events.  Indexed by `eventhead` and
    /// `eventtail`. [`D_PopEvent`] hands out pointers into it, so it stays
    /// in place inside [`EVENT`].
    ///
    /// Corresponds to `static event_t events[MAXEVENTS]` in `d_event.c`.
    events: [event_t; MAXEVENTS],
    /// Write index into `events`: next slot to be filled by [`D_PostEvent`].
    ///
    /// Corresponds to `static int eventhead` in `d_event.c`.
    eventhead: usize,
    /// Read index into `events`: next slot to be drained by [`D_PopEvent`].
    ///
    /// Corresponds to `static int eventtail` in `d_event.c`.
    eventtail: usize,
}

impl EventState {
    /// An empty queue.
    pub const fn new() -> Self {
        Self {
            events: [DEFAULT_EVENT; MAXEVENTS],
            eventhead: 0,
            eventtail: 0,
        }
    }
}

impl Default for EventState {
    fn default() -> Self {
        Self::new()
    }
}

/// The event queue.
pub static EVENT: Global<EventState> = Global::new(EventState::new());

/// Enqueue an input event into the global event ring-buffer.
///
/// Corresponds to `D_PostEvent` in `d_event.c`.  Called by I/O functions
/// (keyboard, mouse, joystick drivers) whenever input is detected.
///
/// The event `ev` is copied into the queue at `eventhead`; the head index
/// then advances modulo `MAXEVENTS` (64).  If the queue is full, the oldest
/// unread event is silently overwritten (same behaviour as C).
pub fn D_PostEvent(ev: &event_t) {
    let event = *ev;
    let mut queue = EVENT.borrow_mut();
    let head = queue.eventhead;
    queue.events[head] = event;
    queue.eventhead = (head + 1) % MAXEVENTS;
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
/// Returns `null_mut()` when no events are pending (`eventtail == eventhead`).
///
/// # Safety
/// The returned pointer aliases internal mutable state.  Callers must not
/// retain the pointer across subsequent calls to [`D_PostEvent`] that could
/// recycle the same slot.
pub fn D_PopEvent() -> *mut event_t {
    let tail = {
        let mut queue = EVENT.borrow_mut();
        let tail = queue.eventtail;
        if tail == queue.eventhead {
            return std::ptr::null_mut();
        }
        queue.eventtail = (tail + 1) % MAXEVENTS;
        tail
    };
    // The responders read the event through this pointer, so it is taken
    // from the slot's stable address inside `EVENT`.
    unsafe { &raw mut (*EVENT.as_ptr()).events[tail] }
}

#[cfg(test)]
mod tests {
    use {super::*, std::sync::Mutex};

    /// Serialises the tests: they share the global event queue, and cargo
    /// runs tests in parallel.
    static QUEUE_LOCK: Mutex<()> = Mutex::new(());

    /// Empty the queue and hold it for the calling test until the returned
    /// guard drops.
    fn reset_queue() -> std::sync::MutexGuard<'static, ()> {
        let guard = QUEUE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *EVENT.borrow_mut() = EventState::new();
        guard
    }

    #[test]
    fn post_then_pop_returns_same_values() {
        unsafe {
            let _queue = reset_queue();
            let ev = event_t {
                kind: 1,
                data1: 42,
                data2: 99,
                data3: 0,
                data4: 0,
            };
            D_PostEvent(&ev);
            let result = D_PopEvent();
            assert!(!result.is_null());
            let result = &*result;
            assert_eq!(result.kind, 1);
            assert_eq!(result.data1, 42);
            assert_eq!(result.data2, 99);
            assert_eq!(D_PopEvent(), std::ptr::null_mut());
        }
    }

    #[test]
    fn pop_on_empty_returns_null() {
        let _queue = reset_queue();
        assert!(D_PopEvent().is_null());
    }

    /// The queue starts empty, with both cursors at slot 0.
    #[test]
    fn new_queue_is_empty() {
        let queue = EventState::new();
        assert_eq!(queue.eventhead, 0);
        assert_eq!(queue.eventtail, 0);
        assert!(queue.events.iter().all(|event| event.kind == 0));
    }

    #[test]
    fn wrap_around_at_maxevents() {
        unsafe {
            let _queue = reset_queue();
            let max = MAXEVENTS - 1;
            for i in 0..max {
                let ev = event_t {
                    kind: i as i32,
                    data1: i as i32,
                    data2: 0,
                    data3: 0,
                    data4: 0,
                };
                D_PostEvent(&ev);
            }
            assert_eq!(EVENT.borrow().eventhead, max);
            for i in 0..max {
                let result = D_PopEvent();
                assert!(!result.is_null());
                assert_eq!((*result).kind, i as i32);
                assert_eq!((*result).data1, i as i32);
            }
            assert!(D_PopEvent().is_null());
        }
    }
}

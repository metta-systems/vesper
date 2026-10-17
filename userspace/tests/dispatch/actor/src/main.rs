#![no_std]
#![no_main]

//! Dispatch-test EL0 component. Its loader passes the address of four words
//! in a page mapped writable in this component only (`x0`): the role, the
//! wire key of a Notification in this component's table, and two words for
//! the component's counts. A *waiter* waits on the Notification forever and
//! counts each wakeup; a *kicker* signals it once and then counts forever
//! without blocking or yielding, so it stops only when the nucleus stops it.
//! The layout is shared with `dispatch-test`; keep the two in sync.

use {
    core::sync::atomic::{AtomicU64, Ordering},
    libobject::{CapError, NotificationKey, RawKey},
};

libuser::entry!(main);

/// Words of the page.
const ROLE: usize = 0;
const NOTIFICATION: usize = 1;
/// Wakeups seen by a waiter, or spins counted by a kicker.
const COUNT: usize = 2;

const WAITER: u64 = 0;
const KICKER: u64 = 1;
/// Waits with a timeout nobody signals within, and counts each timeout.
const TIMED: u64 = 2;

/// How long a timed waiter waits: ten milliseconds.
const TIMEOUT_NS: u64 = 10_000_000;

/// Stop on an unexpected error (`CapError` has no `Debug`).
fn fail(what: &str, error: CapError) -> ! {
    panic!("dispatch actor: {what} failed: {:?}", error.code())
}

fn main(page: u64) -> ! {
    // SAFETY: the loader maps the page for this component before it starts;
    // only this component writes the count.
    let words = unsafe { core::slice::from_raw_parts(page as *const AtomicU64, 4) };
    let notification = NotificationKey::from_key(RawKey::from_wire(
        words[NOTIFICATION].load(Ordering::Relaxed),
    ));
    match words[ROLE].load(Ordering::Relaxed) {
        WAITER => loop {
            notification
                .wait(NotificationKey::WAIT_INFINITE)
                .unwrap_or_else(|error| fail("Notification.Wait", error));
            words[COUNT].fetch_add(1, Ordering::Release);
        },
        KICKER => {
            notification
                .signal(1)
                .unwrap_or_else(|error| fail("Notification.Signal", error));
            loop {
                words[COUNT].fetch_add(1, Ordering::Relaxed);
                core::hint::spin_loop();
            }
        }
        TIMED => loop {
            match notification.wait(TIMEOUT_NS) {
                Err(CapError::TimedOut { value: 0 }) => {
                    words[COUNT].fetch_add(1, Ordering::Release);
                }
                Err(error) => fail("Notification.Wait with a timeout", error),
                Ok(_) => panic!("dispatch actor: a timed wait was signalled"),
            }
        },
        _ => panic!("dispatch actor: unknown role"),
    }
}

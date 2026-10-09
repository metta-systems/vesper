#![no_std]
#![no_main]

//! Preemption-test EL0 component. Its loader passes the address of a counter
//! in `x0`, in a page mapped writable in this component only; the component
//! increments it forever and never blocks or yields, so it only stops running
//! when the nucleus preempts it.

use core::sync::atomic::{AtomicU64, Ordering};

libuser::entry!(main);

fn main(counter: u64) -> ! {
    // SAFETY: the loader maps the counter page for this component before it
    // starts, and nothing else writes the word.
    let counter = unsafe { AtomicU64::from_ptr(counter as *mut u64) };
    loop {
        counter.fetch_add(1, Ordering::Relaxed);
        core::hint::spin_loop();
    }
}

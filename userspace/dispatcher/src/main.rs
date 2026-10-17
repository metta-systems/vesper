#![no_std]
#![no_main]

//! The root dispatcher: the one policy that decides which Thread of the root
//! Scheduler runs.
//!
//! Kickstart shares the Scheduler's record table with it and hands it a key
//! to every managed Thread (see `DriverInit::dispatch`). The policy is
//! round-robin: take the next Thread whose record says it can run and
//! dispatch it for one slice. A Thread that blocks, uses up its budget or its
//! slice, or exits activates the dispatcher, and so does a Thread becoming
//! runnable or a budget arriving, so a woken Thread is not left waiting for
//! the slice of the one running. When nothing can run it waits with nothing
//! dispatched, which lets the idle Thread run.

use {
    libdevice_driver::{DispatchInit, driver_init, local_key},
    libobject::{
        CapError, RawKey,
        scheduler::{SchedulerKey, activation_mask},
        scheduler_record::ThreadRecord,
        thread::ThreadState,
    },
};

libuser::entry!(main);

/// How long a Thread runs before the dispatcher decides again.
const SLICE_NS: u64 = 10_000_000;

/// How many times a record read may retry before giving up.
const SNAPSHOT_RETRIES: u32 = 1_000;

/// Stop on an unexpected error (`CapError` has no `Debug`).
fn fail(what: &str, error: CapError) -> ! {
    panic!("root dispatcher: {what} failed: {:?}", error.code())
}

fn main(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    let DispatchInit {
        scheduler,
        records,
        stride,
        slot_count,
        first_thread_key_slot,
    } = init.dispatch;
    let scheduler = SchedulerKey::from_key(RawKey::from_wire(scheduler));
    let stride = usize::try_from(stride).unwrap_or(usize::MAX);
    let slot_count = usize::try_from(slot_count).unwrap_or(0);
    let record = |slot: usize| {
        // SAFETY: Kickstart maps the record table read-only for this
        // component, 64-byte aligned, and every slot up to `slot_count` lies
        // inside it; records are read only through atomics.
        unsafe { ThreadRecord::at((records as *const u8).add(slot * stride)) }
    };
    let thread_key = |slot: usize| {
        local_key(
            init.table,
            u32::try_from(first_thread_key_slot).unwrap_or(u32::MAX)
                + u32::try_from(slot).unwrap_or(u32::MAX),
        )
    };

    scheduler
        .configure_activation(activation_mask::ALL)
        .unwrap_or_else(|error| fail("ConfigureActivation", error));

    let mut next = 1;
    loop {
        // The first Thread at or after `next` that can run now.
        let chosen = (0..slot_count)
            .map(|offset| (next - 1 + offset) % slot_count + 1)
            .find_map(|slot| {
                let snapshot = record(slot).try_snapshot(SNAPSHOT_RETRIES)?;
                (snapshot.thread_state() == Some(ThreadState::Runnable)
                    && !snapshot.awaits_budget())
                .then_some((slot, snapshot.generation))
            });
        let Some((slot, generation)) = chosen else {
            // Nothing can run: wait for an event with nothing dispatched.
            scheduler
                .dispatch(None, u64::MAX, 0)
                .unwrap_or_else(|error| fail("Dispatch (idle)", error));
            continue;
        };
        next = slot % slot_count + 1;
        match scheduler.dispatch(Some(thread_key(slot)), SLICE_NS, generation) {
            // Ran, or the Thread changed between the read and the call (look
            // again).
            Ok(_) | Err(CapError::StaleGeneration { .. }) => {}
            // It cannot be dispatched right now; wait for something to change
            // rather than choose it again.
            Err(CapError::InvalidOperation) => {
                scheduler
                    .dispatch(None, u64::MAX, 0)
                    .unwrap_or_else(|error| fail("Dispatch (idle)", error));
            }
            Err(error) => fail("Dispatch", error),
        }
    }
}

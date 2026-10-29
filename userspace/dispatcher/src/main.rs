// Host builds only check that the crate builds; the dispatcher itself runs on
// Vesper.
#![cfg_attr(target_os = "vesper", no_std, no_main)]

//! The root dispatcher: the one policy that decides which Thread of the root
//! Scheduler runs.
//!
//! Kickstart shares the Scheduler's record table with it and hands it a key
//! to every managed Thread and each one's priority (see
//! `DriverInit::dispatch`). The policy is fixed priority with round-robin
//! within a level ([`root_dispatcher::Rounds`]): take the next Thread of the
//! highest level whose record says it can run and dispatch it for one slice.
//! There is no starvation guard: a runnable high-priority Thread always runs
//! before every normal one. A Thread that blocks, uses up its budget or its
//! slice, or exits activates the dispatcher, and so does a Thread becoming
//! runnable or a budget arriving, so a woken Thread is not left waiting for
//! the slice of the one running, and a woken high-priority Thread runs at
//! once. When nothing can run it waits with nothing dispatched, which lets
//! the nucleus idle.

#[cfg(target_os = "vesper")]
mod dispatcher {
    use {
        libdevice_driver::{DispatchInit, driver_init, local_key},
        libobject::{
            CapError, RawKey,
            scheduler::{SchedulerKey, activation_mask},
            scheduler_record::ThreadRecord,
            thread::ThreadState,
        },
        root_dispatcher::Rounds,
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
        let dispatch: &DispatchInit = &init.dispatch;
        let scheduler = SchedulerKey::from_key(RawKey::from_wire(dispatch.scheduler));
        let stride = usize::try_from(dispatch.stride).unwrap_or(usize::MAX);
        let slot_count = usize::try_from(dispatch.slot_count).unwrap_or(0);
        let records = dispatch.records;
        let record = |slot: usize| {
            // SAFETY: Kickstart maps the record table read-only for this
            // component, 64-byte aligned, and every slot up to `slot_count`
            // lies inside it; records are read only through atomics.
            unsafe { ThreadRecord::at((records as *const u8).add(slot * stride)) }
        };
        let thread_key = |slot: usize| {
            local_key(
                init.table,
                u32::try_from(dispatch.first_thread_key_slot).unwrap_or(u32::MAX)
                    + u32::try_from(slot).unwrap_or(u32::MAX),
            )
        };

        scheduler
            .configure_activation(activation_mask::ALL)
            .unwrap_or_else(|error| fail("ConfigureActivation", error));

        let mut rounds = Rounds::new(slot_count);
        loop {
            // The next Thread of the highest level that can run now.
            let chosen = rounds.choose(|slot| {
                let snapshot = record(slot).try_snapshot(SNAPSHOT_RETRIES)?;
                (snapshot.thread_state() == Some(ThreadState::Runnable)
                    && !snapshot.awaits_budget())
                .then_some((dispatch.priority(slot).0, snapshot.generation))
            });
            let Some((slot, generation)) = chosen else {
                // Nothing can run: wait for an event with nothing dispatched.
                scheduler
                    .dispatch(None, u64::MAX, 0)
                    .unwrap_or_else(|error| fail("Dispatch (idle)", error));
                continue;
            };
            match scheduler.dispatch(Some(thread_key(slot)), SLICE_NS, generation) {
                // Ran, or the Thread changed between the read and the call
                // (look again).
                Ok(_) | Err(CapError::StaleGeneration { .. }) => {}
                // It cannot be dispatched right now; wait for something to
                // change rather than choose it again.
                Err(CapError::InvalidOperation) => {
                    scheduler
                        .dispatch(None, u64::MAX, 0)
                        .unwrap_or_else(|error| fail("Dispatch (idle)", error));
                }
                Err(error) => fail("Dispatch", error),
            }
        }
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}

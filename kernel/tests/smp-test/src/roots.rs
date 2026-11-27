//! Per-core roots: Threads on a secondary core's root Scheduler.
//!
//! The boot Thread binds a dispatcher (an `EL1t` Thread of this image) to the
//! root Scheduler of the first secondary core, then drives it with system
//! calls from the boot core, so every step takes the kernel lock:
//!
//! - the secondary enters the dispatcher, its first Thread;
//! - a waiter on the secondary blocks on a Notification, and a Signal from the
//!   boot core wakes it there;
//! - a spinner on the secondary runs while the boot Thread runs on the boot
//!   core, until the secondary's timer stops it at the end of its finite
//!   budget; each run is charged the time it ran, measured on the secondary
//!   around the dispatch call;
//! - Transfer from the boot core's Time into a Time of the secondary is
//!   refused.
//!
//! The Threads and their Times are carved before the dispatcher is bound,
//! while nothing runs on the secondary; after the bind the boot Thread only
//! makes system calls and reads atomics.

use {
    crate::{check, wait_for},
    core::{
        cell::UnsafeCell,
        sync::atomic::{AtomicU64, Ordering},
    },
    kickstart::image::BootImage,
    libcomponent_loader::keys::BOOT_TABLE_GUARD,
    libexception::arch::aarch64::SavedContext,
    libobject::{
        CapError, KeySlot, NotificationKey, RawKey, Rights, TimeKey,
        scheduler::{SchedulerKey, activation_mask, activation_reason},
        time::INFINITE_BUDGET,
    },
    nucleus::{
        api::key_entry::KeyEntry,
        core_record,
        objects::{
            ExecutionContext, KeyTable, Thread, Time,
            carved::{Carved, CarvedId},
        },
    },
};

/// How long the dispatcher lets a worker run before it decides again. Under
/// QEMU without `-icount` a run lasts longer: the emulated timer fires on a
/// host timer, which comes about a quarter of the interval late on a macOS
/// host (`CNTP_CTL_EL0.ISTATUS` reads clear past `CNTP_CVAL_EL0` until it
/// does), and the run is charged what it ran.
const SLICE_NS: u64 = 10_000_000;

/// The spinner's budget: two and a half slices, so it runs out within a
/// slice and the run ends by exhaustion, never at the same time as a slice.
const SPINNER_BUDGET_NS: u64 = 25_000_000;

const STACK_BYTES: usize = 8 * 1024;

#[repr(C, align(16))]
struct Stack(UnsafeCell<[u8; STACK_BYTES]>);

// SAFETY: each stack is one Thread's, used only through its SP.
unsafe impl Sync for Stack {}

/// The dispatcher's, the waiter's and the spinner's stacks.
static STACKS: [Stack; 3] = [const { Stack(UnsafeCell::new([0; STACK_BYTES])) }; 3];

fn stack_top(index: usize) -> u64 {
    STACKS[index].0.get() as u64 + STACK_BYTES as u64
}

const WAITER: usize = 0;
const SPINNER: usize = 1;

/// The secondary's root Scheduler, as a wire key.
static SCHEDULER: AtomicU64 = AtomicU64::new(0);
/// The workers' wire keys, set as each is started (zero before).
static WORKERS: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
/// Every activation reason the dispatcher's calls returned. A call returns
/// a pending activation at once without running the Thread it names, so a
/// reason belongs to the round, not to the Thread of that call.
static REASONS: AtomicU64 = AtomicU64::new(0);
/// The waiter's Notification, as a wire key.
static NOTIFICATION: AtomicU64 = AtomicU64::new(0);
/// The core the dispatcher entered on, the waiter waited on and the waiter
/// woke on, plus one (zero: not yet; `u64::MAX`: the wait failed).
static ENTERED_ON: AtomicU64 = AtomicU64::new(0);
static WAITING_ON: AtomicU64 = AtomicU64::new(0);
static WOKEN_ON: AtomicU64 = AtomicU64::new(0);
/// The spinner's Time, as a wire key.
static SPINNER_TIME: AtomicU64 = AtomicU64::new(0);
/// The spinner's progress.
static SPINS: AtomicU64 = AtomicU64::new(0);
/// The spinner's runs checked, and the charge and call time of one that was
/// charged otherwise than it ran (zero: none).
static SPINNER_RUNS: AtomicU64 = AtomicU64::new(0);
static MISCHARGED_NS: AtomicU64 = AtomicU64::new(0);
static MISCHARGED_CALL_NS: AtomicU64 = AtomicU64::new(0);

/// What a dispatch call takes besides the spinner's run: the dispatcher's
/// kernel entry and exit and the two switches (about 0.2 ms under QEMU).
const SWITCH_MARGIN_NS: u64 = 1_000_000;

/// The bit the boot Thread signals the waiter with.
const WAKE_BIT: u64 = 1 << 3;

fn this_core_plus_one() -> u64 {
    u64::from(core_record::current().index) + 1
}

/// The secondary's dispatcher: tries each started worker for a slice and
/// records why its run ended; when none ran, waits with nothing dispatched
/// until an activation (a worker started or woken).
extern "C" fn dispatcher() -> ! {
    ENTERED_ON.store(this_core_plus_one(), Ordering::Release);
    let core = u64::from(core_record::current().index);
    let scheduler = SchedulerKey::from_key(RawKey::from_wire(SCHEDULER.load(Ordering::Acquire)));
    scheduler.configure_activation(activation_mask::ALL).ok();
    loop {
        let mut ran = false;
        for worker in &WORKERS {
            let key = worker.load(Ordering::Acquire);
            if key == 0 {
                continue;
            }
            let spinner = key == WORKERS[SPINNER].load(Ordering::Acquire);
            let left_before = if spinner { spinner_left() } else { 0 };
            let started = nucleus::timer::counter();
            // A Thread without a record keeps generation 0.
            let result = scheduler.dispatch_on(core, Some(RawKey::from_wire(key)), SLICE_NS, 0);
            let took_ns = ticks_to_ns(nucleus::timer::counter() - started);
            if spinner && result.is_ok() {
                account_spinner_run(left_before, spinner_left(), took_ns);
            }
            if let Ok(activation) = result {
                REASONS.fetch_or(activation.reasons, Ordering::AcqRel);
                ran = true;
            }
        }
        if !ran {
            scheduler.dispatch_on(core, None, u64::MAX, 0).ok();
        }
    }
}

/// Waits on the Notification once, records where it woke, then parks.
extern "C" fn waiter() -> ! {
    let notification =
        NotificationKey::from_key(RawKey::from_wire(NOTIFICATION.load(Ordering::Acquire)));
    WAITING_ON.store(this_core_plus_one(), Ordering::Release);
    let woken = notification.await_bits(NotificationKey::WAIT_INFINITE);
    WOKEN_ON.store(
        if woken.is_ok_and(|bits| bits & WAKE_BIT != 0) {
            this_core_plus_one()
        } else {
            u64::MAX
        },
        Ordering::Release,
    );
    loop {
        notification.await_bits(NotificationKey::WAIT_INFINITE).ok();
    }
}

/// Counts until its budget runs out.
extern "C" fn spinner() -> ! {
    loop {
        SPINS.fetch_add(1, Ordering::Relaxed);
    }
}

fn ticks_to_ns(ticks: u64) -> u64 {
    ticks.saturating_mul(1_000_000_000) / nucleus::timer::frequency()
}

/// The spinner's remaining budget, read on the dispatcher's core.
fn spinner_left() -> u64 {
    TimeKey::from_key(RawKey::from_wire(SPINNER_TIME.load(Ordering::Acquire)))
        .query()
        .map_or(u64::MAX, |query| query.budget_ns)
}

/// Check one dispatch of the spinner, which took `took_ns` from the call to
/// its return and took its budget from `left_before` to `left_after`: the
/// spinner ran only inside the call, so it is charged at most the call's
/// time, and at least that time less the dispatcher's own entry and exit. A
/// run that spent the budget ran at least what was left. A call that ran
/// nothing (a pending activation) charges nothing and is not counted.
fn account_spinner_run(left_before: u64, left_after: u64, took_ns: u64) {
    let charged = left_before.saturating_sub(left_after);
    if charged == 0 {
        return;
    }
    let exact = if left_after == 0 {
        took_ns + SWITCH_MARGIN_NS >= left_before
    } else {
        charged <= took_ns && took_ns - charged <= SWITCH_MARGIN_NS
    };
    SPINNER_RUNS.fetch_add(1, Ordering::AcqRel);
    if !exact {
        MISCHARGED_NS.store(charged, Ordering::Release);
        MISCHARGED_CALL_NS.store(took_ns, Ordering::Release);
    }
}

/// A key to `entry` in the next free boot-table slot.
fn grant(image: &mut BootImage<'_>, entry: KeyEntry) -> RawKey {
    let slot = image.slots.take(1);
    // SAFETY: the live boot table; nothing on another core uses it until the
    // dispatcher is bound, after the last grant.
    unsafe { &mut *KeyTable::at_carve(image.builder.boot_table_addr) }
        .insert(KeySlot(slot), entry, BOOT_TABLE_GUARD)
        .unwrap_or_else(|failure| panic!("smp-test grant failed: {:?}", failure.error.code()))
}

/// A Thread of the Scheduler `scheduler` in the boot `AddressSpace`,
/// configured to start with `saved`, bound to `time`; its key.
fn configured_thread(
    image: &mut BootImage<'_>,
    scheduler: CarvedId,
    saved: &SavedContext,
    time: Option<CarvedId>,
) -> RawKey {
    let thread = image.builder.carve_object(Thread::new(
        image.boot_as_id,
        ExecutionContext::Configured { saved: *saved },
    ));
    let object = image.nucleus.thread_mut(thread).expect("a carved Thread");
    object.scheduler = Some(scheduler);
    object.time = time;
    image.nucleus.join_scheduler(thread, scheduler);
    if let Some(time) = time {
        // SAFETY: the Time was carved for this Thread and nothing else names
        // it yet.
        unsafe {
            (*Carved::<Time>::validate(time).unwrap_or_else(|_| panic!("a carved Time")))
                .bind(thread);
        }
    }
    grant(image, KeyEntry::new_thread(thread, Rights::all(), 0))
}

/// Run the per-core root checks on the first secondary with a root
/// Scheduler; whether they all held.
pub fn check_roots(image: &mut BootImage<'_>) -> bool {
    let boot_core = core_record::current().index as usize;
    let Some((core, root)) = image
        .keys
        .core_roots
        .iter()
        .enumerate()
        .find_map(|(core, root)| root.filter(|_| core != boot_core).map(|root| (core, root)))
    else {
        return check(false, format_args!("a secondary core has a root Scheduler"));
    };
    let core_plus_one = core as u64 + 1;
    let scheduler_id = image
        .builder
        .entry(root.scheduler)
        .carved_id()
        .unwrap_or_else(|error| panic!("core {core} root Scheduler: {:?}", error.code()));
    let scheduler = SchedulerKey::from_key(root.scheduler);
    SCHEDULER.store(root.scheduler.to_wire(), Ordering::Release);
    let notification = image.notification();
    NOTIFICATION.store(notification.to_wire(), Ordering::Release);

    // ── Everything is carved before the secondary runs any of it ──
    let waiter_time = image
        .builder
        .carve_object(Time::root(core, INFINITE_BUDGET));
    let spinner_time = image
        .builder
        .carve_object(Time::root(core, SPINNER_BUDGET_NS));
    let spinner_time_key = grant(image, KeyEntry::new_carved(spinner_time, Rights::all(), 0));
    SPINNER_TIME.store(spinner_time_key.to_wire(), Ordering::Release);
    let dispatcher_key = configured_thread(
        image,
        scheduler_id,
        &SavedContext::el1t(dispatcher as *const () as u64, stack_top(2)),
        None,
    );
    let waiter_key = configured_thread(
        image,
        scheduler_id,
        &SavedContext::el1t(waiter as *const () as u64, stack_top(WAITER)),
        Some(waiter_time),
    );
    let spinner_key = configured_thread(
        image,
        scheduler_id,
        &SavedContext::el1t_interruptible(spinner as *const () as u64, stack_top(SPINNER)),
        Some(spinner_time),
    );

    let mut passed = true;
    // ── The secondary enters its first Thread ──
    scheduler
        .bind_dispatcher(dispatcher_key)
        .unwrap_or_else(|error| panic!("core {core} BindDispatcher failed: {:?}", error.code()));
    passed &= check(
        wait_for(
            || ENTERED_ON.load(Ordering::Acquire) == core_plus_one,
            core::hint::spin_loop,
        ),
        format_args!("core {core} enters its dispatcher, its first Thread"),
    );

    // ── A Signal from the boot core wakes a waiter on the secondary ──
    WORKERS[WAITER].store(waiter_key.to_wire(), Ordering::Release);
    scheduler
        .start_thread(waiter_key)
        .unwrap_or_else(|error| panic!("waiter StartThread failed: {:?}", error.code()));
    let idle = &core_record::of(core)
        .expect("the secondary's record")
        .idling;
    // It waits once the secondary has nothing left to run.
    let blocked = wait_for(
        || WAITING_ON.load(Ordering::Acquire) == core_plus_one && idle.load(Ordering::Acquire),
        core::hint::spin_loop,
    );
    passed &= check(blocked, format_args!("the waiter blocks on core {core}"));
    NotificationKey::from_key(notification)
        .signal(WAKE_BIT)
        .unwrap_or_else(|error| panic!("Signal failed: {:?}", error.code()));
    let woken_on = || WOKEN_ON.load(Ordering::Acquire);
    passed &= check(
        wait_for(|| woken_on() != 0, core::hint::spin_loop) && woken_on() == core_plus_one,
        format_args!(
            "a Signal from core {boot_core} wakes it on core {core}: {}",
            woken_on()
        ),
    );

    // ── Two Threads on two cores at once ──
    WORKERS[SPINNER].store(spinner_key.to_wire(), Ordering::Release);
    scheduler
        .start_thread(spinner_key)
        .unwrap_or_else(|error| panic!("spinner StartThread failed: {:?}", error.code()));
    passed &= check(
        wait_for(|| SPINS.load(Ordering::Relaxed) != 0, core::hint::spin_loop),
        format_args!("the spinner starts on core {core}"),
    );
    // The boot Thread runs with interrupts masked and never yields: the
    // spinner advancing while it looks runs on another core at the same time.
    let before = SPINS.load(Ordering::Relaxed);
    let parallel = wait_for(
        || SPINS.load(Ordering::Relaxed) > before + 1000,
        core::hint::spin_loop,
    );
    passed &= check(
        parallel,
        format_args!("the spinner runs on core {core} while core {boot_core} runs"),
    );
    let exhausted = wait_for(
        || REASONS.load(Ordering::Acquire) & activation_reason::EXHAUSTED != 0,
        core::hint::spin_loop,
    );
    passed &= check(
        exhausted,
        format_args!("core {core}'s timer stops the spinner at the end of its budget"),
    );
    let left = TimeKey::from_key(spinner_time_key)
        .query()
        .map(|query| query.budget_ns);
    passed &= check(
        matches!(left, Ok(0)),
        format_args!("the spinner's Time is charged its whole budget"),
    );
    // Each run is charged what it ran, measured on core 1 around the call.
    let runs = SPINNER_RUNS.load(Ordering::Acquire);
    let (mischarged, call) = (
        MISCHARGED_NS.load(Ordering::Acquire),
        MISCHARGED_CALL_NS.load(Ordering::Acquire),
    );
    passed &= check(
        runs >= 2 && mischarged == 0,
        format_args!(
            "each of the spinner's {runs} runs on core {core} is charged the time it ran \
             (a mischarge: {mischarged} ns for a {call} ns call)"
        ),
    );

    // ── Transfer into another core's Time ──
    let refused = TimeKey::from_key(image.keys.boot_time)
        .transfer(&TimeKey::from_key(spinner_time_key), 1_000);
    passed &= check(
        matches!(refused, Err(CapError::InvalidOperation)),
        format_args!("Transfer from core {boot_core}'s Time to core {core}'s is refused"),
    );
    passed
}

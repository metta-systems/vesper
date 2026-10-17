#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! dispatch-test: a userspace root dispatcher end to end.
//!
//! Kickstart loads the privileged interrupt-controller component and the
//! nucleus arms the timer. The boot Thread (which stays on the kernel's
//! first-in, first-out queue) then builds:
//!
//! - three **workers** in one EL0 component, each counting forever in its
//!   own word and never blocking or yielding; the third runs on a small
//!   finite Time;
//! - the **dispatcher** component, bound to the root Scheduler with
//!   `Scheduler.BindDispatcher`, with the workers' records mapped for it.
//!
//! The dispatcher refuses a stale and a zero-limit dispatch, runs the workers
//! round-robin, each for a limit, until the finite one is exhausted, then
//! waits with nothing dispatched. Only the first-in, first-out Threads run
//! then: the boot Thread checks that the workers stopped, refills the
//! exhausted Time, and the budget arriving wakes the dispatcher, which runs
//! the worker again.
//!
//! Runs on the real Kickstart boot path; in-guest assertions and the QEMU
//! semihosting exit status are the result (`just test-dispatch`).

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    kickstart::{
        bootstrap::{bootstrap_nucleus, device_untypeds, retained_init_memory, spawn_idle},
        kickstart_init_el2,
    },
    libaddress::PhysAddr,
    libcomponent_loader::{
        builder::{Builder, ImageArchive, ImageTarget, verify_retained_image},
        component::ttbr,
        keys::{SlotCursor, boot_key},
        loader::verify_component,
        paging::image_table_count,
    },
    // `libkicktest` supplies the panic handler.
    libkicktest as _,
    libobject::{
        KeySlot, KeyTableKey, NotificationKey, ObjectType, RawKey, Rights, TimeKey, UntypedKey,
        address_space::AddressSpaceKey,
        scheduler::{SchedulerKey, ThreadStart, activation_reason},
        scheduler_record::{StopReason, ThreadRecord},
        time::INFINITE_BUDGET,
    },
    libqemu::semihosting as semi,
    nucleus::objects::KeyTable,
};

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

/// First boot `KeyTable` slot the builder carves fixtures into.
const FIRST_FIXTURE_SLOT: u32 = 64;

/// Guard of the `KeyTable` archiving the builder's image Frames.
const BUILDER_ARCHIVE_GUARD: u32 = 0x0A_4C01;

/// Guards of the worker and dispatcher components' `KeyTable`s.
const WORKER_GUARD: u32 = 0x05_7201;
const DISPATCHER_GUARD: u32 = 0x05_7202;
const ACTOR_GUARD: u32 = 0x05_7203;

/// Where the workers' counter page is mapped, in their `AddressSpace` only.
const COUNTER_VA: u64 = 0x2000_0000;

/// Where the dispatcher's page is mapped, in its `AddressSpace` only.
const PAGE_VA: u64 = 0x2100_0000;

/// Where the record Frame is mapped in the dispatcher's `AddressSpace`.
const RECORD_VA: u64 = 0x2200_0000;

/// Where the guarded stacks are carved.
const STACK_REGION: u64 = 0x3000_0000;

/// Pages of each stack.
const STACK_PAGES: u32 = 4;

/// Stride of the record table.
const RECORD_STRIDE: u32 = 192;

const WORKERS: usize = 3;

/// The worker on the finite Time.
const FINITE_WORKER: usize = 2;

/// Record slots of the workers.
const WORKER_SLOT: [u32; WORKERS] = [1, 2, 3];

/// Budget of the finite worker: it is exhausted in the dispatcher's third
/// round, while every dispatch is limited to [`LIMIT_NS`].
const FINITE_BUDGET_NS: u64 = 25_000_000;

/// Budget the boot Thread gives the exhausted worker.
const REFILL_NS: u64 = 30_000_000;

/// How long each dispatch may run.
const LIMIT_NS: u64 = 10_000_000;

/// Rounds of dispatching per worker.
const ROUNDS: u64 = 5;

/// Slots of the dispatcher's own table holding its keys; the dispatcher
/// finds them through the wire keys in its page.
const SCHEDULER_SLOT: u32 = 32;
const WORKER_THREAD_SLOT: u32 = 33;
const PHASE_ONE_SLOT: u32 = 40;
const PHASE_TWO_SLOT: u32 = 41;

/// Phase three: the dispatcher's keys to the actors' Threads (three
/// waiters, a kicker, a spare it retires, one never configured), to the two
/// Notifications it signals itself, and the one it signals at the end.
const ACTOR_THREAD_SLOT: u32 = 48;
const BURST_SLOT: u32 = 54;
const PHASE_THREE_SLOT: u32 = 56;

/// Phase four: the dispatcher's keys to the timed waiter's Thread and to the
/// `EventCount` it sleeps on. The timed waiter is the sixth Thread of the actor
/// component, with its own record slot and Notification.
const TIMED_THREAD_SLOT: u32 = 57;
const SLEEPER_SLOT: u32 = 58;
const TIMED_RECORD_SLOT: u32 = 9;
const TIMED_ROLE: u64 = 2;
const TIMED_ACTOR: usize = 5;

/// Threads of the actor component: waiters on Notifications 0, 1 and 2, a
/// kicker that signals Notification 0, and a spare that never runs.
const ACTORS: usize = 5;
/// The same, as a slot offset.
const ACTOR_COUNT: u32 = 5;
const KICKER: usize = 3;
const SPARE: usize = 4;

/// Record slots of the actors.
const ACTOR_SLOT: [u32; ACTORS] = [4, 5, 6, 7, 8];

/// Slots of the actor component's table holding its Notifications.
const ACTOR_NOTIFICATION_SLOT: u32 = 32;

/// The dispatcher page's words (see `dispatch-dispatcher`): parameters, then
/// the dispatcher's observations.
mod page {
    pub const RECORDS: usize = 0;
    pub const STRIDE: usize = 1;
    pub const SCHEDULER: usize = 2;
    pub const PHASE_ONE: usize = 3;
    pub const PHASE_TWO: usize = 4;
    pub const WORKER_KEYS: usize = 5;
    pub const WORKER_SLOTS: usize = 8;
    pub const ROUNDS: usize = 11;
    pub const LIMIT_NS: usize = 12;
    pub const REFILLED: usize = 13;
    pub const DISPATCHES: usize = 16;
    pub const REASONS: usize = 17;
    pub const STALE_REFUSED: usize = 18;
    pub const INVALID_REFUSED: usize = 19;
    pub const PER_WORKER: usize = 20;
    pub const WAKE_REASONS: usize = 23;
    pub const P3_THREADS: usize = 32;
    pub const P3_SLOTS: usize = 38;
    pub const P3_NOTIFICATIONS: usize = 43;
    pub const PHASE_THREE: usize = 45;
    pub const P3_WAKE_REASONS: usize = 48;
    pub const P3_BURST_REASONS: usize = 49;
    pub const P3_AFTER_REASONS: usize = 50;
    pub const P3_RETIRED_REFUSED: usize = 51;
    pub const P3_FOREIGN_REFUSED: usize = 52;
    pub const P3_BURST_RUNNABLE: usize = 53;
    pub const P4_TIMED_KEY: usize = 54;
    pub const P4_TIMED_SLOT: usize = 55;
    pub const P4_SLEEPER: usize = 56;
    pub const P4_SLEPT_NS: usize = 57;
    pub const P4_TIMER_REASONS: usize = 58;
    pub const P4_TIMER_ON_TIME: usize = 59;
    pub const P4_TIMEOUT_REASONS: usize = 60;
    pub const P4_TIMEOUT_NS: usize = 61;
}

/// A word of a page the kernel window maps.
struct Words(*mut u64);

impl Words {
    fn read(&self, index: usize) -> u64 {
        // SAFETY: the page is mapped in the kernel's window for the whole
        // test; only its component writes the observation words.
        unsafe { self.0.add(index).read_volatile() }
    }

    fn write(&self, index: usize, value: u64) {
        // SAFETY: as above; used only before the component starts.
        unsafe { self.0.add(index).write_volatile(value) }
    }
}

pub fn run() -> ! {
    semi::println!("dispatch-test: enabled MMU and dropped to EL1");
    let retained = retained_init_memory();
    let image_tables = image_table_count(&retained);
    let boot = bootstrap_nucleus();
    let (nucleus, keytable_addr, boot_as_id) = (boot.nucleus, boot.keytable_addr, boot.boot_as_id);
    let untyped = UntypedKey::from_key(boot.boot_untyped_key);
    let self_table = KeyTableKey::from_key(boot.self_table_key);
    let builder = Builder {
        untyped: &untyped,
        self_table: &self_table,
        boot_table_addr: keytable_addr,
        retained: &retained,
    };
    // Kickstart parks the device Untypeds in boot slots the fixtures must avoid.
    let devices = device_untypeds();
    let mut slots =
        SlotCursor::starting_at(FIRST_FIXTURE_SLOT).avoiding(devices.first_slot, devices.count);

    // The builder's own AddressSpace maps the boot image, so the boot Thread
    // and idle have a real root and ASID.
    let builder_as_key = boot_key(KeySlot::SELF_ADDRESS_SPACE.0, 1);
    let builder_l2 = builder.root_chain(builder_as_key, &mut slots);
    assert_eq!(Builder::assign_asid(builder_as_key), 1);
    builder.map_retained_image(
        &[ImageTarget {
            address_space: builder_as_key,
            l2: builder_l2,
            first_table_slot: slots.take(u32::try_from(image_tables).unwrap_or(u32::MAX)),
        }],
        &ImageArchive {
            slot: slots.take(1),
            guard: BUILDER_ARCHIVE_GUARD,
            grant_scratch: slots.take(1),
            copy_scratch: slots.take(1),
        },
    );
    verify_retained_image(&retained, &[ttbr(nucleus, boot_as_id)]);
    AddressSpaceKey::from_key(builder_as_key)
        .activate()
        .unwrap_or_else(|error| panic!("builder Activate failed: {:?}", error.code()));

    let mut retype = |kind: ObjectType| {
        untyped
            .retype(kind, 0, 0, 1, &self_table, slots.take(1), Rights::all())
            .unwrap_or_else(|error| panic!("{kind:?} Retype failed: {:?}", error.code()))
    };
    let phase_one_key = retype(ObjectType::NOTIFICATION);
    let phase_two_key = retype(ObjectType::NOTIFICATION);
    let worker_thread_keys: [RawKey; WORKERS] =
        core::array::from_fn(|_| retype(ObjectType::THREAD));
    let dispatcher_thread_key = retype(ObjectType::THREAD);
    let worker_time_keys: [RawKey; WORKERS] = core::array::from_fn(|_| retype(ObjectType::TIME));
    let dispatcher_time_key = retype(ObjectType::TIME);
    let actor_thread_keys: [RawKey; ACTORS] = core::array::from_fn(|_| retype(ObjectType::THREAD));
    let actor_time_keys: [RawKey; ACTORS] = core::array::from_fn(|_| retype(ObjectType::TIME));
    let never_configured_key = retype(ObjectType::THREAD);
    // Notifications 0 to 2 for the waiters, 3 for the timed waiter.
    let notification_keys: [RawKey; 4] = core::array::from_fn(|_| retype(ObjectType::NOTIFICATION));
    let timed_thread_key = retype(ObjectType::THREAD);
    let timed_time_key = retype(ObjectType::TIME);
    let sleeper_key = retype(ObjectType::EVENT_COUNT);
    let phase_three_key = retype(ObjectType::NOTIFICATION);
    let boot_time = TimeKey::from_key(boot.boot_time_key);
    for (worker, time_key) in worker_time_keys.iter().enumerate() {
        let budget_ns = if worker == FINITE_WORKER {
            FINITE_BUDGET_NS
        } else {
            INFINITE_BUDGET
        };
        boot_time
            .transfer(&TimeKey::from_key(*time_key), budget_ns)
            .unwrap_or_else(|error| panic!("Time.Transfer failed: {:?}", error.code()));
    }
    boot_time
        .transfer(&TimeKey::from_key(dispatcher_time_key), INFINITE_BUDGET)
        .unwrap_or_else(|error| panic!("Time.Transfer failed: {:?}", error.code()));
    for time_key in actor_time_keys.into_iter().chain([timed_time_key]) {
        boot_time
            .transfer(&TimeKey::from_key(time_key), INFINITE_BUDGET)
            .unwrap_or_else(|error| panic!("Time.Transfer failed: {:?}", error.code()));
    }

    // ── The workers: one component, one counter page, three stacks ───────
    let worker = builder.component(WORKER_GUARD, &mut slots);
    builder.load_component(
        &worker,
        &components::WORKER,
        WORKER_GUARD ^ 0x80_0000,
        &mut slots,
    );
    verify_component(ttbr(nucleus, worker.address_space), &components::WORKER);
    let mut worker_stacks = builder.stack_region(&worker, STACK_REGION, &mut slots);
    let worker_stack_tops: [u64; WORKERS] = core::array::from_fn(|_| {
        builder
            .user_stack(&mut worker_stacks, STACK_PAGES, &mut slots)
            .top
    });
    let counter_page = builder.private_pages(
        worker.address_space_key,
        worker.l2,
        COUNTER_VA,
        1,
        &mut slots,
    );
    let counters = PhysAddr::new(counter_page)
        .user_to_kernel()
        .as_mut_ptr::<u64>();
    let counter = |worker: usize| {
        // SAFETY: the counter page is mapped in the kernel's window for the
        // whole test; only the worker writes its word (one cache line each).
        unsafe { counters.add(worker * 8).read_volatile() }
    };

    // ── The dispatcher: its component, page and the shared record table ──
    let dispatcher = builder.component(DISPATCHER_GUARD, &mut slots);
    builder.load_component(
        &dispatcher,
        &components::DISPATCHER,
        DISPATCHER_GUARD ^ 0x80_0000,
        &mut slots,
    );
    verify_component(
        ttbr(nucleus, dispatcher.address_space),
        &components::DISPATCHER,
    );
    let mut dispatcher_stacks = builder.stack_region(&dispatcher, STACK_REGION, &mut slots);
    let dispatcher_stack = builder.user_stack(&mut dispatcher_stacks, STACK_PAGES, &mut slots);
    let dispatcher_page = builder.private_pages(
        dispatcher.address_space_key,
        dispatcher.l2,
        PAGE_VA,
        1,
        &mut slots,
    );
    // The record table: one Frame, mapped read-only in the dispatcher's
    // AddressSpace, which is where the scheduler reads its records.
    Builder::map_table(
        builder.carve_tables(slots.take(1), 1),
        dispatcher.l2,
        RECORD_VA,
    );
    let record_first = slots.take(1);
    let record_frames = builder.retype_frames(record_first, 1);
    Builder::map_frames(
        record_first,
        record_frames.incarnation(),
        1,
        dispatcher.address_space_key,
        RECORD_VA,
        Rights(Rights::READ),
    );
    let record_frame = boot_key(record_first, record_frames.incarnation());
    let records = PhysAddr::new(Builder::frame_paddr(record_frame))
        .user_to_kernel()
        .as_mut_ptr::<u8>();

    // ── The actors: one component, a page of words per Thread ───────────
    let actor = builder.component(ACTOR_GUARD, &mut slots);
    builder.load_component(
        &actor,
        &components::ACTOR,
        ACTOR_GUARD ^ 0x80_0000,
        &mut slots,
    );
    verify_component(ttbr(nucleus, actor.address_space), &components::ACTOR);
    let mut actor_stacks = builder.stack_region(&actor, STACK_REGION, &mut slots);
    let actor_stack_tops: [u64; ACTORS + 1] = core::array::from_fn(|_| {
        builder
            .user_stack(&mut actor_stacks, STACK_PAGES, &mut slots)
            .top
    });
    let actor_page =
        builder.private_pages(actor.address_space_key, actor.l2, COUNTER_VA, 1, &mut slots);
    let actor_words = Words(dispatcher_page_pointer(actor_page));
    for (index, key) in (0_u32..).zip(notification_keys) {
        // SAFETY: the actor's retained table; no invocation overlaps and its
        // Threads have not started.
        unsafe { &mut *KeyTable::at_carve(actor.table_addr) }
            .insert(
                KeySlot(ACTOR_NOTIFICATION_SLOT + index),
                builder.entry(key),
                actor.guard,
            )
            .unwrap_or_else(|failure| panic!("key grant failed: {:?}", failure.error.code()));
    }
    for actor_index in 0..ACTORS {
        // Waiters on Notification 0, 1 and 2; the kicker signals 0; the spare
        // names 0 and never runs.
        let notification = if actor_index < 3 {
            u32::try_from(actor_index).unwrap_or(0)
        } else {
            0
        };
        actor_words.write(actor_index * 8, u64::from(actor_index == KICKER));
        actor_words.write(
            actor_index * 8 + 1,
            actor
                .local_key(ACTOR_NOTIFICATION_SLOT + notification, 1)
                .to_wire(),
        );
    }

    // The timed waiter: the sixth Thread of the actor component, waiting on
    // Notification 3 with a timeout.
    actor_words.write(TIMED_ACTOR * 8, TIMED_ROLE);
    actor_words.write(
        TIMED_ACTOR * 8 + 1,
        actor.local_key(ACTOR_NOTIFICATION_SLOT + 3, 1).to_wire(),
    );

    // The dispatcher's keys: the Scheduler, the workers' Threads and the two
    // Notifications it signals, in slots of its own table.
    let root_scheduler = SchedulerKey::from_key(boot.root_scheduler_key);
    let grant = |slot: u32, key: RawKey| {
        // SAFETY: the dispatcher's retained table; no invocation overlaps and
        // its Thread has not started.
        unsafe { &mut *KeyTable::at_carve(dispatcher.table_addr) }
            .insert(KeySlot(slot), builder.entry(key), dispatcher.guard)
            .unwrap_or_else(|failure| panic!("key grant failed: {:?}", failure.error.code()));
        dispatcher.local_key(slot, 1).to_wire()
    };
    let words = Words(dispatcher_page_pointer(dispatcher_page));
    words.write(page::RECORDS, RECORD_VA);
    words.write(page::STRIDE, u64::from(RECORD_STRIDE));
    words.write(
        page::SCHEDULER,
        grant(SCHEDULER_SLOT, boot.root_scheduler_key),
    );
    words.write(page::PHASE_ONE, grant(PHASE_ONE_SLOT, phase_one_key));
    words.write(page::PHASE_TWO, grant(PHASE_TWO_SLOT, phase_two_key));
    for (index, thread_key) in (0_u32..).zip(worker_thread_keys) {
        words.write(
            page::WORKER_KEYS + index as usize,
            grant(WORKER_THREAD_SLOT + index, thread_key),
        );
        words.write(
            page::WORKER_SLOTS + index as usize,
            u64::from(WORKER_SLOT[index as usize]),
        );
    }
    words.write(page::ROUNDS, ROUNDS);
    words.write(page::LIMIT_NS, LIMIT_NS);
    words.write(page::REFILLED, FINITE_WORKER as u64);
    for (index, thread_key) in (0_u32..).zip(actor_thread_keys) {
        words.write(
            page::P3_THREADS + index as usize,
            grant(ACTOR_THREAD_SLOT + index, thread_key),
        );
        words.write(
            page::P3_SLOTS + index as usize,
            u64::from(ACTOR_SLOT[index as usize]),
        );
    }
    words.write(
        page::P3_THREADS + ACTORS,
        grant(ACTOR_THREAD_SLOT + ACTOR_COUNT, never_configured_key),
    );
    for (index, key) in (0_u32..).zip(&notification_keys[1..3]) {
        words.write(
            page::P3_NOTIFICATIONS + index as usize,
            grant(BURST_SLOT + index, *key),
        );
    }
    words.write(page::PHASE_THREE, grant(PHASE_THREE_SLOT, phase_three_key));
    words.write(
        page::P4_TIMED_KEY,
        grant(TIMED_THREAD_SLOT, timed_thread_key),
    );
    words.write(page::P4_TIMED_SLOT, u64::from(TIMED_RECORD_SLOT));
    words.write(page::P4_SLEEPER, grant(SLEEPER_SLOT, sleeper_key));

    // The scheduler shares the table, then takes over the workers' records.
    root_scheduler
        .share_region(record_frame, 0, RECORD_STRIDE)
        .unwrap_or_else(|error| panic!("ShareRegion failed: {:?}", error.code()));
    for (index, thread_key) in worker_thread_keys.iter().enumerate() {
        root_scheduler
            .configure_thread(
                *thread_key,
                worker.address_space_key,
                ThreadStart {
                    entry: components::WORKER.entry.expect("the worker has an entry"),
                    stack_pointer: worker_stack_tops[index],
                    argument: COUNTER_VA + index as u64 * 64,
                },
                worker_time_keys[index],
            )
            .unwrap_or_else(|error| panic!("worker ConfigureThread failed: {:?}", error.code()));
        root_scheduler
            .assign_record(*thread_key, WORKER_SLOT[index])
            .unwrap_or_else(|error| panic!("AssignRecord failed: {:?}", error.code()));
        // Started under the first-in, first-out queue until the dispatcher
        // binds, which takes the workers off it.
        root_scheduler
            .start_thread(*thread_key)
            .unwrap_or_else(|error| panic!("worker StartThread failed: {:?}", error.code()));
    }
    for (index, thread_key) in actor_thread_keys.iter().enumerate() {
        root_scheduler
            .configure_thread(
                *thread_key,
                actor.address_space_key,
                ThreadStart {
                    entry: components::ACTOR.entry.expect("the actor has an entry"),
                    stack_pointer: actor_stack_tops[index],
                    argument: COUNTER_VA + index as u64 * 64,
                },
                actor_time_keys[index],
            )
            .unwrap_or_else(|error| panic!("actor ConfigureThread failed: {:?}", error.code()));
        root_scheduler
            .assign_record(*thread_key, ACTOR_SLOT[index])
            .unwrap_or_else(|error| panic!("actor AssignRecord failed: {:?}", error.code()));
        root_scheduler
            .start_thread(*thread_key)
            .unwrap_or_else(|error| panic!("actor StartThread failed: {:?}", error.code()));
    }
    root_scheduler
        .configure_thread(
            timed_thread_key,
            actor.address_space_key,
            ThreadStart {
                entry: components::ACTOR.entry.expect("the actor has an entry"),
                stack_pointer: actor_stack_tops[ACTORS],
                argument: COUNTER_VA + TIMED_ACTOR as u64 * 64,
            },
            timed_time_key,
        )
        .unwrap_or_else(|error| panic!("timed ConfigureThread failed: {:?}", error.code()));
    root_scheduler
        .assign_record(timed_thread_key, TIMED_RECORD_SLOT)
        .unwrap_or_else(|error| panic!("timed AssignRecord failed: {:?}", error.code()));
    root_scheduler
        .start_thread(timed_thread_key)
        .unwrap_or_else(|error| panic!("timed StartThread failed: {:?}", error.code()));
    root_scheduler
        .configure_thread(
            dispatcher_thread_key,
            dispatcher.address_space_key,
            ThreadStart {
                entry: components::DISPATCHER
                    .entry
                    .expect("the dispatcher has an entry"),
                stack_pointer: dispatcher_stack.top,
                argument: PAGE_VA,
            },
            dispatcher_time_key,
        )
        .unwrap_or_else(|error| panic!("dispatcher ConfigureThread failed: {:?}", error.code()));

    let idle = spawn_idle(
        nucleus,
        keytable_addr,
        RawKey::from_wire(untyped.to_wire()),
        boot_as_id,
    );
    assert_eq!(nucleus.idle_thread, Some(idle));
    root_scheduler
        .bind_dispatcher(dispatcher_thread_key)
        .unwrap_or_else(|error| panic!("BindDispatcher failed: {:?}", error.code()));
    // A second dispatcher is refused.
    assert!(
        root_scheduler
            .bind_dispatcher(dispatcher_thread_key)
            .is_err(),
        "a second BindDispatcher succeeded"
    );
    semi::println!("dispatch-test: dispatcher bound; parking");

    // This Thread stays on the first-in, first-out queue. It runs only when
    // the dispatcher waits with nothing dispatched.
    let phase_one = NotificationKey::from_key(phase_one_key);
    let phase_two = NotificationKey::from_key(phase_two_key);
    assert_eq!(
        phase_one
            .wait(NotificationKey::WAIT_INFINITE)
            .unwrap_or_else(|error| panic!("phase one wait failed: {:?}", error.code())),
        1
    );

    // ── First phase over: the workers ran round-robin ────────────────────
    let runs_of = |worker: usize| words.read(page::PER_WORKER + worker);
    semi::println!(
        "dispatch-test: dispatches {} {} {}, reasons {:#x}",
        runs_of(0),
        runs_of(1),
        runs_of(2),
        words.read(page::REASONS)
    );
    assert_eq!(runs_of(0), ROUNDS, "round-robin skipped a worker");
    assert_eq!(runs_of(1), ROUNDS, "round-robin skipped a worker");
    assert!(
        (1..ROUNDS).contains(&runs_of(FINITE_WORKER)),
        "the finite worker was not exhausted part-way"
    );
    let reasons = words.read(page::REASONS);
    assert_ne!(reasons & activation_reason::LIMIT, 0, "no limit activation");
    assert_ne!(reasons & activation_reason::EXHAUSTED, 0, "no exhaustion");
    assert_eq!(
        words.read(page::DISPATCHES),
        runs_of(0) + runs_of(1) + runs_of(2),
        "the dispatch counts disagree"
    );
    assert_eq!(words.read(page::STALE_REFUSED), 1);
    assert_eq!(words.read(page::INVALID_REFUSED), 1);
    for worker in 0..WORKERS {
        assert!(counter(worker) > 0, "worker {worker} never ran");
    }
    // The exhausted worker waits for budget, as its record shows.
    // SAFETY: the record Frame is mapped in the kernel's window; the nucleus
    // writes records through atomics.
    let finite_record = unsafe {
        ThreadRecord::at(
            records
                .add(WORKER_SLOT[FINITE_WORKER] as usize * RECORD_STRIDE as usize)
                .cast_const(),
        )
    }
    .try_snapshot(1_000_000)
    .expect("a consistent record");
    assert!(finite_record.awaits_budget());
    assert_eq!(
        finite_record.stop_reason(),
        Some(StopReason::BudgetExhausted)
    );

    // The dispatcher waits with nothing dispatched, so no worker runs while
    // this Thread does.
    let frozen: [u64; WORKERS] = core::array::from_fn(counter);
    let finite_dispatches = runs_of(FINITE_WORKER);
    boot_time
        .transfer(
            &TimeKey::from_key(worker_time_keys[FINITE_WORKER]),
            REFILL_NS,
        )
        .unwrap_or_else(|error| panic!("refill Transfer failed: {:?}", error.code()));
    assert_eq!(
        phase_two
            .wait(NotificationKey::WAIT_INFINITE)
            .unwrap_or_else(|error| panic!("phase two wait failed: {:?}", error.code())),
        1
    );

    // ── Second phase over: the arriving budget woke the dispatcher ───────
    assert_ne!(
        words.read(page::WAKE_REASONS) & activation_reason::EVENT,
        0,
        "the budget did not activate the dispatcher"
    );
    assert_eq!(runs_of(FINITE_WORKER), finite_dispatches + 1);
    assert!(counter(FINITE_WORKER) > frozen[FINITE_WORKER]);
    for (worker, &frozen_count) in frozen.iter().enumerate().take(FINITE_WORKER) {
        assert_eq!(counter(worker), frozen_count, "an undispatched worker ran");
    }

    // ── Third phase over: wake, burst, retired and foreign dispatches ────
    let phase_three = NotificationKey::from_key(phase_three_key);
    assert_eq!(
        phase_three
            .wait(NotificationKey::WAIT_INFINITE)
            .unwrap_or_else(|error| panic!("phase three wait failed: {:?}", error.code())),
        1
    );
    assert_eq!(
        words.read(page::P3_WAKE_REASONS),
        activation_reason::EVENT,
        "the kicker's wakeup did not preempt it alone"
    );
    assert_eq!(words.read(page::P3_BURST_REASONS), activation_reason::EVENT);
    assert_eq!(words.read(page::P3_BURST_RUNNABLE), 2);
    assert_eq!(
        words.read(page::P3_AFTER_REASONS),
        activation_reason::BLOCKED
    );
    assert_eq!(words.read(page::P3_RETIRED_REFUSED), 1);
    assert_eq!(words.read(page::P3_FOREIGN_REFUSED), 1);
    // Each waiter woke exactly once and the retired spare never ran.
    let actor_count = |index: usize| actor_words.read(index * 8 + 2);
    for waiter in 0..3 {
        assert_eq!(
            actor_count(waiter),
            1,
            "waiter {waiter} woke the wrong number of times"
        );
    }
    // The kicker was stopped by the wakeup it caused, before or soon after it
    // began counting; the dispatcher already checked its record changed.
    assert_eq!(actor_count(SPARE), 0, "the retired spare ran");

    // ── Fourth phase: the clock, a timer, a timeout and a sleep ──────────
    // The armed timer activated the waiting dispatcher with TIMER, though
    // the mask asks only for runnable Threads, and not before its deadline.
    assert_eq!(
        words.read(page::P4_TIMER_REASONS) & activation_reason::TIMER,
        activation_reason::TIMER
    );
    assert_eq!(words.read(page::P4_TIMER_ON_TIME), 1);
    // The timed wait ended by its ten-millisecond timeout: the Thread became
    // runnable, which woke the idle-waiting dispatcher, and it counted one
    // timeout (and is waiting again).
    assert_eq!(
        words.read(page::P4_TIMEOUT_REASONS),
        activation_reason::EVENT
    );
    let timeout_ns = words.read(page::P4_TIMEOUT_NS);
    assert!(
        (10_000_000..1_000_000_000).contains(&timeout_ns),
        "the wait timed out after {timeout_ns} ns"
    );
    assert_eq!(actor_count(TIMED_ACTOR), 1);
    // The sleep lasted its fifteen milliseconds, no less.
    let slept_ns = words.read(page::P4_SLEPT_NS);
    assert!(
        (15_000_000..1_000_000_000).contains(&slept_ns),
        "the sleep took {slept_ns} ns"
    );

    semi::println!(
        "dispatch-test: ✅ bind, round-robin dispatch, wake preemption, burst coalescing, refusals, timers, timeouts and sleep passed"
    );
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            libcpu::endless_sleep()
        }
    }
}

/// The dispatcher's page, through the kernel's window.
fn dispatcher_page_pointer(paddr: u64) -> *mut u64 {
    PhysAddr::new(paddr).user_to_kernel().as_mut_ptr::<u64>()
}

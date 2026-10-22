#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! dispatch-test: a userspace root dispatcher end to end.
//!
//! Kickstart loads the privileged interrupt-controller component and the
//! nucleus arms the timer. The boot Thread (a Thread of the root Scheduler,
//! which the dispatcher runs whenever the test needs it to) then builds:
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
    aarch64_cpu::registers::{CNTFRQ_EL0, CNTV_CTL_EL0, CNTV_TVAL_EL0, Readable, Writeable},
    kickstart::{
        bootstrap::{
            bootstrap_nucleus, device_untypeds, interrupt_controllers, retained_init_memory,
        },
        kickstart_init_el2,
    },
    libaddress::PhysAddr,
    libcomponent_loader::{
        builder::{Builder, ImageArchive, ImageTarget, verify_retained_image},
        component::ttbr,
        keys::{BOOT_TABLE_GUARD, SlotCursor, boot_key},
        loader::verify_component,
        paging::image_table_count,
    },
    libdevicetree::interrupts::{TimerInterrupt, timer_interrupt},
    // `libkicktest` supplies the panic handler.
    libkicktest as _,
    libobject::{
        IrqControlKey, IrqHandlerKey, KeySlot, KeyTableKey, NotificationKey, ObjectType, RawKey,
        Rights, TimeKey, UntypedKey,
        address_space::AddressSpaceKey,
        scheduler::{SchedulerKey, ThreadStart, activation_reason},
        scheduler_record::{SchedulerRecord, StopReason, ThreadRecord},
        time::INFINITE_BUDGET,
    },
    libqemu::semihosting as semi,
    nucleus::{api::key_entry::KeyEntry, objects::KeyTable},
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

/// Phase five: the interrupt receiver is the seventh Thread of the actor
/// component, a waiter on the interrupt's Notification, with its own record
/// slot; the dispatcher's keys to it and to the three Notifications of the
/// handshake.
const RECEIVER_ACTOR: usize = 6;

/// Phase six: the exiter is the eighth Thread of the actor component. It
/// counts, then retires itself with its own Thread key (in its page); the
/// dispatcher holds a key to it too.
const EXITER_ACTOR: usize = 7;
const EXITER_ROLE: u64 = 3;
const EXITER_RECORD_SLOT: u32 = 11;
const EXITER_THREAD_SLOT: u32 = 64;
const EXITER_DONE_SLOT: u32 = 65;
const EXITER_OWN_SLOT: u32 = 37;

/// Phase seven: a child Scheduler with its own dispatcher component, record
/// table and one Thread (a worker, donated to it). The dispatcher holds keys
/// to the child dispatcher and the Thread.
const CHILD_GUARD: u32 = 0x05_7204;
const CHILD_DISPATCHER_RECORD_SLOT: u32 = 12;
const CHILD_THREAD_RECORD_SLOT: u32 = 1;
const CHILD_DISPATCHER_SLOT: u32 = 66;
const CHILD_THREAD_SLOT: u32 = 67;
const CHILD_DONE_SLOT: u32 = 68;
/// The boot Thread's key in the dispatcher's table.
const BOOT_THREAD_SLOT: u32 = 69;
/// Slots of the child dispatcher's own table: the child Scheduler and its Thread.
const CHILD_SCHEDULER_KEY_SLOT: u32 = 32;
const CHILD_THREAD_KEY_SLOT: u32 = 33;
/// The child Thread is a worker, counting in the fourth word group.
const CHILD_WORKER: usize = 3;
const RECEIVER_RECORD_SLOT: u32 = 10;
const RECEIVER_THREAD_SLOT: u32 = 60;
const READY_SLOT: u32 = 61;
const ARMED_SLOT: u32 = 62;
const DONE_SLOT: u32 = 63;

/// The badge the interrupt's Notification signals with.
const IRQ_BADGE: u64 = 1;

/// How far ahead the test arms the virtual timer, in microseconds.
const TIMER_DELAY_US: u64 = 25_000;

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
    pub const P5_RECEIVER_KEY: usize = 62;
    pub const P5_RECEIVER_SLOT: usize = 63;
    pub const P5_READY: usize = 64;
    pub const P5_ARMED: usize = 65;
    pub const P5_DONE: usize = 66;
    pub const P5_REASONS: usize = 67;
    pub const P5_ELAPSED_NS: usize = 68;
    pub const P5_AFTER_REASONS: usize = 69;
    pub const P6_EXITER_KEY: usize = 70;
    pub const P6_EXITER_SLOT: usize = 71;
    pub const P6_DONE: usize = 72;
    pub const P6_EXIT_REASONS: usize = 73;
    pub const P7_CHILD_KEY: usize = 80;
    pub const P7_CHILD_SLOT: usize = 81;
    pub const P7_THREAD_KEY: usize = 82;
    pub const P7_DONE: usize = 83;
    pub const BOOT_THREAD: usize = 89;
    pub const P7_FIRST_REASONS: usize = 84;
    pub const P7_FIRST_NS: usize = 85;
    pub const P7_SECOND_REASONS: usize = 86;
    pub const P7_SECOND_NS: usize = 87;
    pub const P7_FAILED_REASONS: usize = 88;
    /// The child dispatcher's page.
    pub const C_RECORDS: usize = 0;
    pub const C_STRIDE: usize = 1;
    pub const C_SCHEDULER: usize = 2;
    pub const C_THREAD: usize = 3;
    pub const C_THREAD_SLOT: usize = 4;
    pub const C_GRANT_ONE_END_NS: usize = 8;
    pub const C_FIRST_RESUME_REASONS: usize = 9;
    pub const C_GRANT_TWO_END_NS: usize = 10;
    pub const C_INNER_LIMITS: usize = 11;
    pub const C_INNER_REASONS: usize = 12;
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

    // The virtual timer is the interrupt source: the test arms it, the line is
    // issued from the device tree, and a receiver Thread waits for it.
    let device_tree = kickstart::device_tree::at_el1();
    let virtual_timer = timer_interrupt(device_tree.index(), TimerInterrupt::Virtual)
        .expect("the timer has a virtual interrupt");
    let timer_controller = interrupt_controllers()
        .index_of(virtual_timer.controller)
        .expect("the timer's controller is driven");
    let issued = IrqControlKey::from_key(boot.irq_control_key)
        .issue(
            &self_table,
            slots.take(1),
            timer_controller,
            virtual_timer.cells(),
        )
        .unwrap_or_else(|error| panic!("IRQControl.Issue failed: {:?}", error.code()));
    let irq_handler = IrqHandlerKey::from_key(issued.handler);

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
    let receiver_thread_key = retype(ObjectType::THREAD);
    let receiver_time_key = retype(ObjectType::TIME);
    let irq_notification_key = retype(ObjectType::NOTIFICATION);
    let exiter_thread_key = retype(ObjectType::THREAD);
    let exiter_time_key = retype(ObjectType::TIME);
    let exiter_done_key = retype(ObjectType::NOTIFICATION);
    let child_scheduler_key = retype(ObjectType::SCHEDULER);
    let child_thread_key = retype(ObjectType::THREAD);
    let child_thread_time_key = retype(ObjectType::TIME);
    let child_dispatcher_key = retype(ObjectType::THREAD);
    let child_dispatcher_time_key = retype(ObjectType::TIME);
    let child_done_key = retype(ObjectType::NOTIFICATION);
    let [ready_key, armed_key, done_key] =
        core::array::from_fn(|_| retype(ObjectType::NOTIFICATION));
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
    for time_key in actor_time_keys.into_iter().chain([
        timed_time_key,
        receiver_time_key,
        exiter_time_key,
        child_thread_time_key,
        child_dispatcher_time_key,
    ]) {
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
    let actor_stack_tops: [u64; ACTORS + 3] = core::array::from_fn(|_| {
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

    // The interrupt receiver: the seventh Thread of the actor component, a
    // waiter on the interrupt's Notification.
    // SAFETY: the actor's retained table; no invocation overlaps and its
    // Threads have not started.
    unsafe { &mut *KeyTable::at_carve(actor.table_addr) }
        .insert(
            KeySlot(ACTOR_NOTIFICATION_SLOT + 4),
            builder.entry(irq_notification_key),
            actor.guard,
        )
        .unwrap_or_else(|failure| panic!("key grant failed: {:?}", failure.error.code()));
    actor_words.write(RECEIVER_ACTOR * 8, 0);
    actor_words.write(
        RECEIVER_ACTOR * 8 + 1,
        actor.local_key(ACTOR_NOTIFICATION_SLOT + 4, 1).to_wire(),
    );

    // The exiter: the eighth Thread of the actor component. It names a
    // Notification it never uses, and holds its own Thread key to retire
    // itself with.
    let exiter_id = builder
        .entry(exiter_thread_key)
        .thread_id()
        .unwrap_or_else(|error| panic!("exiter Thread identity: {:?}", error.code()));
    actor_words.write(EXITER_ACTOR * 8, EXITER_ROLE);
    actor_words.write(
        EXITER_ACTOR * 8 + 1,
        actor.local_key(ACTOR_NOTIFICATION_SLOT, 1).to_wire(),
    );
    // SAFETY: the actor's retained table; no invocation overlaps and its
    // Threads have not started.
    unsafe { &mut *KeyTable::at_carve(actor.table_addr) }
        .insert(
            KeySlot(EXITER_OWN_SLOT),
            builder.entry(exiter_thread_key),
            actor.guard,
        )
        .unwrap_or_else(|failure| panic!("key grant failed: {:?}", failure.error.code()));
    actor_words.write(
        EXITER_ACTOR * 8 + 3,
        actor.local_key(EXITER_OWN_SLOT, 1).to_wire(),
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
    words.write(
        page::P5_RECEIVER_KEY,
        grant(RECEIVER_THREAD_SLOT, receiver_thread_key),
    );
    words.write(page::P5_RECEIVER_SLOT, u64::from(RECEIVER_RECORD_SLOT));
    words.write(page::P5_READY, grant(READY_SLOT, ready_key));
    words.write(page::P5_ARMED, grant(ARMED_SLOT, armed_key));
    words.write(page::P5_DONE, grant(DONE_SLOT, done_key));
    words.write(
        page::P6_EXITER_KEY,
        grant(EXITER_THREAD_SLOT, exiter_thread_key),
    );
    words.write(page::P6_EXITER_SLOT, u64::from(EXITER_RECORD_SLOT));
    words.write(page::P6_DONE, grant(EXITER_DONE_SLOT, exiter_done_key));

    // ── The child domain: a component, a record table and a Thread ───────
    let child = builder.component(CHILD_GUARD, &mut slots);
    builder.load_component(
        &child,
        &components::CHILD,
        CHILD_GUARD ^ 0x80_0000,
        &mut slots,
    );
    verify_component(ttbr(nucleus, child.address_space), &components::CHILD);
    let mut child_stacks = builder.stack_region(&child, STACK_REGION, &mut slots);
    let child_stack = builder.user_stack(&mut child_stacks, STACK_PAGES, &mut slots);
    let child_page =
        builder.private_pages(child.address_space_key, child.l2, PAGE_VA, 1, &mut slots);
    Builder::map_table(builder.carve_tables(slots.take(1), 1), child.l2, RECORD_VA);
    let child_record_first = slots.take(1);
    let child_record_frames = builder.retype_frames(child_record_first, 1);
    Builder::map_frames(
        child_record_first,
        child_record_frames.incarnation(),
        1,
        child.address_space_key,
        RECORD_VA,
        Rights(Rights::READ),
    );
    let child_record_frame = boot_key(child_record_first, child_record_frames.incarnation());
    let child_words = Words(dispatcher_page_pointer(child_page));
    let child_grant = |slot: u32, key: RawKey| {
        // SAFETY: the child dispatcher's retained table; no invocation overlaps
        // and its Thread has not started.
        unsafe { &mut *KeyTable::at_carve(child.table_addr) }
            .insert(KeySlot(slot), builder.entry(key), child.guard)
            .unwrap_or_else(|failure| panic!("key grant failed: {:?}", failure.error.code()));
        child.local_key(slot, 1).to_wire()
    };
    child_words.write(page::C_RECORDS, RECORD_VA);
    child_words.write(page::C_STRIDE, u64::from(RECORD_STRIDE));
    child_words.write(
        page::C_SCHEDULER,
        child_grant(CHILD_SCHEDULER_KEY_SLOT, child_scheduler_key),
    );
    child_words.write(
        page::C_THREAD,
        child_grant(CHILD_THREAD_KEY_SLOT, child_thread_key),
    );
    child_words.write(page::C_THREAD_SLOT, u64::from(CHILD_THREAD_RECORD_SLOT));
    words.write(
        page::P7_CHILD_KEY,
        grant(CHILD_DISPATCHER_SLOT, child_dispatcher_key),
    );
    words.write(page::P7_CHILD_SLOT, u64::from(CHILD_DISPATCHER_RECORD_SLOT));
    words.write(
        page::P7_THREAD_KEY,
        grant(CHILD_THREAD_SLOT, child_thread_key),
    );
    words.write(page::P7_DONE, grant(CHILD_DONE_SLOT, child_done_key));
    // The boot Thread (this test body) is a Thread of the root Scheduler,
    // which the dispatcher runs when the test needs it to.
    {
        let root = builder
            .entry(boot.root_scheduler_key)
            .carved_id()
            .unwrap_or_else(|error| panic!("root Scheduler identity: {:?}", error.code()));
        let boot_thread = nucleus.current_thread.expect("the boot Thread exists");
        nucleus
            .thread_mut(boot_thread)
            .expect("the boot Thread exists")
            .scheduler = Some(root);
        nucleus.join_scheduler(boot_thread, root);
        words.write(
            page::BOOT_THREAD,
            grant(BOOT_THREAD_SLOT, boot.boot_thread_key),
        );
    }
    let child_worker_stack = builder.user_stack(&mut worker_stacks, STACK_PAGES, &mut slots);

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
        // Started before the dispatcher binds: nothing runs it until then.
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
            receiver_thread_key,
            actor.address_space_key,
            ThreadStart {
                entry: components::ACTOR.entry.expect("the actor has an entry"),
                stack_pointer: actor_stack_tops[ACTORS + 1],
                argument: COUNTER_VA + RECEIVER_ACTOR as u64 * 64,
            },
            receiver_time_key,
        )
        .unwrap_or_else(|error| panic!("receiver ConfigureThread failed: {:?}", error.code()));
    root_scheduler
        .assign_record(receiver_thread_key, RECEIVER_RECORD_SLOT)
        .unwrap_or_else(|error| panic!("receiver AssignRecord failed: {:?}", error.code()));
    root_scheduler
        .start_thread(receiver_thread_key)
        .unwrap_or_else(|error| panic!("receiver StartThread failed: {:?}", error.code()));
    root_scheduler
        .configure_thread(
            exiter_thread_key,
            actor.address_space_key,
            ThreadStart {
                entry: components::ACTOR.entry.expect("the actor has an entry"),
                stack_pointer: actor_stack_tops[ACTORS + 2],
                argument: COUNTER_VA + EXITER_ACTOR as u64 * 64,
            },
            exiter_time_key,
        )
        .unwrap_or_else(|error| panic!("exiter ConfigureThread failed: {:?}", error.code()));
    root_scheduler
        .assign_record(exiter_thread_key, EXITER_RECORD_SLOT)
        .unwrap_or_else(|error| panic!("exiter AssignRecord failed: {:?}", error.code()));
    root_scheduler
        .start_thread(exiter_thread_key)
        .unwrap_or_else(|error| panic!("exiter StartThread failed: {:?}", error.code()));
    // The line signals the Notification, with a badge, for the receiver, and
    // is then enabled.
    let irq_id = builder
        .entry(irq_notification_key)
        .carved_id()
        .unwrap_or_else(|error| panic!("Notification identity: {:?}", error.code()));
    // SAFETY: the live boot table, borrowed for this bootstrap grant only.
    let badged_irq = unsafe { &mut *KeyTable::at_carve(keytable_addr) }
        .insert(
            KeySlot(slots.take(1)),
            KeyEntry::new_carved(irq_id, Rights::all(), IRQ_BADGE),
            BOOT_TABLE_GUARD,
        )
        .unwrap_or_else(|failure| panic!("boot install failed: {:?}", failure.error.code()));
    irq_handler
        .set_notification(badged_irq, receiver_thread_key)
        .unwrap_or_else(|error| panic!("SetNotification failed: {:?}", error.code()));
    irq_handler
        .ack()
        .unwrap_or_else(|error| panic!("Ack failed: {:?}", error.code()));
    // The child Scheduler: adopted by the root, with its own record table; its
    // Thread is a worker the root configures and starts, then donates; its
    // dispatcher is a Thread of the root, bound to the child.
    let child_scheduler = SchedulerKey::from_key(child_scheduler_key);
    root_scheduler
        .adopt(child_scheduler_key)
        .unwrap_or_else(|error| panic!("Adopt failed: {:?}", error.code()));
    child_scheduler
        .share_region(child_record_frame, 0, RECORD_STRIDE)
        .unwrap_or_else(|error| panic!("child ShareRegion failed: {:?}", error.code()));
    root_scheduler
        .configure_thread(
            child_thread_key,
            worker.address_space_key,
            ThreadStart {
                entry: components::WORKER.entry.expect("the worker has an entry"),
                stack_pointer: child_worker_stack.top,
                argument: COUNTER_VA + CHILD_WORKER as u64 * 64,
            },
            child_thread_time_key,
        )
        .unwrap_or_else(|error| panic!("child Thread ConfigureThread failed: {:?}", error.code()));
    root_scheduler
        .start_thread(child_thread_key)
        .unwrap_or_else(|error| panic!("child Thread StartThread failed: {:?}", error.code()));
    root_scheduler
        .move_thread(
            child_thread_key,
            child_scheduler_key,
            CHILD_THREAD_RECORD_SLOT,
        )
        .unwrap_or_else(|error| panic!("MoveThread failed: {:?}", error.code()));
    root_scheduler
        .configure_thread(
            child_dispatcher_key,
            child.address_space_key,
            ThreadStart {
                entry: components::CHILD.entry.expect("the child has an entry"),
                stack_pointer: child_stack.top,
                argument: PAGE_VA,
            },
            child_dispatcher_time_key,
        )
        .unwrap_or_else(|error| {
            panic!(
                "child dispatcher ConfigureThread failed: {:?}",
                error.code()
            )
        });
    root_scheduler
        .assign_record(child_dispatcher_key, CHILD_DISPATCHER_RECORD_SLOT)
        .unwrap_or_else(|error| panic!("child dispatcher AssignRecord failed: {:?}", error.code()));
    child_scheduler
        .bind_dispatcher(child_dispatcher_key)
        .unwrap_or_else(|error| panic!("child BindDispatcher failed: {:?}", error.code()));
    root_scheduler
        .start_thread(child_dispatcher_key)
        .unwrap_or_else(|error| panic!("child dispatcher StartThread failed: {:?}", error.code()));
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
            .await_bits(NotificationKey::WAIT_INFINITE)
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
            .await_bits(NotificationKey::WAIT_INFINITE)
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
            .await_bits(NotificationKey::WAIT_INFINITE)
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

    // ── Fifth phase: an interrupt wakes a receiver while a spinner runs ──
    // The dispatcher is ready: arm the virtual timer, a few milliseconds
    // away, and let it go on to dispatch the spinner.
    NotificationKey::from_key(ready_key)
        .await_bits(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("ready wait failed: {:?}", error.code()));
    let ticks = CNTFRQ_EL0.get() * TIMER_DELAY_US / 1_000_000;
    CNTV_TVAL_EL0.set(ticks.max(1));
    CNTV_CTL_EL0.write(CNTV_CTL_EL0::ENABLE::SET + CNTV_CTL_EL0::IMASK::CLEAR);
    NotificationKey::from_key(armed_key)
        .signal(1)
        .unwrap_or_else(|error| panic!("armed signal failed: {:?}", error.code()));
    NotificationKey::from_key(done_key)
        .await_bits(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("done wait failed: {:?}", error.code()));
    CNTV_CTL_EL0.write(CNTV_CTL_EL0::ENABLE::CLEAR);
    // The wakeup alone ended the spinner's two-second dispatch, a few
    // milliseconds after the interrupt was armed: the dispatcher decided.
    assert_eq!(words.read(page::P5_REASONS), activation_reason::EVENT);
    let preempted_after_ns = words.read(page::P5_ELAPSED_NS);
    assert!(
        (TIMER_DELAY_US * 1_000 / 2..1_000_000_000).contains(&preempted_after_ns),
        "the spinner ran {preempted_after_ns} ns before the interrupt woke the receiver"
    );
    // The receiver handled it once and waits again.
    assert_eq!(
        words.read(page::P5_AFTER_REASONS),
        activation_reason::BLOCKED
    );
    assert_eq!(actor_count(RECEIVER_ACTOR), 1);
    // The interrupt was counted, with the time it took, on the line and in
    // the Scheduler record (not charged to the spinner).
    let line = irq_handler
        .query()
        .unwrap_or_else(|error| panic!("IRQHandler.Query failed: {:?}", error.code()));
    assert!(line.handled >= 1 && line.nanoseconds > 0);
    // SAFETY: slot 0 of the shared record Frame, mapped in the kernel's window.
    let scheduler_record = unsafe { SchedulerRecord::at(records.cast_const()) }
        .try_snapshot(1_000_000)
        .expect("a consistent scheduler record");
    assert!(scheduler_record.interrupt_ns > 0);
    assert!(scheduler_record.interrupt_ns >= line.nanoseconds || line.handled > 1);
    // The nucleus idled while every Thread waited: that time is metered
    // apart from the interrupts.
    assert!(scheduler_record.idle_ns > 0);
    // ── Sixth phase: Suspend, Resume and a Thread retiring itself ────────
    NotificationKey::from_key(exiter_done_key)
        .await_bits(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("phase six wait failed: {:?}", error.code()));
    assert_eq!(words.read(page::P6_EXIT_REASONS), activation_reason::EXITED);
    // The suspended waiter took the wakeup it missed when it was resumed,
    // the one resumed while its wait was still open took nothing, and the
    // exiter ran once.
    assert_eq!(actor_count(1), 2);
    assert_eq!(actor_count(2), 1);
    assert_eq!(actor_count(EXITER_ACTOR), 1);
    // The exiter's key no longer names a Thread.
    assert!(
        nucleus.thread(exiter_id).is_none(),
        "the exiter is still a Thread"
    );
    // ── Seventh phase: a child domain ─────────────────────────────────────
    NotificationKey::from_key(child_done_key)
        .await_bits(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("phase seven wait failed: {:?}", error.code()));
    // The first grant (60 ms) stopped the child's ten-second dispatch and
    // activated the parent, not the child.
    assert_eq!(words.read(page::P7_FIRST_REASONS), activation_reason::LIMIT);
    let first_ns = words.read(page::P7_FIRST_NS);
    assert!(
        (50_000_000..1_000_000_000).contains(&first_ns),
        "the first grant lasted {first_ns} ns"
    );
    // The child saw both grants end later than the one before, its parked
    // Dispatch returned LIMIT when the parent dispatched it again, and its own
    // short dispatches ended at their own limit inside the second grant.
    assert_ne!(child_words.read(page::C_GRANT_ONE_END_NS), 0);
    assert_eq!(
        child_words.read(page::C_FIRST_RESUME_REASONS),
        activation_reason::LIMIT
    );
    assert!(
        child_words.read(page::C_GRANT_TWO_END_NS) > child_words.read(page::C_GRANT_ONE_END_NS)
    );
    assert_eq!(child_words.read(page::C_INNER_LIMITS), 3);
    assert_eq!(
        child_words.read(page::C_INNER_REASONS),
        activation_reason::LIMIT
    );
    // With nothing left to run, the child handed the CPU back: the parent's
    // second dispatch ended as a block, well inside the second grant.
    assert_eq!(
        words.read(page::P7_SECOND_REASONS),
        activation_reason::BLOCKED
    );
    assert!(words.read(page::P7_SECOND_NS) < 1_000_000_000);
    // Retiring the child dispatcher failed the domain.
    assert_eq!(
        words.read(page::P7_FAILED_REASONS),
        activation_reason::SCHED_DOMAIN_FAILED
    );
    assert!(counter(CHILD_WORKER) > 0, "the child's Thread never ran");
    semi::println!(
        "dispatch-test: ✅ bind, round-robin dispatch, wake preemption, burst coalescing, refusals, timers, timeouts, sleep, interrupts, suspend, self-retire and child domains passed"
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

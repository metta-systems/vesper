//! The synchronization suite: `Notification` and `EventCount` state through the
//! real SVC path, then blocking end to end — the boot Thread parks on a
//! `Notification` and on `EventCount` Awaits while the Bounce fixture Thread
//! (its own `AddressSpace`, root and ASID; see `libkicktest::bounce`) signals
//! and advances, with every switch observing the selected translation
//! context — and finally `Thread.Retire` of the parked Bounce Thread.

use {
    core::sync::atomic::{AtomicU64, Ordering},
    kickstart::bootstrap::{
        BOOT_TABLE_GUARD, BootState, PoolCapacities, bootstrap_nucleus, retained_init_memory,
    },
    libexception::arch::aarch64::ExceptionOrigin,
    libkicktest::{
        bounce::{
            self, TEST_TABLE_GUARD, assert_source_selected, fixture_stack_state, fixture_trap_sp,
            record_fixture_stacks,
        },
        builder::Builder,
        keys::{boot_key, boot_slot},
        paging::image_table_count,
        threads, translation,
    },
    libobject::{
        CapError, EventCountKey, InconsistencyReason, KeySlot, KeyTableKey, NotificationKey,
        ObjectType, RawKey, Rights, UntypedKey, domain::DomainId, thread::ThreadKey,
    },
    libqemu::semihosting as semi,
    nucleus::{
        api::key_entry::KeyEntry,
        objects::{
            ExecutionContext, KeyTable, Thread, arch_objects::AddressSpaceObject,
            completion::PendingState,
        },
    },
};

pub fn run() {
    let (boot_execution_sp, shared_trap_sp) = record_fixture_stacks();
    let retained_init = retained_init_memory();
    translation::observe_bootstrap(crate::run as *const u8 as u64);
    // The boot and Bounce Threads and AddressSpaces; two plain Notifications
    // plus N1/N2; one EventCount; the source and Bounce root prefixes, both
    // image closures and both probe tables.
    let boot = bootstrap_nucleus(&PoolCapacities {
        threads: 2,
        address_spaces: 2,
        notifications: 4,
        event_counts: 1,
        page_tables: 8 + 2 * image_table_count(&retained_init),
        asid_pools: 1,
    });
    let BootState {
        nucleus,
        keytable_addr,
        boot_as_id,
        self_table_key,
        boot_untyped_key,
        debug_console_key,
    } = boot;
    let boot_table_binding = nucleus
        .pools
        .arch
        .address_spaces
        .get_live(usize::from(boot_as_id.index))
        .expect("boot AddressSpace missing")
        .keytable();
    let untyped = UntypedKey::from_key(boot_untyped_key);
    let self_table = KeyTableKey::from_key(self_table_key);
    let builder = Builder {
        untyped: &untyped,
        self_table: &self_table,
        boot_table_addr: keytable_addr,
        retained: &retained_init,
    };

    // ─────────────────────────────────────────────────────────────────
    // Notification: Retype-creatable synchronization state (2026-09-16)
    // ─────────────────────────────────────────────────────────────────

    // Retype two Notifications: pure kernel state allocated from the
    // bootstrap-carved pool — no Untyped bytes are carved.
    let notification_key = untyped
        .retype(
            ObjectType::NOTIFICATION,
            0,
            0,
            2,
            &self_table,
            KeySlot(16).0,
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("notification Retype failed: {:?}", error.code()));
    assert_eq!(notification_key.slot(), boot_slot(16));

    // A nonzero size_bits is rejected before any pool slot is taken.
    assert!(matches!(
        untyped.retype(
            ObjectType::NOTIFICATION,
            12,
            0,
            1,
            &self_table,
            KeySlot(18).0,
            Rights::all(),
        ),
        Err(CapError::InvalidSize(12))
    ));

    let notification = NotificationKey::from_key(notification_key);

    // Signal coalesces distinct bits into the bitmap; Poll consumes all
    // the pending bits at once.
    notification
        .signal(0b0110)
        .unwrap_or_else(|error| panic!("Notification.Signal failed: {:?}", error.code()));
    notification
        .signal(0b0001)
        .unwrap_or_else(|error| panic!("second Notification.Signal failed: {:?}", error.code()));
    assert_eq!(
        notification
            .poll()
            .unwrap_or_else(|error| panic!("Notification.Poll failed: {:?}", error.code())),
        0b0111
    );
    // Nothing pending: Poll returns zero, never blocking.
    assert_eq!(
        notification
            .poll()
            .unwrap_or_else(|error| panic!("second Notification.Poll failed: {:?}", error.code())),
        0
    );

    // Wait tests follow complete source/Bounce provisioning below, so even
    // the already-satisfied/rejected waits enter from the bound source root.

    // N1: the notification the boot domain blocks on. N2: the one
    // Bounce parks on between its rounds. EC: the event count whose
    // awaits the boot domain blocks on and Bounce advances. All carved
    // through the public Retype path.
    let n1_key = untyped
        .retype(
            ObjectType::NOTIFICATION,
            0,
            0,
            1,
            &self_table,
            KeySlot(18).0,
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("N1 Retype failed: {:?}", error.code()));
    let n2_key = untyped
        .retype(
            ObjectType::NOTIFICATION,
            0,
            0,
            1,
            &self_table,
            KeySlot(19).0,
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("N2 Retype failed: {:?}", error.code()));
    let ec_key = untyped
        .retype(
            ObjectType::EVENT_COUNT,
            0,
            0,
            1,
            &self_table,
            KeySlot(37).0,
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("EC Retype failed: {:?}", error.code()));

    // ── The Bounce fixture: its own AddressSpace, root and ASID ───────────
    let source = bounce::provision_source(&builder);
    let fixture = bounce::provision(
        &builder,
        nucleus,
        &source,
        boot_table_binding,
        boot_as_id,
        debug_console_key,
        shared_trap_sp,
    );
    let bounce_as_id = fixture.as_id;
    let bounce_table_addr = fixture.table_addr;
    let bounce_table_binding = fixture.table_binding;
    let (bounce_stack_bottom, bounce_stack_top) = (fixture.stack.bottom, fixture.stack.top);
    let source_root = fixture.source_root;
    let bound_asid = source.asid;

    // Bootstrap grant: install Bounce's notification keys (the boot
    // test's authority delegated kernel-privately by the bootstrap
    // builder, like the boot console grant).
    {
        // SAFETY: the boot table is the live carved boot KeyTable.
        let boot_table_ref = unsafe { &*(keytable_addr as *const KeyTable) };
        let n1_id = boot_table_ref
            .lookup(n1_key, BOOT_TABLE_GUARD)
            .and_then(KeyEntry::object_id)
            .unwrap_or_else(|_| panic!("N1 entry missing or not a pool identity"));
        let n2_id = boot_table_ref
            .lookup(n2_key, BOOT_TABLE_GUARD)
            .and_then(KeyEntry::object_id)
            .unwrap_or_else(|_| panic!("N2 entry missing or not a pool identity"));
        let ec_id = boot_table_ref
            .lookup(ec_key, BOOT_TABLE_GUARD)
            .and_then(KeyEntry::object_id)
            .unwrap_or_else(|_| panic!("EC entry missing or not a pool identity"));
        // SAFETY: Bounce's table is the freshly carved, live KeyTable.
        let bounce_table = unsafe { &mut *(bounce_table_addr as *mut KeyTable) };
        let bounce_n1 = bounce_table
            .insert(
                KeySlot(5),
                KeyEntry::new::<nucleus::objects::Notification>(n1_id, Rights::all(), 0),
                TEST_TABLE_GUARD,
            )
            .unwrap_or_else(|failure| panic!("Bounce N1 grant failed: {:?}", failure.error.code()));
        let bounce_n2 = bounce_table
            .insert(
                KeySlot(6),
                KeyEntry::new::<nucleus::objects::Notification>(n2_id, Rights::all(), 0),
                TEST_TABLE_GUARD,
            )
            .unwrap_or_else(|failure| panic!("Bounce N2 grant failed: {:?}", failure.error.code()));
        // The EventCount sits at slot 7: slots 1–4 are the well-known
        // layout (return key, self AddressSpace, parent Thread, self
        // KeyTable) and must not be disturbed by fixture grants.
        let bounce_ec = bounce_table
            .insert(
                KeySlot(7),
                KeyEntry::new::<nucleus::objects::EventCount>(ec_id, Rights::all(), 0),
                TEST_TABLE_GUARD,
            )
            .unwrap_or_else(|failure| panic!("Bounce EC grant failed: {:?}", failure.error.code()));
        // Hand over the actual recipient-local keys, including returned
        // incarnations; slot conventions alone cannot mint authority.
        BOUNCE_N1_KEY.store(bounce_n1.to_wire(), Ordering::Release);
        BOUNCE_N2_KEY.store(bounce_n2.to_wire(), Ordering::Release);
        BOUNCE_EC_KEY.store(bounce_ec.to_wire(), Ordering::Release);
    }
    // An already-satisfied Wait consumes and returns the bits
    // immediately through the real SVC path.
    notification
        .signal(0b1)
        .unwrap_or_else(|error| panic!("third Notification.Signal failed: {:?}", error.code()));
    assert_eq!(
        notification
            .wait(NotificationKey::WAIT_INFINITE)
            .unwrap_or_else(|error| panic!(
                "satisfied Notification.Wait failed: {:?}",
                error.code()
            )),
        0b1
    );

    // A wait that would block now blocks for real (completion foundation,
    // 2026-09-16): the end-to-end proof below parks the boot domain and
    // resumes it through the Bounce fixture domain. A finite timeout is
    // still rejected: the time subsystem does not exist yet.
    assert!(matches!(
        notification.wait(1_000_000),
        Err(CapError::InvalidOperation)
    ));
    assert_source_selected(nucleus, boot_as_id, source_root, bound_asid);
    let bounce_id = threads::spawn(
        nucleus,
        bounce_as_id,
        bounce_entry as *const () as u64,
        bounce_stack_top,
    );
    assert_eq!(bounce_id.index, 1);

    // The boot thread blocks on N1: this SVC does not return — the
    // kernel parks it, starts Bounce (which signals N1 and parks on N2),
    // then resumes the boot thread with the delivered bitmap.
    let received = translation::notification_wait(n1_key)
        .unwrap_or_else(|error| panic!("blocking Notification.Wait failed: {:?}", error.code()));
    assert_eq!(received, BOUNCE_MAGIC_BITS);
    assert_source_selected(nucleus, boot_as_id, source_root, bound_asid);
    translation::observe_source();
    assert_eq!(fixture_stack_state(), (boot_execution_sp, shared_trap_sp));
    // Bounce is parked on N2; the boot thread resumed with the bits.
    let first_bounce_parked = {
        let bounce = nucleus
            .pools
            .threads
            .get_live(usize::from(bounce_id.index))
            .unwrap_or_else(|| panic!("Bounce Domain missing"));
        assert_bounce_parked_context(bounce, bounce_stack_bottom, bounce_stack_top);
        assert_eq!(bounce.address_space, bounce_as_id);
        assert_eq!(
            nucleus
                .pools
                .arch
                .address_spaces
                .get_live(usize::from(bounce.address_space.index))
                .expect("Bounce AddressSpace missing")
                .keytable()
                .address(),
            bounce_table_binding.address()
        );
        bounce.context
    };

    // ─────────────────────────────────────────────────────────────────
    // EventCount end-to-end (2026-09-18): monotonic counting, the
    // selected overflow policy, and blocking Await through the Bounce
    // fixture (broadcast wakeups, error wakeups)
    // ─────────────────────────────────────────────────────────────────

    let event_count = EventCountKey::from_key(ec_key);

    // A fresh counter reads zero.
    assert_eq!(
        event_count
            .read()
            .unwrap_or_else(|error| panic!("EventCount.Read failed: {:?}", error.code())),
        0
    );
    // Every advance is counted and returns the new value.
    assert_eq!(
        event_count
            .advance(7)
            .unwrap_or_else(|error| panic!("EventCount.Advance failed: {:?}", error.code())),
        7
    );
    assert_eq!(
        event_count
            .read()
            .unwrap_or_else(|error| panic!("second EventCount.Read failed: {:?}", error.code())),
        7
    );
    // Zero is invalid: an advance must strictly increase (selected
    // 2026-09-18).
    assert!(matches!(
        event_count.advance(0),
        Err(CapError::InvalidOperation)
    ));
    // Overflow rejects, leaves the counter unchanged, and carries the
    // shared status (selected 2026-09-18). No waiter is queued here, so
    // only the advancer observes the error.
    assert!(matches!(
        event_count.advance(u64::MAX),
        Err(CapError::CounterOverflow)
    ));
    assert_eq!(
        event_count
            .read()
            .unwrap_or_else(|error| panic!("third EventCount.Read failed: {:?}", error.code())),
        7
    );
    // An already-satisfied await returns the current value without
    // blocking; awaiting does not consume the counter.
    assert_eq!(
        event_count
            .await_ge(7, EventCountKey::WAIT_INFINITE)
            .unwrap_or_else(|error| panic!(
                "satisfied EventCount.Await failed: {:?}",
                error.code()
            )),
        7
    );
    // A finite timeout is still rejected: the time subsystem does not
    // exist yet.
    assert!(matches!(
        event_count.await_ge(100, 1_000_000),
        Err(CapError::InvalidOperation)
    ));

    // The preceding nonblocking SVCs reused the shared trap stack while
    // Bounce stayed parked. Its entire saved state and wait identity must
    // remain unchanged, not merely its completion-result registers.
    assert_eq!(
        nucleus
            .pools
            .threads
            .get_live(usize::from(bounce_id.index))
            .expect("parked Bounce missing")
            .context,
        first_bounce_parked
    );
    assert_eq!(fixture_stack_state(), (boot_execution_sp, shared_trap_sp));

    // Blocking Await end-to-end: request Bounce's +3 advance by
    // signaling N2 (bit 0), then block on target 10. Bounce advances the
    // counter, this domain's record completes with the new value, and
    // Bounce parks again.
    NotificationKey::from_key(n2_key)
        .signal(0b1)
        .unwrap_or_else(|error| panic!("N2 trigger signal failed: {:?}", error.code()));
    assert_eq!(
        translation::event_count_await(ec_key, 10).unwrap_or_else(|error| {
            panic!("blocking EventCount.Await failed: {:?}", error.code())
        }),
        10
    );
    assert_source_selected(nucleus, boot_as_id, source_root, bound_asid);
    translation::observe_source();
    assert_eq!(
        event_count
            .read()
            .unwrap_or_else(|error| panic!("fourth EventCount.Read failed: {:?}", error.code())),
        10
    );

    assert_eq!(fixture_stack_state(), (boot_execution_sp, shared_trap_sp));
    assert_bounce_parked_context(
        nucleus
            .pools
            .threads
            .get_live(usize::from(bounce_id.index))
            .expect("Bounce missing after successful Await"),
        bounce_stack_bottom,
        bounce_stack_top,
    );

    // Overflow wakes blocked waiters with the shared error (selected
    // 2026-09-18): request Bounce's overflowing advance (bit 1), then
    // block on an unreachable target. The advance completes the await
    // with `CounterOverflow`, the counter stays unchanged, and Bounce
    // parks for good.
    NotificationKey::from_key(n2_key)
        .signal(0b10)
        .unwrap_or_else(|error| panic!("N2 overflow trigger failed: {:?}", error.code()));
    assert!(matches!(
        translation::event_count_await(ec_key, u64::MAX - 2),
        Err(CapError::CounterOverflow)
    ));
    assert_eq!(
        event_count
            .read()
            .unwrap_or_else(|error| panic!("fifth EventCount.Read failed: {:?}", error.code())),
        10
    );
    assert_source_selected(nucleus, boot_as_id, source_root, bound_asid);
    translation::observe_source();
    translation::assert_rounds();
    assert_eq!(fixture_stack_state(), (boot_execution_sp, shared_trap_sp));
    // Bounce is parked on N2 for good; the boot domain resumed with the
    // error completion.
    {
        let bounce = nucleus
            .pools
            .threads
            .get_live(usize::from(bounce_id.index))
            .unwrap_or_else(|| panic!("Bounce Domain missing"));
        assert_bounce_parked_context(bounce, bounce_stack_bottom, bounce_stack_top);
    }

    // ─────────────────────────────────────────────────────────────────
    // Thread.Retire end-to-end: the Thread-control teardown trigger —
    // cancel every pending record naming the target as waiter, purge its
    // queued wakeup, reclaim its pool slot — through the real SVC path
    // under `RETIRE` authority.
    // ─────────────────────────────────────────────────────────────────
    {
        // Bounce is parked on N2 with a Waiting record: the canonical
        // teardown state of a blocked Thread.
        let ExecutionContext::Parked { saved, record } = nucleus
            .pools
            .threads
            .get_live(usize::from(bounce_id.index))
            .unwrap_or_else(|| panic!("Bounce Domain missing"))
            .context
        else {
            panic!("Bounce is not parked")
        };
        assert_eq!(
            nucleus.pending.state(record).ok(),
            Some(PendingState::Waiting)
        );
        assert_eq!(nucleus.pending.waiter(record).ok(), Some(bounce_id));

        // Bootstrap grants: Bounce's Thread capability in the boot table,
        // kernel-privately (like the boot console grant) — one with full
        // rights and one without `RETIRE`, so the authority check is
        // observable through the real SVC path. Thread is not on the
        // CopyDerive allowlist, so no public path could build these.
        let (bounce_thread_key, unprivileged_bounce_key) = {
            // SAFETY: keytable_addr names the live carved boot KeyTable.
            let boot_table = unsafe { &mut *(keytable_addr as *mut KeyTable) };
            let full = boot_table
                .insert(
                    KeySlot(38),
                    KeyEntry::new::<Thread>(bounce_id, Rights::all(), 0),
                    BOOT_TABLE_GUARD,
                )
                .unwrap_or_else(|failure| {
                    panic!("Bounce Thread grant failed: {:?}", failure.error.code())
                });
            let limited = boot_table
                .insert(
                    KeySlot(39),
                    KeyEntry::new::<Thread>(bounce_id, Rights(Rights::READ), 0),
                    BOOT_TABLE_GUARD,
                )
                .unwrap_or_else(|failure| {
                    panic!(
                        "limited Bounce Thread grant failed: {:?}",
                        failure.error.code()
                    )
                });
            (full, limited)
        };

        // Authority is explicit: a capability without `RETIRE` is
        // rejected before any teardown effect — Bounce stays parked.
        assert!(matches!(
            ThreadKey::from_key(unprivileged_bounce_key, DomainId(1)).retire(),
            Err(CapError::InsufficientRights)
        ));
        assert_eq!(
            nucleus.pending.state(record).ok(),
            Some(PendingState::Waiting)
        );

        // Self-retirement is rejected: the current Thread must survive
        // its own invocation (never-returns self-retirement is recorded
        // in the contract as wanted follow-up) — and again nothing was
        // torn down. The boot Thread's own capability (slot 40) is the
        // invoked key.
        let boot_thread_key = boot_key(50, 1);
        assert!(matches!(
            ThreadKey::from_key(boot_thread_key, DomainId(0)).retire(),
            Err(CapError::InvalidOperation)
        ));
        assert_eq!(
            nucleus.pending.state(record).ok(),
            Some(PendingState::Waiting)
        );

        assert_eq!(
            nucleus
                .pools
                .threads
                .get_live(usize::from(bounce_id.index))
                .expect("rejected retirement removed Bounce")
                .context,
            ExecutionContext::Parked { saved, record }
        );
        assert_eq!(fixture_stack_state(), (boot_execution_sp, shared_trap_sp));

        // Retire Bounce through the real SVC path: the parked record is
        // cancelled and released, and no queued wakeup survives.
        ThreadKey::from_key(bounce_thread_key, DomainId(1))
            .retire()
            .unwrap_or_else(|error| panic!("Bounce Retire failed: {:?}", error.code()));
        assert!(nucleus.pending.is_empty());
        assert!(nucleus.scheduler.is_empty());
        // The released record's identity is stale.
        nucleus.pending.state(record).unwrap_err();

        // N2 no longer holds Bounce: a signal through the real SVC path
        // delivers to no dead waiter — it succeeds and the bits stay
        // pending — and the notification remains fully usable.
        NotificationKey::from_key(n2_key)
            .signal(0b100)
            .unwrap_or_else(|error| panic!("post-retire Signal failed: {:?}", error.code()));
        let bits = NotificationKey::from_key(n2_key)
            .wait(NotificationKey::WAIT_INFINITE)
            .unwrap_or_else(|error| panic!("post-retire Wait failed: {:?}", error.code()));
        assert_eq!(bits, 0b100);

        // The retired Thread's pool slot is reclaimed and its capability
        // is stale: the identity no longer resolves, and a further Retire
        // through the same capability fails with a defined error.
        nucleus.pools.threads.validate(bounce_id).unwrap_err();
        // Thread retirement does not retire its AddressSpace or reset the
        // shared table's counters/backing; the binding remains live.
        nucleus
            .pools
            .arch
            .address_spaces
            .validate(bounce_as_id)
            .unwrap_or_else(|_| panic!("Thread retirement invalidated Bounce's AddressSpace"));
        assert_eq!(
            nucleus
                .pools
                .arch
                .address_spaces
                .get_live(usize::from(bounce_as_id.index))
                .expect("Thread retirement removed Bounce's AddressSpace")
                .keytable()
                .address(),
            bounce_table_binding.address()
        );
        assert!(
            nucleus
                .pools
                .threads
                .get_live(usize::from(bounce_id.index))
                .is_none()
        );
        // Its capability still passes key lookup, but names a retired
        // object: Retire reports that against the invoked key.
        assert_eq!(
            ThreadKey::from_key(bounce_thread_key, DomainId(1))
                .retire()
                .map_err(CapError::code),
            Err(CapError::InconsistentKey {
                key: bounce_thread_key,
                reason: InconsistencyReason::ObjectRetired,
                operand: 0,
            }
            .code())
        );
    }

    semi::println!("Synchronization suite passed");
}

static BOUNCE_N1_KEY: AtomicU64 = AtomicU64::new(0);
static BOUNCE_N2_KEY: AtomicU64 = AtomicU64::new(0);
static BOUNCE_EC_KEY: AtomicU64 = AtomicU64::new(0);

fn assert_bounce_parked_context(thread: &Thread, stack_bottom: u64, stack_top: u64) {
    let ExecutionContext::Parked { saved, .. } = thread.context else {
        panic!("Bounce has no Thread-resident parked context")
    };
    assert_eq!(saved.origin, ExceptionOrigin::CurrentSp0);
    assert_eq!(saved.spsr_el1 & 0xf, 4, "Bounce must resume as EL1t");
    assert_eq!(
        saved.spsr_el1 & 0x3c0,
        0x3c0,
        "Bounce must retain its DAIF masks"
    );
    assert_eq!(saved.sp & 15, 0);
    assert!(saved.sp >= stack_bottom && saved.sp < stack_top);
    assert_ne!(saved.sp, fixture_trap_sp());
    assert_ne!(saved.elr_el1, 0);
    assert_ne!(saved.lr, 0);
    translation::assert_parked_registers(
        &saved,
        RawKey::from_wire(BOUNCE_N2_KEY.load(Ordering::Acquire)),
    );
}

// ─────────────────────────────────────────────────────────────────────
// Bounce fixture domain (completion foundation, 2026-09-16)
// ─────────────────────────────────────────────────────────────────────

/// The bits Bounce delivers to the blocked boot domain.
const BOUNCE_MAGIC_BITS: u64 = 0b1010_1010;

/// The Bounce fixture domain's entry (N4-A, 2026-09-16): a second EL1
/// execution context that unlocks the first blocked domain for testing.
///
/// Bounce first signals the notification the boot domain is blocked on
/// (waking it). It then serves one `EventCount` advance per N2 trigger:
/// trigger bit 0 requests a plain +3 advance, trigger bit 1 an overflowing
/// one (which completes waiters with the shared `CounterOverflow` error,
/// 2026-09-18). After two rounds it parks forever on N2 — nothing ever
/// signals it again, so the scheduler resumes the boot domain.
/// Bootstrap-era fixture mechanism, not the Phase 7 Activate contract: no
/// budget, no EL0 entry, no legal-transition enforcement beyond what this
/// path exercises.
/// Implementation status: Bounce's Thread now names its own `AddressSpace` and
/// its provisioned table, but still runs through the kernel high map without
/// an actual translation-context switch. This is not a PPC migration trial.
/// Implementation status update: the linked low image now runs under Bounce's
/// independent ASID-2 root, selected by the parent scheduling path. The trusted
/// high execution stack is retained; this remains a two-Thread test, not PPC.
/// Implementation status: execution uses `EL1t`/`SP_EL0`, with the same shared
/// high `SP_EL1` trap stack as the boot Thread and Thread-resident saved state.
#[unsafe(no_mangle)]
extern "C" fn bounce_entry() -> ! {
    translation::observe_bounce();
    let initial_stack_state = fixture_stack_state();
    assert_eq!(initial_stack_state.1, fixture_trap_sp());
    let n1 = NotificationKey::from_key(RawKey::from_wire(BOUNCE_N1_KEY.load(Ordering::Acquire)));
    let n2_key = RawKey::from_wire(BOUNCE_N2_KEY.load(Ordering::Acquire));
    let ec = EventCountKey::from_key(RawKey::from_wire(BOUNCE_EC_KEY.load(Ordering::Acquire)));
    n1.signal(BOUNCE_MAGIC_BITS)
        .unwrap_or_else(|error| panic!("Bounce: Notification.Signal failed: {:?}", error.code()));
    // Serve one EventCount advance per N2 trigger, then park forever.
    for round in 0..2_u64 {
        let trigger = translation::notification_wait(n2_key).unwrap_or_else(|error| {
            panic!("Bounce: round {round} wait failed: {:?}", error.code())
        });
        assert_eq!(fixture_stack_state(), initial_stack_state);
        translation::observe_bounce();
        let result = match trigger {
            // A plain +3 advance that satisfies the boot domain's target.
            0b1 => ec.advance(3),
            // The overflowing advance: it completes waiters with the shared
            // error and leaves the counter unchanged (selected 2026-09-18).
            0b10 => ec.advance(u64::MAX),
            other => panic!("Bounce: unexpected trigger bits {other:#x}"),
        };
        if trigger == 0b10 {
            assert!(
                matches!(result, Err(CapError::CounterOverflow)),
                "Bounce: round {round} advance should overflow"
            );
        } else {
            result.unwrap_or_else(|error| {
                panic!("Bounce: round {round} advance failed: {:?}", error.code())
            });
        }
    }
    // Park forever: this wait blocks, so the scheduler resumes the boot
    // domain. Reaching either arm below is a fixture failure.
    match translation::notification_wait(n2_key) {
        Ok(bits) => panic!("Bounce: unexpected wakeup with bits {bits:#x}"),
        Err(error) => panic!("Bounce: Notification.Wait failed: {:?}", error.code()),
    }
}

//! `Scheduler.ConfigureThread`/`StartThread` through production dispatch on
//! the serial QEMU nucleus fixture. Threads, Times and the Scheduler are
//! carved from the unit-test arena standing in for Untyped memory.

use {
    super::{FIXTURE_GUARD, cap_invoke, with_nucleus},
    crate::{
        api::{InvokeOutcome, KeyEntry},
        objects::{
            ArchObjects, ArchObjectsImpl, EventCount, ExecutionContext, Notification, Nucleus,
            Scheduler, Thread, Time,
            arch_objects::{AddressSpaceId, AddressSpaceObject},
            carved::{Carved, CarvedId},
            notification::WaitOutcome,
            nucleus::test_arena,
            thread::ThreadId,
        },
    },
    core::cell::UnsafeCell,
    libexception::arch::aarch64::SavedContext,
    libobject::{
        CapError, KeySlot, RawKey, Rights,
        scheduler::SchedulerOp,
        scheduler_record::{
            BlockReason, FLAG_AWAITING_BUDGET, FLAG_HAS_TIME, SchedulerRecord, ThreadRecord,
            ThreadSnapshot,
        },
        thread::ThreadState,
    },
};

const ROOT: u64 = 0x2100_0000;
const ENTRY: u64 = 0x40_0000;
const STACK: u64 = 0x7f_0000;
const ARGUMENT: u64 = 0xA5;

/// Keys installed in the caller's table, plus the inert Thread's identity.
struct Keys {
    scheduler: RawKey,
    thread: RawKey,
    thread_id: ThreadId,
    address_space: RawKey,
    time: RawKey,
}

fn install(nucleus: &mut Nucleus<ArchObjectsImpl>, slot: u32, entry: KeyEntry) -> RawKey {
    nucleus
        .current_thread_table_mut()
        .unwrap()
        .insert(KeySlot(slot), entry, FIXTURE_GUARD)
        .unwrap_or_else(|failure| panic!("fixture install failed: {:?}", failure.error.code()))
}

/// A caller Thread in `fixture_as`, which gets a root and an ASID, and the
/// keys ConfigureThread needs, each with `rights` (the Scheduler with
/// `scheduler_rights`).
fn setup(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    fixture_as: AddressSpaceId,
    rights: Rights,
    scheduler: Scheduler,
) -> Keys {
    nucleus
        .create_console_thread(fixture_as)
        .expect("caller Thread");
    nucleus.current_thread = nucleus.fixture_thread(0);
    let address_space = nucleus.address_space_mut(fixture_as).unwrap();
    address_space.set_translation_root(Some(ROOT));
    address_space.set_asid(Some(3));

    let scheduler_id = test_arena::carve(scheduler).expect("arena exhausted");
    let time_id = test_arena::carve(Time::empty(0)).expect("arena exhausted");
    let thread_id = test_arena::carve(Thread::new(CarvedId::UNBOUND, ExecutionContext::Inactive))
        .expect("arena exhausted");
    Keys {
        scheduler: install(
            nucleus,
            20,
            KeyEntry::new_carved(scheduler_id, Rights::all(), 0),
        ),
        thread: install(nucleus, 21, KeyEntry::new_thread(thread_id, rights, 0)),
        thread_id,
        address_space: install(nucleus, 22, KeyEntry::new_carved(fixture_as, rights, 0)),
        time: install(nucleus, 23, KeyEntry::new_carved(time_id, rights, 0)),
    }
}

fn configure(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    keys: &Keys,
    entry: u64,
    stack: u64,
) -> Result<(), CapError> {
    cap_invoke(
        nucleus,
        keys.scheduler,
        SchedulerOp::ConfigureThread as u64,
        &[
            keys.thread.to_wire(),
            keys.address_space.to_wire(),
            entry,
            stack,
            keys.time.to_wire(),
            ARGUMENT,
        ],
    )
    .map(|_| ())
}

fn start(nucleus: &mut Nucleus<ArchObjectsImpl>, keys: &Keys) -> Result<(), CapError> {
    cap_invoke(
        nucleus,
        keys.scheduler,
        SchedulerOp::StartThread as u64,
        &[keys.thread.to_wire(), 0, 0, 0, 0, 0],
    )
    .map(|_| ())
}

#[test_case]
fn configure_then_start_makes_an_inert_thread_runnable() {
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        let keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        // Starting before configuring is refused.
        assert!(matches!(
            start(nucleus, &keys),
            Err(CapError::InvalidOperation)
        ));

        configure(nucleus, &keys, ENTRY, STACK)
            .unwrap_or_else(|error| panic!("ConfigureThread: {:?}", error.code()));
        let thread = nucleus.thread(keys.thread_id).unwrap();
        assert_eq!(thread.address_space, fixture_as);
        assert!(thread.time.is_some());
        assert!(thread.scheduler.is_some());
        assert_eq!(
            thread.context,
            ExecutionContext::Configured {
                saved: SavedContext::el0(ENTRY, STACK, ARGUMENT)
            }
        );
        assert!(nucleus.activations.is_empty());
        // A configured Thread cannot be configured again.
        assert!(matches!(
            configure(nucleus, &keys, ENTRY, STACK),
            Err(CapError::InvalidOperation)
        ));

        // A Thread runs only on funded Time: give it some before starting.
        let root = install_root_time(nucleus, 24);
        time_op(
            nucleus,
            root,
            libobject::TimeOp::Transfer,
            [keys.time.to_wire(), 1_000_000, 0, 0, 0, 0],
        )
        .unwrap_or_else(|error| panic!("fund the Time: {:?}", error.code()));
        start(nucleus, &keys).unwrap_or_else(|error| panic!("StartThread: {:?}", error.code()));
        assert_eq!(
            nucleus.thread(keys.thread_id).unwrap().context,
            ExecutionContext::NotStarted {
                saved: SavedContext::el0(ENTRY, STACK, ARGUMENT)
            }
        );
        // Nothing runs it: only a dispatcher does.
        assert!(nucleus.activations.is_empty());
        // Starting twice is refused.
        assert!(matches!(
            start(nucleus, &keys),
            Err(CapError::InvalidOperation)
        ));
        while nucleus.activations.pop().is_some() {}
    });
}

#[test_case]
fn configure_rejects_bad_start_state_and_leaves_the_thread_inert() {
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        let keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        for (entry, stack) in [
            (ENTRY + 2, STACK),
            (ENTRY, STACK + 8),
            (ArchObjectsImpl::USER_VA_END, STACK),
            (ENTRY, ArchObjectsImpl::USER_VA_END + 16),
        ] {
            assert!(matches!(
                configure(nucleus, &keys, entry, stack),
                Err(CapError::InvalidOperation)
            ));
        }
        assert_eq!(
            nucleus.thread(keys.thread_id).unwrap().context,
            ExecutionContext::Inactive
        );
    });
}

#[test_case]
fn configure_requires_rights_and_a_mapped_address_space() {
    // Missing WRITE/MAP on the Thread, AddressSpace and Time keys.
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        let keys = setup(nucleus, fixture_as, Rights(Rights::READ), Scheduler::root());
        assert!(matches!(
            configure(nucleus, &keys, ENTRY, STACK),
            Err(CapError::InsufficientRights)
        ));
    });
    // An AddressSpace without an ASID.
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        let keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        nucleus
            .address_space_mut(fixture_as)
            .unwrap()
            .set_asid(None);
        assert!(matches!(
            configure(nucleus, &keys, ENTRY, STACK),
            Err(CapError::NoASIDAssigned)
        ));
    });
    // A Thread key in the AddressSpace operand is the wrong kind.
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        let mut keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        keys.address_space = keys.thread;
        assert!(matches!(
            configure(nucleus, &keys, ENTRY, STACK),
            Err(CapError::TypeMismatch { .. })
        ));
    });
}

/// Carve a root Time with an infinite budget and install it at `slot`.
fn install_root_time(nucleus: &mut Nucleus<ArchObjectsImpl>, slot: u32) -> RawKey {
    let id = test_arena::carve(Time::empty(0)).expect("arena exhausted");
    // SAFETY: the record was just carved into the test arena; nothing else
    // references it.
    unsafe {
        *crate::objects::carved::Carved::<Time>::validate(id)
            .ok()
            .expect("root Time carved") = Time::root(0, libobject::time::INFINITE_BUDGET);
    }
    install(nucleus, slot, KeyEntry::new_carved(id, Rights::all(), 0))
}

fn time_op(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    key: RawKey,
    op: libobject::TimeOp,
    args: [u64; 6],
) -> Result<(), CapError> {
    cap_invoke(nucleus, key, op as u64, &args).map(|_| ())
}

#[test_case]
fn a_bound_time_is_its_threads_own() {
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        let keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        let root = install_root_time(nucleus, 24);
        let spare = {
            let id = test_arena::carve(Time::empty(0)).expect("arena exhausted");
            install(nucleus, 25, KeyEntry::new_carved(id, Rights::all(), 0))
        };
        // Give the Thread's Time a budget, then bind it.
        time_op(
            nucleus,
            root,
            libobject::TimeOp::Transfer,
            [keys.time.to_wire(), 1_000, 0, 0, 0, 0],
        )
        .unwrap_or_else(|error| panic!("fund the Time: {:?}", error.code()));
        configure(nucleus, &keys, ENTRY, STACK)
            .unwrap_or_else(|error| panic!("ConfigureThread: {:?}", error.code()));

        // Nothing drains it: neither a Transfer out nor a Merge out.
        assert!(matches!(
            time_op(
                nucleus,
                keys.time,
                libobject::TimeOp::Transfer,
                [spare.to_wire(), 1, 0, 0, 0, 0],
            ),
            Err(CapError::InvalidOperation)
        ));
        assert!(matches!(
            time_op(
                nucleus,
                spare,
                libobject::TimeOp::Merge,
                [keys.time.to_wire(), 0, 0, 0, 0, 0],
            ),
            Err(CapError::InvalidOperation)
        ));
        // Budget can still flow in.
        time_op(
            nucleus,
            root,
            libobject::TimeOp::Transfer,
            [keys.time.to_wire(), 500, 0, 0, 0, 0],
        )
        .unwrap_or_else(|error| panic!("top up the bound Time: {:?}", error.code()));

        // A second Thread cannot be bound to the same Time.
        let second = test_arena::carve(Thread::new(CarvedId::UNBOUND, ExecutionContext::Inactive))
            .expect("arena exhausted");
        let second_keys = Keys {
            thread: install(nucleus, 26, KeyEntry::new_thread(second, Rights::all(), 0)),
            thread_id: second,
            ..keys
        };
        assert!(matches!(
            configure(nucleus, &second_keys, ENTRY, STACK),
            Err(CapError::InvalidOperation)
        ));
        assert_eq!(
            nucleus.thread(second).unwrap().context,
            ExecutionContext::Inactive
        );

        // Once the bound Thread is retired the Time is free again.
        // SAFETY: the Thread is a test-arena record nobody references now.
        unsafe { crate::objects::carved::Carved::<Thread>::retire(keys.thread_id) }
            .ok()
            .expect("retire the bound Thread");
        time_op(
            nucleus,
            keys.time,
            libobject::TimeOp::Transfer,
            [spare.to_wire(), 1, 0, 0, 0, 0],
        )
        .unwrap_or_else(|error| panic!("drain after retirement: {:?}", error.code()));
        configure(nucleus, &second_keys, ENTRY, STACK)
            .unwrap_or_else(|error| panic!("rebind after retirement: {:?}", error.code()));
    });
}

#[test_case]
fn a_thread_started_on_an_unfunded_time_waits_until_a_transfer_arrives() {
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        let keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        let root = install_root_time(nucleus, 24);
        configure(nucleus, &keys, ENTRY, STACK)
            .unwrap_or_else(|error| panic!("ConfigureThread: {:?}", error.code()));

        // The Time is empty: the Thread starts but stays off the run queue.
        start(nucleus, &keys).unwrap_or_else(|error| panic!("StartThread: {:?}", error.code()));
        assert!(nucleus.activations.is_empty());
        let thread = nucleus.thread(keys.thread_id).unwrap();
        assert!(thread.awaiting_budget);
        assert_eq!(
            thread.context,
            ExecutionContext::NotStarted {
                saved: SavedContext::el0(ENTRY, STACK, ARGUMENT)
            }
        );

        // A Transfer into its Time ends the wait.
        time_op(
            nucleus,
            root,
            libobject::TimeOp::Transfer,
            [keys.time.to_wire(), 1_000_000, 0, 0, 0, 0],
        )
        .unwrap_or_else(|error| panic!("fund the Time: {:?}", error.code()));
        assert!(nucleus.activations.is_empty());
        assert!(!nucleus.thread(keys.thread_id).unwrap().awaiting_budget);

        // A second Transfer changes nothing.
        time_op(
            nucleus,
            root,
            libobject::TimeOp::Transfer,
            [keys.time.to_wire(), 1_000, 0, 0, 0, 0],
        )
        .unwrap_or_else(|error| panic!("top up the Time: {:?}", error.code()));
        assert!(nucleus.activations.is_empty());
    });
}

/// `unwrap` for capability results: `CapError` has no `Debug`, so show the
/// wire status instead.
trait ExpectOk<T> {
    fn expect_ok(self) -> T;
}

impl<T> ExpectOk<T> for Result<T, CapError> {
    fn expect_ok(self) -> T {
        self.unwrap_or_else(|error| panic!("unexpected capability error {:?}", error.code()))
    }
}

// ═══════════════════════════════════════════════════════════════
// SCHEDULER RECORDS
// ═══════════════════════════════════════════════════════════════

/// Three pages the tests share as record Frames. The unit-test binaries run
/// without translation, so a page's address is its physical address and the
/// nucleus's kernel mapping of it.
#[repr(C, align(4096))]
struct RecordPages(UnsafeCell<[u8; 3 * 4096]>);

// SAFETY: the tests run one at a time and touch the pages only through
// atomic record accesses and whole-slot writes.
unsafe impl Sync for RecordPages {}

static RECORD_PAGES: RecordPages = RecordPages(UnsafeCell::new([0; 3 * 4096]));

fn page_paddr(index: u64) -> u64 {
    RECORD_PAGES.0.get() as u64 + index * 4096
}

fn clear_pages() {
    // SAFETY: serial tests; nothing else uses the pages.
    unsafe { RECORD_PAGES.0.get().write([0; 3 * 4096]) };
}

/// A record Frame key for page `index`, installed at `slot`, mapped in
/// `mapped_in` if given.
fn frame_key(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    slot: u32,
    index: u64,
    rights: u8,
    device: bool,
    mapped_in: Option<AddressSpaceId>,
) -> RawKey {
    let mut entry = KeyEntry::new_frame(page_paddr(index), 12, device, Rights(rights));
    if let Some(space) = mapped_in {
        entry
            .as_frame_mut()
            .unwrap_or_else(|_| panic!("a Frame entry"))
            .set_mapped(space, 0x1000_0000 + index * 0x1000);
    }
    install(nucleus, slot, entry)
}

fn share(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    keys: &Keys,
    frame: RawKey,
    first_slot: u64,
    stride: u64,
) -> Result<(), CapError> {
    cap_invoke(
        nucleus,
        keys.scheduler,
        SchedulerOp::ShareRegion as u64,
        &[frame.to_wire(), first_slot, stride, 0, 0, 0],
    )
    .map(|_| ())
}

fn assign(nucleus: &mut Nucleus<ArchObjectsImpl>, keys: &Keys, slot: u64) -> Result<(), CapError> {
    cap_invoke(
        nucleus,
        keys.scheduler,
        SchedulerOp::AssignRecord as u64,
        &[keys.thread.to_wire(), slot, 0, 0, 0, 0],
    )
    .map(|_| ())
}

/// The Thread record in `slot` of the first page, read back.
fn thread_snapshot(slot: u64, stride: u64) -> ThreadSnapshot {
    // SAFETY: serial tests; the page is live and 64-byte aligned.
    let record = unsafe { ThreadRecord::at((page_paddr(0) + slot * stride) as *const u8) };
    record.try_snapshot(1).expect("a consistent record")
}

fn scheduler_record() -> &'static SchedulerRecord {
    // SAFETY: serial tests; slot 0 of the first page.
    unsafe { SchedulerRecord::at(page_paddr(0) as *const u8) }
}

#[test_case]
fn share_region_checks_authority_and_the_frame() {
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        clear_pages();
        let keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        let rw = Rights::READ | Rights::WRITE;
        let mapped = Some(fixture_as);

        // A Frame must be RAM, mapped, and readable and writable.
        let device = frame_key(nucleus, 30, 0, rw, true, mapped);
        let unmapped = frame_key(nucleus, 31, 0, rw, false, None);
        let read_only = frame_key(nucleus, 32, 0, Rights::READ, false, mapped);
        assert!(matches!(
            share(nucleus, &keys, device, 0, 128),
            Err(CapError::InvalidOperation)
        ));
        assert!(matches!(
            share(nucleus, &keys, unmapped, 0, 128),
            Err(CapError::NotMapped)
        ));
        assert!(matches!(
            share(nucleus, &keys, read_only, 0, 128),
            Err(CapError::InsufficientRights)
        ));
        // A key that is not a Frame, and a zeroed reserved word.
        assert!(matches!(
            share(nucleus, &keys, keys.thread, 0, 128),
            Err(CapError::TypeMismatch { .. })
        ));
        let frame = frame_key(nucleus, 33, 0, rw, false, mapped);
        assert!(matches!(
            cap_invoke(
                nucleus,
                keys.scheduler,
                SchedulerOp::ShareRegion as u64,
                &[frame.to_wire(), 0, 128, 1, 0, 0],
            ),
            Err(CapError::InvalidOperation)
        ));
        // Nothing was shared, so the page is untouched.
        assert_eq!(scheduler_record().layout(), (0, 0));
    });
}

#[test_case]
fn share_region_shapes_the_table() {
    with_nucleus(|nucleus, _table, _second, fixture_as, second_as| {
        clear_pages();
        let keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        let rw = Rights::READ | Rights::WRITE;
        let first = frame_key(nucleus, 30, 0, rw, false, Some(fixture_as));
        let second = frame_key(nucleus, 31, 1, rw, false, Some(fixture_as));
        let other_space = frame_key(nucleus, 32, 2, rw, false, Some(second_as));

        // The first region starts at slot 0 and fixes the stride.
        assert!(matches!(
            share(nucleus, &keys, first, 1, 128),
            Err(CapError::InvalidOperation)
        ));
        for stride in [0, 64, 130, 8192] {
            assert!(matches!(
                share(nucleus, &keys, first, 0, stride),
                Err(CapError::InvalidOperation)
            ));
        }
        share(nucleus, &keys, first, 0, 128)
            .unwrap_or_else(|error| panic!("ShareRegion: {:?}", error.code()));
        // The scheduler record is initialized with its layout header.
        assert_eq!(scheduler_record().layout(), (1, 128));

        // The same Frame again, a different stride, overlapping slots and a
        // Frame mapped in another AddressSpace are refused.
        assert!(matches!(
            share(nucleus, &keys, first, 32, 128),
            Err(CapError::InvalidOperation)
        ));
        assert!(matches!(
            share(nucleus, &keys, second, 32, 192),
            Err(CapError::InvalidOperation)
        ));
        assert!(matches!(
            share(nucleus, &keys, second, 31, 128),
            Err(CapError::InvalidOperation)
        ));
        assert!(matches!(
            share(nucleus, &keys, other_space, 32, 128),
            Err(CapError::InvalidOperation)
        ));
        // The next Frame, with the next slots, is accepted.
        share(nucleus, &keys, second, 32, 128)
            .unwrap_or_else(|error| panic!("second ShareRegion: {:?}", error.code()));
    });
}

#[test_case]
fn assign_record_links_a_thread_and_publishes_its_state() {
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        clear_pages();
        let keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        let frame = frame_key(
            nucleus,
            30,
            0,
            Rights::READ | Rights::WRITE,
            false,
            Some(fixture_as),
        );
        // No table yet.
        assert!(matches!(
            assign(nucleus, &keys, 1),
            Err(CapError::InvalidOperation)
        ));
        share(nucleus, &keys, frame, 0, 128).expect_ok();
        // The Scheduler record's slot, a slot outside the table, and a
        // Thread this Scheduler does not run are refused.
        assert!(matches!(
            assign(nucleus, &keys, 0),
            Err(CapError::InvalidOperation)
        ));
        assert!(matches!(
            assign(nucleus, &keys, 32),
            Err(CapError::InvalidOperation)
        ));
        assert!(matches!(
            assign(nucleus, &keys, 1),
            Err(CapError::InvalidOperation)
        ));
        assert!(nucleus.thread(keys.thread_id).unwrap().record.is_none());

        // Configured on a Time, the Thread is Inactive in its record.
        configure(nucleus, &keys, ENTRY, STACK).expect_ok();
        assign(nucleus, &keys, 5).unwrap_or_else(|e| panic!("AssignRecord: {:?}", e.code()));
        let snapshot = thread_snapshot(5, 128);
        assert_eq!(snapshot.thread_state(), Some(ThreadState::Inactive));
        assert_eq!(snapshot.incarnation, keys.thread_id.generation);
        assert!(snapshot.has_time());
        assert_eq!(snapshot.flags & FLAG_HAS_TIME, FLAG_HAS_TIME);
        assert_eq!(snapshot.budget_remaining_ns, 0);
        assert_eq!(snapshot.generation, 1);
        // Another slot's record stayed untouched.
        assert_eq!(thread_snapshot(6, 128), ThreadSnapshot::default());
    });
}

#[test_case]
fn records_follow_a_thread_from_start_through_exhaustion_to_retirement() {
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        clear_pages();
        let keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        let frame = frame_key(
            nucleus,
            30,
            0,
            Rights::READ | Rights::WRITE,
            false,
            Some(fixture_as),
        );
        let root = install_root_time(nucleus, 24);
        share(nucleus, &keys, frame, 0, 128).expect_ok();
        configure(nucleus, &keys, ENTRY, STACK).expect_ok();
        assign(nucleus, &keys, 3).expect_ok();
        let generation = thread_snapshot(3, 128).generation;

        // Started on an empty Time: runnable, but awaiting budget.
        start(nucleus, &keys).expect_ok();
        let starved = thread_snapshot(3, 128);
        assert_eq!(starved.thread_state(), Some(ThreadState::Runnable));
        assert!(starved.awaits_budget());
        assert_eq!(starved.flags, FLAG_AWAITING_BUDGET | FLAG_HAS_TIME);
        assert!(starved.generation > generation);

        // Budget arrives: queued, no longer awaiting, with its budget shown.
        time_op(
            nucleus,
            root,
            libobject::TimeOp::Transfer,
            [keys.time.to_wire(), 5_000_000, 0, 0, 0, 0],
        )
        .expect_ok();
        let funded = thread_snapshot(3, 128);
        assert_eq!(funded.thread_state(), Some(ThreadState::Runnable));
        assert!(!funded.awaits_budget());
        assert_eq!(funded.budget_remaining_ns, 5_000_000);
        assert!(funded.generation > starved.generation);

        // Retiring it publishes its final state with the same incarnation.
        let thread_key = keys.thread;
        let before = thread_snapshot(3, 128).incarnation;
        cap_invoke(
            nucleus,
            thread_key,
            libobject::ThreadOp::Retire as u64,
            &[0; 6],
        )
        .unwrap_or_else(|e| panic!("Thread.Retire: {:?}", e.code()));
        let dying = thread_snapshot(3, 128);
        assert_eq!(dying.thread_state(), Some(ThreadState::Dying));
        assert_eq!(dying.incarnation, before);
        assert!(!dying.awaits_budget());
    });
}

#[test_case]
fn the_bound_event_count_advances_with_each_published_state_change() {
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        clear_pages();
        let keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        let frame = frame_key(
            nucleus,
            30,
            0,
            Rights::READ | Rights::WRITE,
            false,
            Some(fixture_as),
        );
        let root = install_root_time(nucleus, 24);
        let event_count_id = test_arena::carve(EventCount::new()).expect("arena exhausted");
        let send_only = install(
            nucleus,
            34,
            KeyEntry::new_carved(event_count_id, Rights(Rights::SEND), 0),
        );
        let receive_only = install(
            nucleus,
            35,
            KeyEntry::new_carved(event_count_id, Rights(Rights::RECV), 0),
        );
        let bind = |nucleus: &mut Nucleus<ArchObjectsImpl>, key: RawKey, operand: u64| {
            cap_invoke(
                nucleus,
                keys.scheduler,
                SchedulerOp::BindEventCount as u64,
                &[key.to_wire(), operand, 0, 0, 0, 0],
            )
        };
        // Needs SEND on a live EventCount; trailing words are zero.
        assert!(matches!(
            bind(nucleus, receive_only, 0),
            Err(CapError::InsufficientRights)
        ));
        assert!(matches!(
            bind(nucleus, keys.thread, 0),
            Err(CapError::TypeMismatch { .. })
        ));
        assert!(matches!(
            bind(nucleus, send_only, 1),
            Err(CapError::InvalidOperation)
        ));
        share(nucleus, &keys, frame, 0, 128).expect_ok();
        bind(nucleus, send_only, 0).unwrap_or_else(|e| panic!("BindEventCount: {:?}", e.code()));
        // Binding is once-only.
        assert!(matches!(
            bind(nucleus, send_only, 0),
            Err(CapError::InvalidOperation)
        ));

        let count = |nucleus: &Nucleus<ArchObjectsImpl>| {
            let _ = nucleus;
            // SAFETY: a test-arena EventCount nothing else references now.
            unsafe {
                (*crate::objects::carved::Carved::<EventCount>::validate(event_count_id)
                    .ok()
                    .expect("EventCount carved"))
                .read()
            }
        };
        assert_eq!(count(nucleus), 0);
        configure(nucleus, &keys, ENTRY, STACK).expect_ok();
        // Linking a record publishes the Thread once.
        assign(nucleus, &keys, 2).expect_ok();
        assert_eq!(count(nucleus), 1);
        assert_eq!(scheduler_record().try_snapshot(1).unwrap().events, 1);
        // Starting it (awaiting budget) and funding it are two more changes.
        start(nucleus, &keys).expect_ok();
        assert_eq!(count(nucleus), 2);
        time_op(
            nucleus,
            root,
            libobject::TimeOp::Transfer,
            [keys.time.to_wire(), 1_000, 0, 0, 0, 0],
        )
        .expect_ok();
        assert_eq!(count(nucleus), 3);
        assert_eq!(scheduler_record().try_snapshot(1).unwrap().events, 3);
        // A top-up that changes no state advances nothing.
        time_op(
            nucleus,
            root,
            libobject::TimeOp::Transfer,
            [keys.time.to_wire(), 1_000, 0, 0, 0, 0],
        )
        .expect_ok();
        assert_eq!(count(nucleus), 3);
    });
}

/// The budget `Time.Query` reports for the Time at `key`.
fn queried_budget(nucleus: &mut Nucleus<ArchObjectsImpl>, key: RawKey) -> u64 {
    match cap_invoke(nucleus, key, libobject::TimeOp::Query as u64, &[0; 6]) {
        Ok(InvokeOutcome::Complete((budget, _))) => budget,
        _ => panic!("Time.Query failed"),
    }
}

#[test_case]
fn the_budget_in_a_record_matches_time_query() {
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        clear_pages();
        let keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        let frame = frame_key(
            nucleus,
            30,
            0,
            Rights::READ | Rights::WRITE,
            false,
            Some(fixture_as),
        );
        let root = install_root_time(nucleus, 24);
        share(nucleus, &keys, frame, 0, 128).expect_ok();
        configure(nucleus, &keys, ENTRY, STACK).expect_ok();
        assign(nucleus, &keys, 1).expect_ok();
        start(nucleus, &keys).expect_ok();
        time_op(
            nucleus,
            root,
            libobject::TimeOp::Transfer,
            [keys.time.to_wire(), 7_654_321, 0, 0, 0, 0],
        )
        .expect_ok();
        // The record's budget is the one Time.Query reports, and a mid-run
        // charge shows in neither until the next publication.
        let published = thread_snapshot(1, 128).budget_remaining_ns;
        assert_eq!(published, queried_budget(nucleus, keys.time));
        assert_eq!(published, 7_654_321);
        let current = nucleus.current_thread_id().expect_ok();
        nucleus.charge_current(current);
        let unchanged = thread_snapshot(1, 128).budget_remaining_ns;
        assert_eq!(unchanged, published);
    });
}

#[test_case]
fn a_reused_slot_starts_a_fresh_record_for_the_new_thread() {
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        clear_pages();
        let keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        let frame = frame_key(
            nucleus,
            30,
            0,
            Rights::READ | Rights::WRITE,
            false,
            Some(fixture_as),
        );
        share(nucleus, &keys, frame, 0, 128).expect_ok();
        configure(nucleus, &keys, ENTRY, STACK).expect_ok();
        assign(nucleus, &keys, 4).expect_ok();
        // Retiring the first Thread leaves its final record in the slot.
        cap_invoke(
            nucleus,
            keys.thread,
            libobject::ThreadOp::Retire as u64,
            &[0; 6],
        )
        .expect_ok();
        let dying = thread_snapshot(4, 128);
        assert_eq!(dying.thread_state(), Some(ThreadState::Dying));

        // A second Thread takes the same slot: the record is reset to it.
        let second_id =
            test_arena::carve(Thread::new(CarvedId::UNBOUND, ExecutionContext::Inactive))
                .expect("arena exhausted");
        let second_time = test_arena::carve(Time::empty(0)).expect("arena exhausted");
        let second = Keys {
            thread: install(
                nucleus,
                40,
                KeyEntry::new_thread(second_id, Rights::all(), 0),
            ),
            thread_id: second_id,
            time: install(
                nucleus,
                41,
                KeyEntry::new_carved(second_time, Rights::all(), 0),
            ),
            ..keys
        };
        configure(nucleus, &second, ENTRY, STACK).expect_ok();
        assign(nucleus, &second, 4).expect_ok();
        let fresh = thread_snapshot(4, 128);
        assert_eq!(fresh.thread_state(), Some(ThreadState::Inactive));
        assert_eq!(fresh.incarnation, second_id.generation);
        assert_eq!(fresh.generation, 1, "the record starts over");
        assert_eq!(
            fresh.stop_reason(),
            Some(libobject::scheduler_record::StopReason::None)
        );
    });
}

#[test_case]
fn a_published_change_wakes_the_agent_waiting_on_the_bound_event_count() {
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        clear_pages();
        let keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        let frame = frame_key(
            nucleus,
            30,
            0,
            Rights::READ | Rights::WRITE,
            false,
            Some(fixture_as),
        );
        let event_count_id = test_arena::carve(EventCount::new()).expect("arena exhausted");
        let event_count = install(
            nucleus,
            34,
            KeyEntry::new_carved(event_count_id, Rights::all(), 0),
        );
        share(nucleus, &keys, frame, 0, 128).expect_ok();
        cap_invoke(
            nucleus,
            keys.scheduler,
            SchedulerOp::BindEventCount as u64,
            &[event_count.to_wire(), 0, 0, 0, 0, 0],
        )
        .expect_ok();
        configure(nucleus, &keys, ENTRY, STACK).expect_ok();

        // The scheduler agent: a Thread that waits for the next event.
        let agent = nucleus.create_thread(fixture_as).expect("agent Thread");
        // SAFETY: a test-arena EventCount nothing else references now.
        let outcome = unsafe {
            (*crate::objects::carved::Carved::<EventCount>::validate(event_count_id)
                .ok()
                .expect("EventCount carved"))
            .await_ge(event_count_id, 1, agent, &mut nucleus.pending)
        };
        let Ok(crate::objects::event_count::AwaitOutcome::Blocked(record)) = outcome else {
            panic!("the agent should block on an unadvanced EventCount");
        };
        assert!(nucleus.activations.is_empty());

        // A published change advances the count and wakes the agent.
        assign(nucleus, &keys, 1).expect_ok();
        // The agent belongs to no Scheduler: nothing queues it, its wait is done.
        assert!(nucleus.activations.is_empty());
        assert!(matches!(
            nucleus.pending.state(record),
            Ok(crate::objects::completion::PendingState::Completed { .. })
        ));
    });
}

#[test_case]
fn a_woken_waiter_is_published_runnable_not_blocked() {
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        clear_pages();
        let keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        let frame = frame_key(
            nucleus,
            30,
            0,
            Rights::READ | Rights::WRITE,
            false,
            Some(fixture_as),
        );
        share(nucleus, &keys, frame, 0, 128).expect_ok();
        configure(nucleus, &keys, ENTRY, STACK).expect_ok();
        assign(nucleus, &keys, 5).unwrap_or_else(|e| panic!("AssignRecord: {:?}", e.code()));

        // The Thread waits on a Notification: its record says Blocked, and
        // why.
        let notification = test_arena::carve(Notification::new()).expect("arena exhausted");
        // SAFETY: a test-arena Notification nothing else references.
        let record = match unsafe {
            (*Carved::<Notification>::validate(notification)
                .ok()
                .expect("Notification carved"))
            .wait(notification, keys.thread_id, &mut nucleus.pending)
        } {
            Ok(WaitOutcome::Blocked(record)) => record,
            _ => panic!("wait should block"),
        };
        nucleus.thread_mut(keys.thread_id).unwrap().context = ExecutionContext::Parked {
            saved: SavedContext::el1t_interruptible(0xE0000, 0xF0000),
            record,
        };
        nucleus.publish_thread(keys.thread_id, false);
        let waiting = thread_snapshot(5, 128);
        assert_eq!(waiting.thread_state(), Some(ThreadState::Blocked));
        assert_eq!(waiting.block_reason, BlockReason::NotificationWait as u32);

        // A signal completes the wait. The Thread stays parked until it is
        // resumed, but it can run: the record must say so, or a dispatcher
        // reading it would never choose the Thread.
        // SAFETY: as above.
        unsafe {
            (*Carved::<Notification>::validate(notification)
                .ok()
                .expect("Notification carved"))
            .signal(0b1, &mut nucleus.pending)
        }
        .ok()
        .expect("signal wakes the waiter");
        nucleus.publish_thread(keys.thread_id, false);
        let woken = thread_snapshot(5, 128);
        assert_eq!(woken.thread_state(), Some(ThreadState::Runnable));
        assert_eq!(woken.block_reason, BlockReason::None as u32);
        assert!(woken.generation > waiting.generation);
    });
}

#[test_case]
fn interrupt_time_is_published_in_the_scheduler_record_of_the_interrupted_thread() {
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        clear_pages();
        let keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        let frame = frame_key(
            nucleus,
            30,
            0,
            Rights::READ | Rights::WRITE,
            false,
            Some(fixture_as),
        );
        share(nucleus, &keys, frame, 0, 128).expect_ok();
        configure(nucleus, &keys, ENTRY, STACK).expect_ok();
        assert_eq!(scheduler_record().try_snapshot(1).unwrap().interrupt_ns, 0);

        // An interrupt entry that interrupted a Thread of the Scheduler
        // refreshes the total, without counting an event.
        let frequency = nucleus.counter_frequency();
        let events = scheduler_record().try_snapshot(1).unwrap().events;
        nucleus.current_thread = Some(keys.thread_id);
        nucleus.account_interrupts(&[(5, frequency / 1_000)], frequency / 1_000);
        let published = scheduler_record().try_snapshot(1).unwrap();
        assert_eq!(published.interrupt_ns, 1_000_000);
        assert_eq!(published.events, events);

        // The total accumulates across entries.
        nucleus.account_interrupts(&[(5, frequency / 1_000)], frequency / 1_000);
        assert_eq!(
            scheduler_record().try_snapshot(1).unwrap().interrupt_ns,
            2_000_000
        );
    });
}

// ═══════════════════════════════════════════════════════════════
// SCHEDULER TREE: ADOPT, NON-ROOT SCHEDULERS
// ═══════════════════════════════════════════════════════════════

fn install_scheduler(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    slot: u32,
    scheduler: Scheduler,
) -> (RawKey, CarvedId) {
    let id = test_arena::carve(scheduler).expect("arena exhausted");
    (
        install(nucleus, slot, KeyEntry::new_carved(id, Rights::all(), 0)),
        id,
    )
}

fn scheduler_object(id: CarvedId) -> &'static mut Scheduler {
    // SAFETY: a test-arena Scheduler; serial tests.
    unsafe {
        &mut *Carved::<Scheduler>::validate(id)
            .ok()
            .expect("Scheduler carved")
    }
}

fn adopt(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    parent: RawKey,
    child: RawKey,
) -> Result<(), CapError> {
    cap_invoke(
        nucleus,
        parent,
        SchedulerOp::Adopt as u64,
        &[child.to_wire(), 0, 0, 0, 0, 0],
    )
    .map(|_| ())
}

#[test_case]
fn adopt_makes_a_fresh_scheduler_a_child_once() {
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        let keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        let (child_key, child) = install_scheduler(nucleus, 30, Scheduler::inert());
        let (other_key, other) = install_scheduler(nucleus, 31, Scheduler::inert());
        // An inert Scheduler is fresh; the root never is.
        assert!(scheduler_object(child).is_fresh());
        assert_eq!(scheduler_object(child).parent(), None);

        adopt(nucleus, keys.scheduler, child_key)
            .unwrap_or_else(|error| panic!("Adopt: {:?}", error.code()));
        assert_eq!(scheduler_object(child).depth(), 1);
        assert!(scheduler_object(child).parent().is_some());
        assert!(!scheduler_object(child).is_fresh());

        // The parent is set once, never to itself, and the root cannot be
        // adopted.
        assert!(matches!(
            adopt(nucleus, other_key, child_key),
            Err(CapError::InvalidOperation)
        ));
        assert!(matches!(
            adopt(nucleus, keys.scheduler, child_key),
            Err(CapError::InvalidOperation)
        ));
        assert!(matches!(
            adopt(nucleus, other_key, other_key),
            Err(CapError::InvalidOperation)
        ));
        assert!(matches!(
            adopt(nucleus, child_key, keys.scheduler),
            Err(CapError::InvalidOperation)
        ));
        // Rights and kinds are checked.
        let weak = install(
            nucleus,
            32,
            KeyEntry::new_carved(other, Rights(Rights::READ), 0),
        );
        assert!(matches!(
            adopt(nucleus, keys.scheduler, weak),
            Err(CapError::InsufficientRights)
        ));
        assert!(matches!(
            adopt(nucleus, keys.scheduler, keys.thread),
            Err(CapError::TypeMismatch { .. })
        ));
        assert!(scheduler_object(other).is_fresh());
    });
}

#[test_case]
fn adopt_refuses_a_scheduler_that_has_children_threads_or_a_dispatcher() {
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        let mut keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        let root_key = keys.scheduler;
        let (busy_key, busy) = install_scheduler(nucleus, 30, Scheduler::inert());
        // A Thread configured on it: not fresh.
        keys.scheduler = busy_key;
        configure(nucleus, &keys, ENTRY, STACK).expect_ok();
        assert_eq!(scheduler_object(busy).thread_count(), 1);
        assert!(matches!(
            adopt(nucleus, root_key, busy_key),
            Err(CapError::InvalidOperation)
        ));
        // A child of its own: not fresh.
        let (parent_key, _) = install_scheduler(nucleus, 31, Scheduler::inert());
        let (grandchild_key, _) = install_scheduler(nucleus, 32, Scheduler::inert());
        adopt(nucleus, parent_key, grandchild_key).expect_ok();
        assert!(matches!(
            adopt(nucleus, root_key, parent_key),
            Err(CapError::InvalidOperation)
        ));
        // A dispatcher: not fresh.
        let (dispatched_key, dispatched) = install_scheduler(nucleus, 33, Scheduler::inert());
        scheduler_object(dispatched)
            .bind_dispatcher(keys.thread_id)
            .ok()
            .expect("bind");
        assert!(matches!(
            adopt(nucleus, root_key, dispatched_key),
            Err(CapError::InvalidOperation)
        ));
    });
}

#[test_case]
fn the_tree_is_at_most_four_levels_deep() {
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        let keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        let mut parent = keys.scheduler;
        // The root plus three levels.
        for level in 1..=Scheduler::MAX_DEPTH {
            let (child_key, child) =
                install_scheduler(nucleus, 40 + u32::from(level), Scheduler::inert());
            adopt(nucleus, parent, child_key)
                .unwrap_or_else(|error| panic!("level {level}: {:?}", error.code()));
            assert_eq!(scheduler_object(child).depth(), level);
            parent = child_key;
        }
        let (too_deep, _) = install_scheduler(nucleus, 50, Scheduler::inert());
        assert!(matches!(
            adopt(nucleus, parent, too_deep),
            Err(CapError::InvalidOperation)
        ));
    });
}

#[test_case]
fn a_child_scheduler_has_no_authority_to_run_threads_until_it_has_a_dispatcher() {
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        let mut keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        let root_key = keys.scheduler;
        let (child_key, child) = install_scheduler(nucleus, 30, Scheduler::inert());
        adopt(nucleus, root_key, child_key).expect_ok();
        // Every operation works on the child; the Thread is its own.
        keys.scheduler = child_key;
        configure(nucleus, &keys, ENTRY, STACK).expect_ok();
        // Only the Scheduler that owns a Thread starts it.
        keys.scheduler = root_key;
        assert!(matches!(
            start(nucleus, &keys),
            Err(CapError::InvalidOperation)
        ));
        keys.scheduler = child_key;
        let queued_before = nucleus.activations.len();
        start(nucleus, &keys).expect_ok();
        // Started, but nothing queues it: its Scheduler has no dispatcher.
        assert_eq!(nucleus.activations.len(), queued_before);
        assert!(!nucleus.thread(keys.thread_id).unwrap().queued);
        // Retiring the Thread releases it from the child.
        assert_eq!(scheduler_object(child).thread_count(), 1);
        cap_invoke(
            nucleus,
            keys.thread,
            libobject::thread::ThreadOp::Retire as u64,
            &[0; 6],
        )
        .expect_ok();
        assert_eq!(scheduler_object(child).thread_count(), 0);
    });
}

fn move_thread_call(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    invoked: RawKey,
    thread: RawKey,
    destination: RawKey,
    slot: u64,
) -> Result<(), CapError> {
    cap_invoke(
        nucleus,
        invoked,
        SchedulerOp::MoveThread as u64,
        &[thread.to_wire(), destination.to_wire(), slot, 0, 0, 0],
    )
    .map(|_| ())
}

#[test_case]
fn a_thread_moves_down_to_a_child_and_back_and_the_old_record_reads_donated() {
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        clear_pages();
        let mut keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        let root_key = keys.scheduler;
        let rw = Rights::READ | Rights::WRITE;
        let (child_key, child) = install_scheduler(nucleus, 30, Scheduler::inert());
        let (stranger_key, stranger) = install_scheduler(nucleus, 31, Scheduler::inert());
        adopt(nucleus, root_key, child_key).expect_ok();
        // The root's table is page 0, the child's page 1.
        let root_frame = frame_key(nucleus, 32, 0, rw, false, Some(fixture_as));
        let child_frame = frame_key(nucleus, 33, 1, rw, false, Some(fixture_as));
        share(nucleus, &keys, root_frame, 0, 128).expect_ok();
        keys.scheduler = child_key;
        share(nucleus, &keys, child_frame, 0, 128).expect_ok();
        keys.scheduler = root_key;

        // A started Thread of the root, with a record in slot 5.
        fund(nucleus, &keys);
        configure(nucleus, &keys, ENTRY, STACK).expect_ok();
        assign(nucleus, &keys, 5).expect_ok();
        start(nucleus, &keys).expect_ok();

        // Moves go only between a Scheduler and its direct child.
        for (invoked, destination) in [
            (child_key, root_key),
            (root_key, root_key),
            (root_key, stranger_key),
            (stranger_key, child_key),
        ] {
            assert!(matches!(
                move_thread_call(nucleus, invoked, keys.thread, destination, 3),
                Err(CapError::InvalidOperation)
            ));
        }
        // A slot the destination does not cover, and slot 0, are refused.
        for slot in [0, 64] {
            assert!(matches!(
                move_thread_call(nucleus, root_key, keys.thread, child_key, slot),
                Err(CapError::InvalidOperation)
            ));
        }
        assert_eq!(
            nucleus.thread(keys.thread_id).unwrap().scheduler.is_some(),
            true
        );
        assert_eq!(scheduler_object(child).thread_count(), 0);
        let _ = stranger;

        // Donation: the child owns it, in its slot 3, and the root's record
        // reads Donated.
        move_thread_call(nucleus, root_key, keys.thread, child_key, 3)
            .unwrap_or_else(|error| panic!("MoveThread: {:?}", error.code()));
        assert_eq!(
            nucleus.thread(keys.thread_id).unwrap().scheduler,
            Some(child)
        );
        assert_eq!(scheduler_object(child).thread_count(), 1);
        let left = thread_snapshot(5, 128);
        assert_eq!(left.thread_state(), Some(ThreadState::Donated));
        // SAFETY: serial tests; the child's page is live and aligned.
        let arrived = unsafe { ThreadRecord::at((page_paddr(1) + 3 * 128) as *const u8) }
            .try_snapshot(1)
            .expect("a consistent record");
        assert_eq!(arrived.thread_state(), Some(ThreadState::Runnable));
        assert_eq!(arrived.incarnation, keys.thread_id.generation);

        // Only the owner's parent takes it back; the child cannot hand it
        // to its own child or away.
        assert!(matches!(
            move_thread_call(nucleus, root_key, keys.thread, child_key, 4),
            Err(CapError::InvalidOperation)
        ));
        move_thread_call(nucleus, root_key, keys.thread, root_key, 6)
            .unwrap_or_else(|error| panic!("MoveThread back: {:?}", error.code()));
        assert_ne!(
            nucleus.thread(keys.thread_id).unwrap().scheduler,
            Some(child)
        );
        assert_eq!(scheduler_object(child).thread_count(), 0);
        // SAFETY: serial tests; the child's page is live and aligned.
        let gone = unsafe { ThreadRecord::at((page_paddr(1) + 3 * 128) as *const u8) }
            .try_snapshot(1)
            .expect("a consistent record");
        assert_eq!(gone.thread_state(), Some(ThreadState::Donated));
        assert_eq!(
            thread_snapshot(6, 128).thread_state(),
            Some(ThreadState::Runnable)
        );
    });
}

#[test_case]
fn only_a_started_thread_that_is_not_running_or_a_dispatcher_moves() {
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        clear_pages();
        let mut keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        let root_key = keys.scheduler;
        let rw = Rights::READ | Rights::WRITE;
        let (child_key, _child) = install_scheduler(nucleus, 30, Scheduler::inert());
        adopt(nucleus, root_key, child_key).expect_ok();
        let child_frame = frame_key(nucleus, 33, 1, rw, false, Some(fixture_as));
        keys.scheduler = child_key;
        share(nucleus, &keys, child_frame, 0, 128).expect_ok();
        keys.scheduler = root_key;

        fund(nucleus, &keys);
        // Never started: refused.
        configure(nucleus, &keys, ENTRY, STACK).expect_ok();
        assert!(matches!(
            move_thread_call(nucleus, root_key, keys.thread, child_key, 3),
            Err(CapError::InvalidOperation)
        ));
        start(nucleus, &keys).expect_ok();
        // The root's dispatcher is never donated.
        let root_id = nucleus.thread(keys.thread_id).unwrap().scheduler.unwrap();
        scheduler_object(root_id)
            .bind_dispatcher(keys.thread_id)
            .ok()
            .expect("bind");
        assert!(matches!(
            move_thread_call(nucleus, root_key, keys.thread, child_key, 3),
            Err(CapError::InvalidOperation)
        ));
    });
}

/// Give the Time of `keys` an infinite budget, so its Thread is not held
/// back waiting for budget when it starts.
fn fund(nucleus: &mut Nucleus<ArchObjectsImpl>, keys: &Keys) {
    let id = nucleus
        .current_thread_table()
        .unwrap()
        .lookup(keys.time, FIXTURE_GUARD)
        .and_then(KeyEntry::carved_id)
        .unwrap_or_else(|error| panic!("Time lookup: {:?}", error.code()));
    // SAFETY: the record is in the test arena and nothing else references it.
    unsafe {
        *Carved::<Time>::validate(id).ok().expect("Time carved") =
            Time::root(0, libobject::time::INFINITE_BUDGET);
    }
}

#[test_case]
fn a_dispatchers_record_carries_the_dispatcher_flag() {
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        clear_pages();
        let keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        let frame = frame_key(
            nucleus,
            30,
            0,
            Rights::READ | Rights::WRITE,
            false,
            Some(fixture_as),
        );
        share(nucleus, &keys, frame, 0, 128).expect_ok();
        configure(nucleus, &keys, ENTRY, STACK).expect_ok();
        assign(nucleus, &keys, 5).expect_ok();
        assert!(!thread_snapshot(5, 128).is_dispatcher());
        // Dispatching a domain: the Thread runs a Scheduler.
        let domain = test_arena::carve(Scheduler::inert()).expect("arena exhausted");
        nucleus.thread_mut(keys.thread_id).unwrap().dispatches = Some(domain);
        nucleus.publish_thread(keys.thread_id, false);
        assert!(thread_snapshot(5, 128).is_dispatcher());
    });
}

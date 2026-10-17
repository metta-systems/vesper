//! `Scheduler.ConfigureThread`/`StartThread` through production dispatch on
//! the serial QEMU nucleus fixture. Threads, Times and the Scheduler are
//! carved from the unit-test arena standing in for Untyped memory.

use {
    super::{FIXTURE_GUARD, cap_invoke, with_nucleus},
    crate::{
        api::KeyEntry,
        objects::{
            ArchObjects, ArchObjectsImpl, ExecutionContext, Nucleus, Scheduler, Thread, Time,
            arch_objects::{AddressSpaceId, AddressSpaceObject},
            carved::CarvedId,
            nucleus::test_arena,
            thread::ThreadId,
        },
    },
    libexception::arch::aarch64::SavedContext,
    libobject::{CapError, KeySlot, RawKey, Rights, scheduler::SchedulerOp},
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
        assert!(nucleus.run_queue.is_empty());
        // A configured Thread cannot be configured again.
        assert!(matches!(
            configure(nucleus, &keys, ENTRY, STACK),
            Err(CapError::InvalidOperation)
        ));

        start(nucleus, &keys).unwrap_or_else(|error| panic!("StartThread: {:?}", error.code()));
        assert_eq!(
            nucleus.thread(keys.thread_id).unwrap().context,
            ExecutionContext::NotStarted {
                saved: SavedContext::el0(ENTRY, STACK, ARGUMENT)
            }
        );
        assert_eq!(nucleus.run_queue.peek(), Some(keys.thread_id));
        // Starting twice is refused.
        assert!(matches!(
            start(nucleus, &keys),
            Err(CapError::InvalidOperation)
        ));
        while nucleus.run_queue.pop().is_some() {}
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
fn configure_requires_rights_a_mapped_address_space_and_the_root_scheduler() {
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

#[test_case]
fn share_region_is_not_defined_yet() {
    with_nucleus(|nucleus, _table, _second, fixture_as, _second_as| {
        let keys = setup(nucleus, fixture_as, Rights::all(), Scheduler::root());
        assert!(matches!(
            cap_invoke(
                nucleus,
                keys.scheduler,
                SchedulerOp::ShareRegion as u64,
                &[0; 6]
            ),
            Err(CapError::InvalidOperation)
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

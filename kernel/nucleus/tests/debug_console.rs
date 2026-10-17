#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![feature(format_args_nl)]
#![feature(likely_unlikely)]
#![test_runner(libtest::test_runner)]
#![reexport_test_harness_main = "test_main"]

#[path = "../../../tests/common/mod.rs"]
mod common;

// Compile the production module trees, not replacement capability/handler models.
// The binary also allows unused items while these APIs are being brought up.
#[allow(unused)]
#[path = "../src/api/mod.rs"]
mod api;
#[allow(unused)]
#[path = "../src/objects/mod.rs"]
mod objects;

// The interrupt-controller component as the IRQ operations reach it.
#[allow(dead_code)]
#[path = "../src/platform.rs"]
mod platform;

// Only the clock is used here; the tick is armed by the nucleus binary.
#[allow(dead_code)]
#[path = "../src/timer.rs"]
mod timer;

#[path = "support/resume.rs"]
mod resume_tests;

#[path = "support/irq.rs"]
mod irq_tests;

#[path = "support/scheduler.rs"]
mod scheduler_tests;

#[path = "support/invocation_call.rs"]
mod invocation_call_tests;

use {
    api::{KeyEntry, debug_console::invoke, key_entry::ThreadSelector},
    core::{
        mem::{MaybeUninit, align_of, size_of},
        num::NonZero,
    },
    libobject::{
        CapError, InconsistencyReason, InvalidKeyReason, KeySlot, ObjectType, PlacementWindow,
        RawKey, Rights, decode_syscall_result,
    },
    objects::{
        ArchObjects, ArchObjectsImpl, ExecutionContext, KeyTable, Nucleus, ObjectPool, Thread,
        access::{Access, ObjectId, PoolTag},
        arch::ArchPools,
        arch_objects::{AddressSpaceId, AddressSpaceObject},
        carved::{Carved, CarvedId, FIRST_GENERATION},
        completion::{PendingKind, PendingState},
        invocation::InvocationStackExtent,
        key_table::{CallerTable, KeyTableBinding},
        nucleus::NucleusPools,
        object_pool::IdentityError,
        thread::ThreadId,
    },
};

// The debug console is a stateless singleton, not a pool object; its entry
// carries a null identity and is validated by type only (see the debug-only
// exception in doc/capabilities-contract.md).
fn console_entry(rights: Rights, badge: u16) -> KeyEntry {
    KeyEntry::from_id(
        ObjectType::DEBUG_CONSOLE,
        ObjectId {
            pool: PoolTag::Region,
            index: 0,
            generation: 0,
        },
        rights,
        badge,
    )
    .unwrap_or_else(|error| panic!("console entry construction: {:?}", error.code()))
}

/// Fixed RAM address for test-carved `KeyTable`s (QEMU rpi3: 1 GiB RAM at 0).
///
/// The test binary loads at `0x80000` and the DTB sits at `0x8000000`; 512 MiB
/// is clear of both. Carving from a fixed address keeps the large `KeyTable`
/// storage out of the test's stack frame.
const TEST_BACKING: u64 = 0x2000_0000;

/// Fixture-table capacity exponent: the historical 256-slot table.
const SIZE_BITS: u8 = 8;

/// The fixture tables' guard:
/// every key minted into a fixture table carries it above the slot index. It
/// fits the 24 guard bits of a 256-entry table's table-relative address.
const FIXTURE_GUARD: u32 = 0xD1A_5EE;

/// The identity of the fixture table carved at `addr`.
pub(crate) fn table_id(addr: u64) -> CarvedId {
    // SAFETY: fixture tables are carved by `carve` and stay initialized.
    unsafe { KeyTable::carve_id(addr) }
}

/// The bare slot index of a fixture key (its low `SIZE_BITS` address bits).
fn bare_index(key: RawKey) -> KeySlot {
    KeySlot(key.slot().0 & ((1_u32 << u32::from(SIZE_BITS)) - 1))
}

/// Carve a `KeyTable` into the fixed test backing at `index`, returning its
/// kernel address. A self-table capability with the fixture guard is installed
/// at the well-known slot, anchoring invocations through this table.
///
/// Implementation status: returns the stable carved-table binding used when
/// provisioning an AddressSpace, rather than a per-Thread table address.
fn carve(index: usize) -> KeyTableBinding {
    let stride = KeyTable::carve_size(SIZE_BITS);
    let obj = (TEST_BACKING + (index as u64) * (stride as u64)) as *mut u8;
    // SAFETY: TEST_BACKING is RAM, 32-byte aligned (a 512 MiB boundary), and
    // exclusively owned by the test fixture for its lifetime; the stride
    // covers the full variable-size carve.
    unsafe {
        let id = KeyTable::initialize(obj, SIZE_BITS);
        let table = &mut *KeyTable::at_carve(id.address);
        table
            .insert(
                KeySlot::SELF_KEYTABLE,
                KeyEntry::new_keytable(id, FIXTURE_GUARD, SIZE_BITS, Rights::all(), 0),
                FIXTURE_GUARD,
            )
            .unwrap_or_else(|_| panic!("fixture self-table installation failed"));
        // SAFETY: the full carve is initialized, private to this serial QEMU
        // fixture, and stays at its fixed address without relocation or
        // reclamation while any binding is live. No binding escapes the
        // fixture; subsequent tests reinitialize it only after this nucleus
        // and all its AddressSpaces have been dropped. Provisioning installs
        // the Slot(1) Return sentinel, as every AddressSpace builder must.
        table
            .bind_address_space()
            .unwrap_or_else(|error| panic!("fixture table provisioning: {:?}", error.code()))
    }
}

impl Nucleus<ArchObjectsImpl> {
    /// One-time console provisioning for a fixture AddressSpace, then Thread creation.
    /// Additional Threads in that AddressSpace use production `create_thread`.
    ///
    /// Implementation status: console authority is test-local; production Thread
    /// creation validates the AddressSpace and allocates without changing its table
    /// or selecting a current caller.
    fn create_console_thread(&mut self, address_space: AddressSpaceId) -> Option<RawKey> {
        let binding = self.address_space(address_space)?.keytable()?;
        let key = {
            // SAFETY: this live AddressSpace binding names a fixed, initialized
            // carve owned by the serial fixture. No table reference or other
            // Access context overlaps this scope, and the guard is dropped
            // before production Thread creation or subsequent dispatch.
            let access = unsafe { Access::new() };
            let mut table = access.resolve_object::<KeyTable>(binding.id()).ok()?;
            // Discover the guard through SELF, the same source used by syscall
            // entry, so the fixture installs a resolvable guarded console key.
            let (_self_addr, guard, _size_bits) = table
                .self_table_capability()
                .expect("fixture table has no self-table capability");
            table
                .insert(
                    KeySlot::DEBUG_CONSOLE,
                    console_entry(Rights::all(), 0),
                    guard,
                )
                .unwrap_or_else(|failure| {
                    panic!(
                        "bootstrap console installation failed: {:?}",
                        failure.error.code()
                    )
                })
        };
        self.create_thread(address_space)?;
        Some(key)
    }
}

/// Dispatch through the production entry signature: every input is read from
/// a synthetic saved frame, as the SVC entry passes its saved copy.
fn cap_invoke(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    key: RawKey,
    op: u64,
    args: &[u64; 6],
) -> Result<api::InvokeOutcome, CapError> {
    let mut saved = libexception::arch::aarch64::SavedContext::el1t(0x8_0000, 0x9_0000);
    saved.gpr[0] = key.to_wire();
    saved.gpr[1] = op;
    saved.gpr[2..8].copy_from_slice(args);
    api::handle_cap_invoke(nucleus, &saved)
}

/// Retire a fixture Thread (bump its generation); `false` if it was not live.
fn retire_thread(id: ThreadId) -> bool {
    // SAFETY: fixture Threads are carved test-arena records; nothing holds a
    // reference to one across this call.
    unsafe { Carved::<Thread>::retire(id) }.is_ok()
}

/// Retire a fixture `AddressSpace` (bump its generation), as
/// `AddressSpace.Retire` does after its teardown checks.
pub(crate) fn retire_address_space(id: AddressSpaceId) -> Result<(), IdentityError> {
    // SAFETY: fixture AddressSpaces are carved test-arena records; nothing
    // holds a reference to one across this call.
    unsafe { Carved::<objects::arch::AArch64AddressSpace>::retire(id) }
}

fn with_nucleus(
    test: impl FnOnce(&mut Nucleus<ArchObjectsImpl>, u64, u64, AddressSpaceId, AddressSpaceId),
) {
    // A 16-byte-aligned carve backing (the minimum pool carve alignment) for
    // the empty ASID-pool pool.
    type Asid = objects::arch::AArch64ASIDPool;
    const ASID_WORDS: usize = ObjectPool::<Asid>::carve_size(0).div_ceil(size_of::<u128>()) + 1;
    let mut asid_backing = MaybeUninit::<[u128; ASID_WORDS]>::uninit();
    const _: () = assert!(align_of::<[u128; ASID_WORDS]>() >= ObjectPool::<Asid>::ALIGN);
    // Fixture Threads are carved from the test arena; start a fresh list.
    crate::objects::nucleus::test_arena::reset_threads();
    // Carve two KeyTable regions from the fixed test backing (mirrors the boot
    // carve / runtime Retype: the table's storage is the carved region).
    let table_binding = carve(0);
    let second_table_binding = carve(1);
    let mut nucleus = Nucleus {
        pools: NucleusPools {
            // SAFETY: the ASID-pool backing is carve-aligned and exclusively
            // owned by this fixture until the nucleus is dropped; the pool has
            // zero capacity because these tests do not bind ASIDs.
            arch: unsafe {
                ArchPools::new(ObjectPool::initialize(
                    asid_backing.as_mut_ptr().cast::<u8>(),
                    0,
                ))
            },
        },
        current_thread: None,
        pending: crate::objects::PendingPool::new(),
        run_queue: crate::objects::RunQueue::new(),
        idle_thread: None,
        irq_table: crate::objects::irq::IrqTable::empty(),
        ticks: 0,
        current_since_ticks: 0,
        dispatched: None,
        dispatch_next: None,
        preempt_requested: false,
        timers: crate::objects::timers::SchedulerTimers::new(),
        interrupt_ticks: 0,
    };
    // One fixture AddressSpace shared by the fixture Threads: these tests
    // never resolve translation state.
    // Implementation status: table selection now resolves AddressSpace
    // identity. Each distinct fixture table is bound to its own AddressSpace
    // at provisioning; Threads in the same AddressSpace share its one table.
    // No translation root or hardware context is installed by this fixture.
    let fixture_as = crate::objects::nucleus::test_arena::carve(
        crate::objects::arch_objects::bound_address_space::<ArchObjectsImpl>(table_binding),
    )
    .expect("no fixture AddressSpace slot");
    let second_as = crate::objects::nucleus::test_arena::carve(
        crate::objects::arch_objects::bound_address_space::<ArchObjectsImpl>(second_table_binding),
    )
    .expect("no second fixture AddressSpace slot");
    assert_ne!(fixture_as, second_as);
    assert_ne!(table_binding.address(), second_table_binding.address());
    assert_eq!(table_binding.size_bits(), SIZE_BITS);
    assert_eq!(second_table_binding.size_bits(), SIZE_BITS);
    test(
        &mut nucleus,
        table_binding.address(),
        second_table_binding.address(),
        fixture_as,
        second_as,
    );
    for n in 0..16 {
        if let Some(id) = nucleus.fixture_thread(n) {
            assert!(retire_thread(id), "thread cleanup failed");
        }
    }
}

#[test_case]
fn missing_caller_cannot_invoke_bootstrap_console() {
    with_nucleus(|nucleus, _table_addr, _second, fixture_as, _second_as| {
        let key = nucleus
            .create_console_thread(fixture_as)
            .expect("bootstrap console key missing");
        assert_eq!(bare_index(key), KeySlot::DEBUG_CONSOLE);
        assert_ne!(key.incarnation(), 0);
        assert_eq!(nucleus.current_thread, None);
        let args = [u64::MAX; 6];

        assert!(nucleus.current_thread_mut().is_none());
        let error = match cap_invoke(nucleus, key, 1, &args) {
            Err(error) => error,
            Ok(_) => panic!("invocation without a caller succeeded"),
        };
        assert!(matches!(error, CapError::InvalidThread));
        let response = error.code();
        assert_eq!(response, (3, 0, 0));
        assert!(matches!(
            decode_syscall_result(response),
            Err(CapError::InvalidThread)
        ));

        // Explicitly selecting the existing boot fixture preserves its debug
        // path. Invalid op 1 proves dispatch without dereferencing write args.
        nucleus.current_thread = nucleus.fixture_thread(0);
        assert!(matches!(
            cap_invoke(nucleus, key, 1, &args),
            Err(CapError::InvalidOperation)
        ));
        nucleus.current_thread = None;
        assert!(matches!(
            cap_invoke(nucleus, key, 1, &args),
            Err(CapError::InvalidThread)
        ));
        // Caller validation also precedes malformed-key validation.
        assert!(matches!(
            cap_invoke(nucleus, RawKey::from_wire(0), 0, &args),
            Err(CapError::InvalidThread)
        ));
    });
}

#[test_case]
fn dispatch_uses_only_the_explicit_allocated_caller_table() {
    with_nucleus(
        |nucleus, _table_addr, _second_table_addr, fixture_as, second_as| {
            let key = nucleus
                .create_console_thread(fixture_as)
                .expect("bootstrap console key missing");
            let args = [u64::MAX; 6];
            // A null address, a misaligned one, another kind's identity
            // and a stale generation of the live console Thread all name no
            // current Thread.
            let console_thread = nucleus.fixture_thread(0).unwrap();
            for caller in [
                CarvedId {
                    address: 0,
                    ..console_thread
                },
                CarvedId {
                    address: console_thread.address + 8,
                    ..console_thread
                },
                CarvedId {
                    kind: ObjectType::NOTIFICATION,
                    ..console_thread
                },
                CarvedId {
                    generation: console_thread.generation + 1,
                    ..console_thread
                },
            ] {
                nucleus.current_thread = Some(caller);
                assert!(nucleus.current_thread_mut().is_none());
                assert!(matches!(
                    cap_invoke(nucleus, key, 1, &args),
                    Err(CapError::InvalidThread)
                ));
            }

            nucleus
                .create_thread(second_as)
                .expect("second thread allocation failed");
            nucleus.current_thread = nucleus.fixture_thread(1);
            let diag = cap_invoke(nucleus, key, 1, &args);
            let words = match diag {
                Ok(_) => panic!("second-table invocation succeeded"),
                Err(error) => error.code(),
            };
            // NeverIssued in wire form: the console key's slot was never issued in
            // the second table.
            assert_eq!(words, (27, key.to_wire(), 3));
            // The second table holds only its self-table capability and the
            // provisioned Return sentinel: the console key (same guard, same
            // slot number) was never issued there.
            assert_eq!(nucleus.current_thread_table_mut().unwrap().len(), 2);
            let id = nucleus.fixture_thread(1).expect("second Thread is live");
            assert!(retire_thread(id));
            assert!(matches!(
                cap_invoke(nucleus, key, 1, &args),
                Err(CapError::InvalidThread)
            ));
        },
    );
}

#[test_case]
fn threads_in_the_same_address_space_share_one_dispatch_table() {
    with_nucleus(|nucleus, table_addr, _second, fixture_as, _second_as| {
        let issued = nucleus
            .create_console_thread(fixture_as)
            .expect("bootstrap console key missing");
        nucleus.current_thread = nucleus.fixture_thread(0);
        let self_key = RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, KeySlot::SELF_KEYTABLE.0, 1);
        let original_console_id = {
            let table = nucleus.current_thread_table_mut().unwrap();
            assert_console_table(table, issued, Some((Rights::all(), 0)));
            table
                .lookup(issued, FIXTURE_GUARD)
                .unwrap_or_else(|_| panic!("console key missing before Thread creation"))
                .object_id()
                .unwrap_or_else(|_| panic!("console identity missing"))
        };
        // Implementation status: production creation is table-neutral even when
        // the AddressSpace already has a console grant. It must not reinstall
        // that grant, advance any slot counter, or replace either live identity.
        let second_thread = nucleus
            .create_thread(fixture_as)
            .expect("second thread allocation failed");
        assert_eq!(second_thread.kind, ObjectType::THREAD);
        assert_eq!(second_thread.generation, FIRST_GENERATION);
        assert_ne!(Some(second_thread), nucleus.fixture_thread(0));
        assert_eq!(nucleus.fixture_thread_count(), 2);
        assert_eq!(nucleus.current_thread, nucleus.fixture_thread(0));
        let first_thread = nucleus.fixture_thread(0).unwrap();
        for caller in [first_thread, second_thread] {
            nucleus.current_thread = Some(caller);
            assert_eq!(
                nucleus.current_thread_mut().unwrap().address_space,
                fixture_as
            );
            assert_eq!(nucleus.current_thread_table_addr(), Some(table_addr));
            assert_dispatch_error(nucleus, issued, 1, (9, 0, 0));
            let table = nucleus.current_thread_table_mut().unwrap();
            assert_console_table(table, issued, Some((Rights::all(), 0)));
            assert_eq!(
                table.self_table_capability(),
                Some((table_id(table_addr), FIXTURE_GUARD, SIZE_BITS))
            );
            let self_cap = table
                .lookup(self_key, FIXTURE_GUARD)
                .unwrap_or_else(|_| panic!("Thread creation changed SELF identity"));
            assert_eq!(self_cap.object_type(), ObjectType::KEY_TABLE);
            assert_eq!(self_cap.rights(), Rights::all());
            assert_eq!(self_cap.badge(), 0);
            assert_eq!(
                table
                    .lookup(issued, FIXTURE_GUARD)
                    .unwrap_or_else(|_| panic!("Thread creation changed console key"))
                    .object_id()
                    .unwrap_or_else(|_| panic!("Thread creation changed console identity")),
                original_console_id
            );
            // Exact live keys still resolve, their next incarnations mismatch,
            // and assert_console_table checks every other slot is NeverIssued:
            // production creation changed neither occupancy nor slot counters.
            for key in [self_key, issued] {
                let future = RawKey::from_parts(
                    FIXTURE_GUARD,
                    SIZE_BITS,
                    bare_index(key).0,
                    key.incarnation() + 1,
                );
                assert!(matches!(
                    table.lookup(future, FIXTURE_GUARD),
                    Err(CapError::InconsistentKey {
                        key: submitted,
                        reason: InconsistencyReason::SlotIncarnationMismatch,
                        operand: 0,
                    }) if submitted == future
                ));
            }
        }

        nucleus.current_thread = nucleus.fixture_thread(0);
        let rights = Rights(Rights::READ);
        let replacement = {
            let table = nucleus.current_thread_table_mut().unwrap();
            table
                .remove(issued, FIXTURE_GUARD)
                .unwrap_or_else(|_| panic!("shared console removal failed"));
            table
                .insert(bare_index(issued), console_entry(rights, 91), FIXTURE_GUARD)
                .unwrap_or_else(|_| panic!("shared console replacement failed"))
        };
        assert_eq!(replacement.incarnation(), issued.incarnation() + 1);
        let first_thread = nucleus.fixture_thread(0).unwrap();
        for caller in [first_thread, second_thread] {
            nucleus.current_thread = Some(caller);
            assert_eq!(nucleus.current_thread_table_addr(), Some(table_addr));
            assert_dispatch_error(nucleus, issued, 0, (28, issued.to_wire(), 1));
            assert_dispatch_error(nucleus, replacement, 1, (9, 0, 0));
            assert_console_table(
                nucleus.current_thread_table_mut().unwrap(),
                replacement,
                Some((rights, 91)),
            );
        }
    });
}

#[test_case]
fn thread_return_selectors_reject_without_mutating_shared_threads_or_pending() {
    with_nucleus(|nucleus, table_addr, _second, fixture_as, _second_as| {
        let first = nucleus.create_thread(fixture_as).expect("first Thread");
        let second = nucleus.create_thread(fixture_as).expect("second Thread");
        let waiting = nucleus
            .pending
            .block(
                first,
                PendingKind::NotificationWait,
                crate::objects::carved::test_id(libobject::ObjectType::NOTIFICATION, 0),
            )
            .unwrap_or_else(|error| panic!("waiting record: {:?}", error.code()));
        let completed = nucleus
            .pending
            .block(
                second,
                PendingKind::EventCountAwait,
                crate::objects::carved::test_id(libobject::ObjectType::EVENT_COUNT, 0),
            )
            .unwrap_or_else(|error| panic!("completed record: {:?}", error.code()));
        nucleus
            .pending
            .complete_with_status(completed, 31, 0x1234, 0x5678)
            .unwrap_or_else(|error| panic!("completion: {:?}", error.code()));
        let terminal = nucleus
            .pending
            .state(completed)
            .unwrap_or_else(|error| panic!("terminal record snapshot: {:?}", error.code()));
        let mut saved = libexception::arch::aarch64::SavedContext::el1t(0x80000, 0x90000);
        saved.gpr = [0xA5A5; 30];
        saved.lr = 0xBEEF;
        saved.spsr_el1 |= 0xA000_0000;
        let contexts = [
            ExecutionContext::Running,
            ExecutionContext::Parked {
                saved,
                record: completed,
            },
        ];
        nucleus.thread_mut(second).unwrap().context = contexts[1];
        assert!(nucleus.run_queue.push(second));
        assert!(nucleus.run_queue.push(first));
        nucleus.current_thread = Some(first);

        let stale = CarvedId {
            generation: u32::MAX,
            ..second
        };
        // Vacate the provisioned Slot(1) sentinel (a component may delete its
        // own) so each row below can install its own Slot(1) variant.
        nucleus
            .current_thread_table_mut()
            .unwrap()
            .remove(
                RawKey::from_parts(
                    FIXTURE_GUARD,
                    SIZE_BITS,
                    KeySlot::THREAD_RETURN.0,
                    KeySlot::THREAD_RETURN_INCARNATION,
                ),
                FIXTURE_GUARD,
            )
            .unwrap_or_else(|error| panic!("provisioned sentinel removal: {:?}", error.code()));
        // Named Return rejection is independent of management rights and object
        // liveness. Current-relative management must reject before either check.
        for (slot, entry, last_op) in [
            (
                KeySlot(10),
                KeyEntry::new_thread(second, Rights::all(), 0xBEEF),
                0,
            ),
            (
                KeySlot(11),
                KeyEntry::new_thread(stale, Rights::empty(), 0),
                0,
            ),
            (KeySlot::THREAD_RETURN, KeyEntry::new_thread_return(), 4),
            (
                KeySlot::THREAD_RETURN,
                KeyEntry::new_thread_return().derive(Rights::all()),
                4,
            ),
        ] {
            let key = nucleus
                .current_thread_table_mut()
                .unwrap()
                .insert(slot, entry, FIXTURE_GUARD)
                .unwrap_or_else(|_| panic!("Thread capability installation"));
            for caller in [first, second] {
                nucleus.current_thread = Some(caller);
                assert_eq!(nucleus.current_thread_table_addr(), Some(table_addr));
                for op in 0..=last_op {
                    for direct in [true, false] {
                        // Dispatched Return on the sentinel by the running
                        // caller at depth zero is an illegal-return fault,
                        // which the interim policy turns into a kernel halt.
                        // The parked caller is still rejected before that.
                        if op == 0 && !direct && slot == KeySlot::THREAD_RETURN && caller == first {
                            continue;
                        }
                        // Zero operands are essential: Retire must reach the
                        // selector check, not reject nonzero reserved arguments.
                        let error = if direct {
                            // SAFETY: the serial fixture owns exclusive nucleus
                            // access; no Access or table/object guard overlaps.
                            let access = unsafe { Access::new() };
                            match api::thread::invoke(
                                &access,
                                CallerTable {
                                    table: table_id(table_addr),
                                    guard: FIXTURE_GUARD,
                                },
                                key,
                                op,
                                &[0; 6],
                                nucleus,
                            ) {
                                Err(error) => error,
                                Ok(_) => panic!("Thread API unexpectedly succeeded"),
                            }
                        } else {
                            match cap_invoke(nucleus, key, op, &[0; 6]) {
                                Err(error) => error,
                                Ok(_) => panic!("Thread dispatch unexpectedly succeeded"),
                            }
                        };
                        assert!(matches!(error, CapError::InvalidOperation));
                        assert_eq!(error.code(), (9, 0, 0));
                        assert_eq!(nucleus.current_thread, Some(caller));
                        assert_eq!(nucleus.current_thread_table_addr(), Some(table_addr));
                        assert_eq!(nucleus.fixture_thread_count(), 2);
                        for (id, context) in [first, second].into_iter().zip(contexts) {
                            assert!(nucleus.thread(id).is_some());
                            let thread = nucleus.thread(id).unwrap();
                            assert_eq!(thread.address_space, fixture_as);
                            assert_eq!(thread.context, context);
                        }
                        assert!(nucleus.address_space(fixture_as).is_some());
                        assert_eq!(nucleus.pending.len(), 2);
                        for (record, waiter, kind, state) in [
                            (
                                waiting,
                                first,
                                PendingKind::NotificationWait,
                                PendingState::Waiting,
                            ),
                            (completed, second, PendingKind::EventCountAwait, terminal),
                        ] {
                            assert_eq!(
                                nucleus.pending.waiter(record).unwrap_or_else(|error| {
                                    panic!("pending waiter preserved: {:?}", error.code())
                                }),
                                waiter
                            );
                            assert_eq!(
                                nucleus.pending.kind(record).unwrap_or_else(|error| {
                                    panic!("pending kind preserved: {:?}", error.code())
                                }),
                                kind
                            );
                            assert_eq!(
                                nucleus.pending.state(record).unwrap_or_else(|error| {
                                    panic!("pending state preserved: {:?}", error.code())
                                }),
                                state
                            );
                        }
                        assert_eq!(nucleus.run_queue.len(), 2);
                        assert_eq!(nucleus.run_queue.peek(), Some(second));
                        let table = nucleus.current_thread_table_mut().unwrap();
                        assert_eq!(table.len(), 2);
                        assert_eq!(
                            table.self_table_capability(),
                            Some((table_id(table_addr), FIXTURE_GUARD, SIZE_BITS))
                        );
                        let retained = table.lookup(key, FIXTURE_GUARD).unwrap_or_else(|error| {
                            panic!("Thread key preserved: {:?}", error.code())
                        });
                        assert_eq!(
                            retained.thread_selector().unwrap_or_else(|error| {
                                panic!("retained Thread selector: {:?}", error.code())
                            }),
                            entry.thread_selector().unwrap_or_else(|error| {
                                panic!("original Thread selector: {:?}", error.code())
                            })
                        );
                        assert_eq!(retained.rights(), entry.rights());
                        assert_eq!(retained.badge(), entry.badge());
                        if entry.is_thread_return_key() {
                            assert!(matches!(
                                retained.thread_id(),
                                Err(CapError::InvalidOperation)
                            ));
                        }
                    }
                }
            }
            nucleus
                .current_thread_table_mut()
                .unwrap()
                .remove(key, FIXTURE_GUARD)
                .unwrap_or_else(|_| panic!("Thread capability cleanup"));
        }
        assert_eq!(nucleus.run_queue.pop(), Some(second));
        assert_eq!(nucleus.run_queue.pop(), Some(first));
        assert!(nucleus.run_queue.is_empty());
    });
}

#[test_case]
fn thread_return_dispatch_requires_checked_packed_key_and_entry_presence() {
    with_nucleus(
        |nucleus, table_addr, second_table_addr, fixture_as, second_as| {
            let first = nucleus.create_thread(fixture_as).expect("first Thread");
            let second = nucleus.create_thread(fixture_as).expect("second Thread");
            let foreign = nucleus.create_thread(second_as).expect("foreign Thread");
            nucleus.current_thread = Some(first);
            let issued = RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, KeySlot::THREAD_RETURN.0, 1);
            let replacement =
                RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, KeySlot::THREAD_RETURN.0, 2);
            let zero = RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, KeySlot::THREAD_RETURN.0, 0);
            let bare = RawKey::from_wire(u64::from(KeySlot::THREAD_RETURN.0));
            let unguarded = RawKey::from_parts(0, SIZE_BITS, KeySlot::THREAD_RETURN.0, 1);
            let wrong_guard =
                RawKey::from_parts(FIXTURE_GUARD ^ 1, SIZE_BITS, KeySlot::THREAD_RETURN.0, 1);
            // Provisioned live, deleted, then replaced: Slot(1) has no special
            // lookup bypass, and a stale key never gains the replacement's
            // authority. A provisioned table can no longer have a never-issued
            // Slot(1): `bind_address_space` installed `issued` already.
            for phase in 1..4 {
                match phase {
                    3 => {
                        let key = nucleus
                            .current_thread_table_mut()
                            .unwrap()
                            .insert(
                                KeySlot::THREAD_RETURN,
                                KeyEntry::new_thread_return(),
                                FIXTURE_GUARD,
                            )
                            .unwrap_or_else(|_| panic!("Return capability installation"));
                        assert_eq!(key, replacement);
                    }
                    2 => {
                        nucleus
                            .current_thread_table_mut()
                            .unwrap()
                            .remove(issued, FIXTURE_GUARD)
                            .unwrap_or_else(|_| panic!("Return capability removal"));
                    }
                    _ => {}
                }
                let (old_words, next_words, live) = match phase {
                    1 => ((9, 0, 0), (28, replacement.to_wire(), 1), Some(issued)),
                    2 => (
                        (28, issued.to_wire(), 2),
                        (28, replacement.to_wire(), 1),
                        None,
                    ),
                    3 => ((28, issued.to_wire(), 1), (9, 0, 0), Some(replacement)),
                    _ => unreachable!(),
                };
                for caller in [first, second] {
                    nucleus.current_thread = Some(caller);
                    for (key, words) in [
                        (zero, (27, zero.to_wire(), 1)),
                        (bare, (27, bare.to_wire(), 1)),
                        (unguarded, (27, unguarded.to_wire(), 4)),
                        (wrong_guard, (27, wrong_guard.to_wire(), 4)),
                        (issued, old_words),
                        (replacement, next_words),
                    ] {
                        for op in [0, 4] {
                            // Return through a live sentinel at depth zero is
                            // an illegal-return fault: the interim policy halts
                            // the kernel. Its classification is tested on the
                            // primitive; Retire still checks the form here.
                            if op == 0 && Some(key) == live {
                                continue;
                            }
                            assert_dispatch_error(nucleus, key, op, words);
                            assert_eq!(nucleus.current_thread, Some(caller));
                            assert_eq!(nucleus.current_thread_table_addr(), Some(table_addr));
                            assert_eq!(nucleus.fixture_thread_count(), 3);
                            for (id, address_space) in [
                                (first, fixture_as),
                                (second, fixture_as),
                                (foreign, second_as),
                            ] {
                                assert!(nucleus.thread(id).is_some());
                                let thread = nucleus.thread(id).unwrap();
                                assert_eq!(thread.address_space, address_space);
                                assert_eq!(thread.context, ExecutionContext::Running);
                            }
                            assert!(nucleus.pending.is_empty());
                            assert!(nucleus.run_queue.is_empty());
                            let table = nucleus.current_thread_table_mut().unwrap();
                            assert_eq!(table.len(), 1 + usize::from(live.is_some()));
                            assert_eq!(
                                table.self_table_capability(),
                                Some((table_id(table_addr), FIXTURE_GUARD, SIZE_BITS))
                            );
                            if let Some(live) = live {
                                let entry =
                                    table.lookup(live, FIXTURE_GUARD).unwrap_or_else(|error| {
                                        panic!("Return key preserved: {:?}", error.code())
                                    });
                                assert!(matches!(
                                    entry.thread_selector(),
                                    Ok(ThreadSelector::CurrentReturnOnly)
                                ));
                                assert_eq!(entry.rights(), Rights::empty());
                                assert_eq!(entry.badge(), 0);
                            }
                        }
                    }
                }
                // The foreign AddressSpace's table was provisioned with its own
                // sentinel under the same packed key. Retire reaches only the
                // form check there (Return at depth zero would halt by policy),
                // and the first table's replacement never authorizes it.
                nucleus.current_thread = Some(foreign);
                assert_eq!(nucleus.current_thread_table_addr(), Some(second_table_addr));
                assert_dispatch_error(nucleus, issued, 4, (9, 0, 0));
                assert_dispatch_error(nucleus, replacement, 0, (28, replacement.to_wire(), 1));
                assert_eq!(nucleus.current_thread_table_mut().unwrap().len(), 2);
                nucleus.current_thread = Some(first);
            }
        },
    );
}

#[test_case]
fn rejected_invocation_dispatch_preserves_the_mandatory_target_and_caller() {
    with_nucleus(|nucleus, table_addr, _second, fixture_as, second_as| {
        let thread = nucleus.create_thread(fixture_as).expect("caller Thread");
        nucleus.current_thread = Some(thread);
        let function = NonZero::new(0x80000).unwrap();
        let extent = InvocationStackExtent::new(0x1010, 0x1080, 48, ArchObjectsImpl::USER_VA_END)
            .unwrap_or_else(|error| panic!("fixture stack extent: {:?}", error.code()));
        let key = nucleus
            .current_thread_table_mut()
            .unwrap()
            .insert(
                KeySlot(10),
                KeyEntry::new_invocation(second_as, function, extent, PlacementWindow::NONE),
                FIXTURE_GUARD,
            )
            .unwrap_or_else(|_| panic!("Invocation capability installation"));
        // Call dispatch is live. A zero saved x9 is an invalid SP, reported
        // before the unready target translation; op 1 is CallWithKey, which this
        // Invocation (empty placement window) rejects, and op 2 is unassigned.
        for (op, expected) in [
            (
                0,
                CapError::InvalidStack {
                    value: 0,
                    reason: libobject::InvalidStackReason::SpOutOfRange,
                }
                .code(),
            ),
            (1, CapError::KeyNotAccepted.code()),
            (2, CapError::InvalidOperation.code()),
        ] {
            match cap_invoke(nucleus, key, op, &[0; 6]) {
                Err(error) => assert_eq!(error.code(), expected),
                Ok(_) => panic!("Invocation op {op} succeeded without a valid Call"),
            }
            assert!(nucleus.thread(thread).unwrap().invocation_stack.is_empty());
            assert_eq!(nucleus.current_thread, Some(thread));
            assert_eq!(nucleus.current_thread_table_addr(), Some(table_addr));
            assert_eq!(nucleus.fixture_thread_count(), 1);
            let retained = nucleus.thread(thread).unwrap();
            assert_eq!(retained.address_space, fixture_as);
            assert_eq!(retained.context, ExecutionContext::Running);
            assert!(nucleus.pending.is_empty());
            assert!(nucleus.run_queue.is_empty());
            let table = nucleus.current_thread_table_mut().unwrap();
            assert_eq!(table.len(), 3);
            let entry = table
                .lookup(key, FIXTURE_GUARD)
                .unwrap_or_else(|error| panic!("Invocation key preserved: {:?}", error.code()));
            assert!(matches!(
                entry.invocation_target(),
                Ok(target) if target == (second_as, function)
            ));
            assert_eq!(
                entry
                    .invocation_stack_extent()
                    .unwrap_or_else(|error| panic!(
                        "Invocation extent preserved: {:?}",
                        error.code()
                    )),
                extent
            );
            assert_eq!(entry.rights(), Rights(Rights::CALL));
            assert_eq!(entry.badge(), 0);
            assert!(!entry.is_thread_return_key());
        }
    });
}

#[test_case]
fn changing_thread_address_space_selects_its_table_on_the_next_dispatch() {
    with_nucleus(
        |nucleus, table_addr, second_table_addr, fixture_as, second_as| {
            let issued = nucleus
                .create_console_thread(fixture_as)
                .expect("bootstrap console key missing");
            nucleus.current_thread = nucleus.fixture_thread(0);
            assert_dispatch_error(nucleus, issued, 1, (9, 0, 0));
            assert_eq!(nucleus.current_thread_table_addr(), Some(table_addr));

            // This is a private-state fixture transition, not PPC or a hardware
            // migration: no translation context or saved registers are switched.
            nucleus.current_thread_mut().unwrap().address_space = second_as;
            assert_eq!(nucleus.current_thread_table_addr(), Some(second_table_addr));
            assert_dispatch_error(nucleus, issued, 0, (27, issued.to_wire(), 3));
            assert_eq!(nucleus.current_thread_table_mut().unwrap().len(), 2);
            let rights = Rights(Rights::READ);
            let second_key = nucleus
                .current_thread_table_mut()
                .unwrap()
                .insert(
                    KeySlot::DEBUG_CONSOLE,
                    console_entry(rights, 92),
                    FIXTURE_GUARD,
                )
                .unwrap_or_else(|_| panic!("second-table console installation failed"));
            // These fixture tables deliberately use the same guard: differing
            // table contents, not differing key bits, prove the table selection.
            assert_eq!(second_key, issued);
            assert_dispatch_error(nucleus, second_key, 1, (9, 0, 0));
            assert_console_table(
                nucleus.current_thread_table_mut().unwrap(),
                second_key,
                Some((rights, 92)),
            );

            nucleus.current_thread_mut().unwrap().address_space = fixture_as;
            assert_eq!(nucleus.current_thread_table_addr(), Some(table_addr));
            assert_dispatch_error(nucleus, issued, 1, (9, 0, 0));
            assert_console_table(
                nucleus.current_thread_table_mut().unwrap(),
                issued,
                Some((Rights::all(), 0)),
            );
            for (id, address) in [(fixture_as, table_addr), (second_as, second_table_addr)] {
                assert!(nucleus.address_space(id).is_some());
                let binding = nucleus.address_space(id).unwrap().keytable().unwrap();
                assert_eq!(binding.address(), address);
                assert_eq!(binding.size_bits(), SIZE_BITS);
            }
        },
    );
}

#[test_case]
fn stale_address_space_is_rejected_before_self_table_or_key_lookup() {
    with_nucleus(
        |nucleus, _table_addr, second_table_addr, fixture_as, second_as| {
            let issued = nucleus
                .create_console_thread(fixture_as)
                .expect("bootstrap console key missing");
            nucleus.current_thread = nucleus.fixture_thread(0);
            let self_key =
                RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, KeySlot::SELF_KEYTABLE.0, 1);
            nucleus
                .current_thread_table_mut()
                .unwrap()
                .remove(self_key, FIXTURE_GUARD)
                .unwrap_or_else(|_| panic!("self-table removal failed"));
            // A live AddressSpace reaches the missing SELF rejection, even for a
            // malformed key; stale AddressSpace validation must win instead.
            // A stale caller AddressSpace reports InvalidAddressSpace (status
            // 4), before SELF or key lookup, not an object-retirement status.
            let malformed = RawKey::from_wire(0);
            assert_dispatch_error(nucleus, malformed, 0, (41, 4, 0));
            assert!(crate::retire_address_space(fixture_as).is_ok());
            assert_eq!(
                nucleus.current_thread_mut().unwrap().address_space,
                fixture_as
            );
            assert!(nucleus.current_thread_table_addr().is_none());
            assert!(nucleus.current_thread_table_mut().is_none());
            for key in [malformed, issued] {
                assert_dispatch_error(nucleus, key, 0, (4, 0, 0));
            }

            let replacement_binding = nucleus
                .address_space(second_as)
                .unwrap()
                .keytable()
                .unwrap();
            assert!(crate::retire_address_space(second_as).is_ok());
            let replacement_as = crate::objects::nucleus::test_arena::carve(
                crate::objects::arch_objects::bound_address_space::<ArchObjectsImpl>(
                    replacement_binding,
                ),
            )
            .expect("replacement AddressSpace allocation failed");
            assert_ne!(replacement_as, fixture_as);
            assert_eq!(replacement_as.generation, FIRST_GENERATION);
            assert!(nucleus.address_space(replacement_as).is_some());
            assert!(nucleus.address_space(fixture_as).is_none());
            assert!(nucleus.current_thread_table_addr().is_none());
            assert!(nucleus.current_thread_table_mut().is_none());
            for key in [malformed, issued] {
                assert_dispatch_error(nucleus, key, 0, (4, 0, 0));
            }
            assert_eq!(nucleus.fixture_thread_count(), 1);

            // Only the fresh identity admits access to the replacement's live
            // table. Neither reuse nor the stale Thread silently refreshes it.
            nucleus.current_thread_mut().unwrap().address_space = replacement_as;
            assert_eq!(nucleus.current_thread_table_addr(), Some(second_table_addr));
            assert_dispatch_error(nucleus, malformed, 0, (27, 0, 1));
            assert_dispatch_error(nucleus, issued, 0, (27, issued.to_wire(), 3));
            assert_eq!(nucleus.current_thread_table_mut().unwrap().len(), 2);
        },
    );
}

#[test_case]
fn fixture_thread_creation_rejects_stale_address_space_without_allocating() {
    with_nucleus(|nucleus, table_addr, _second, fixture_as, _second_as| {
        let binding = nucleus
            .address_space(fixture_as)
            .unwrap()
            .keytable()
            .unwrap();
        assert_eq!(nucleus.fixture_thread_count(), 0);
        assert!(crate::retire_address_space(fixture_as).is_ok());
        // These rejection checks exercise production creation, not the console
        // provisioning helper: a stale AddressSpace must consume no Thread slot.
        assert!(nucleus.create_thread(fixture_as).is_none());
        assert_eq!(nucleus.fixture_thread_count(), 0);
        assert_eq!(nucleus.current_thread, None);

        let replacement_as = crate::objects::nucleus::test_arena::carve(
            crate::objects::arch_objects::bound_address_space::<ArchObjectsImpl>(binding),
        )
        .expect("replacement AddressSpace allocation failed");
        assert_ne!(replacement_as, fixture_as);
        assert_eq!(replacement_as.generation, FIRST_GENERATION);
        assert!(nucleus.create_thread(fixture_as).is_none());
        assert_eq!(nucleus.fixture_thread_count(), 0);
        assert_eq!(nucleus.current_thread, None);

        let issued = nucleus
            .create_console_thread(replacement_as)
            .expect("live replacement AddressSpace rejected");
        assert_eq!(nucleus.fixture_thread_count(), 1);
        // The first Thread has the first generation: rejected creation
        // did not allocate a Thread even temporarily, or consume its identity.
        assert_eq!(
            nucleus.fixture_thread(0).map(|thread| thread.generation),
            Some(FIRST_GENERATION)
        );
        assert_eq!(issued.incarnation(), 1);
        nucleus.current_thread = nucleus.fixture_thread(0);
        assert_eq!(
            nucleus.current_thread_mut().unwrap().address_space,
            replacement_as
        );
        assert_eq!(nucleus.current_thread_table_addr(), Some(table_addr));
        assert_dispatch_error(nucleus, issued, 1, (9, 0, 0));
        assert_console_table(
            nucleus.current_thread_table_mut().unwrap(),
            issued,
            Some((Rights::all(), 0)),
        );
    });
}

#[test_case]
fn mismatched_self_table_address_or_size_rejects_dispatch_without_changes() {
    with_nucleus(
        |nucleus, table_addr, second_table_addr, fixture_as, _second_as| {
            let issued = nucleus
                .create_console_thread(fixture_as)
                .expect("bootstrap console key missing");
            nucleus.current_thread = nucleus.fixture_thread(0);
            let mut self_key =
                RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, KeySlot::SELF_KEYTABLE.0, 1);
            for (address, size_bits) in [
                (second_table_addr, SIZE_BITS),
                (table_addr, SIZE_BITS - 1),
                (table_addr, SIZE_BITS + 1),
            ] {
                let original = nucleus
                    .current_thread_table_mut()
                    .unwrap()
                    .remove(self_key, FIXTURE_GUARD)
                    .unwrap_or_else(|_| panic!("self-table removal failed"));
                self_key = nucleus
                    .current_thread_table_mut()
                    .unwrap()
                    .insert(
                        KeySlot::SELF_KEYTABLE,
                        KeyEntry::new_keytable(
                            table_id(address),
                            FIXTURE_GUARD,
                            size_bits,
                            Rights::all(),
                            93,
                        ),
                        FIXTURE_GUARD,
                    )
                    .unwrap_or_else(|_| panic!("mismatched self-table installation failed"));
                for key in [issued, RawKey::from_wire(0), RawKey::from_wire(u64::MAX)] {
                    for op in [0, u64::MAX] {
                        assert_dispatch_error(nucleus, key, op, (41, 4, key.to_wire()));
                        let table = nucleus.current_thread_table_mut().unwrap();
                        assert_console_table(table, issued, Some((Rights::all(), 0)));
                        assert_eq!(table.size_bits(), SIZE_BITS);
                        assert_eq!(
                            table.self_table_capability(),
                            Some((table_id(address), FIXTURE_GUARD, size_bits))
                        );
                        let cap = table
                            .lookup(self_key, FIXTURE_GUARD)
                            .unwrap_or_else(|_| panic!("rejected dispatch changed SELF identity"));
                        assert_eq!(
                            cap.keytable_id().unwrap_or_else(|_| panic!("not a table")),
                            table_id(address)
                        );
                        assert_eq!(
                            cap.keytable_guard_and_size()
                                .unwrap_or_else(|_| panic!("not a table")),
                            (FIXTURE_GUARD, size_bits)
                        );
                        assert_eq!(cap.rights(), Rights::all());
                        assert_eq!(cap.badge(), 93);
                        let future = RawKey::from_parts(
                            FIXTURE_GUARD,
                            SIZE_BITS,
                            KeySlot::SELF_KEYTABLE.0,
                            self_key.incarnation() + 1,
                        );
                        let error = match table.lookup(future, FIXTURE_GUARD) {
                            Err(error) => error,
                            Ok(_) => panic!("rejected dispatch changed SELF incarnation"),
                        };
                        assert_eq!(error.code(), (28, future.to_wire(), 1));
                    }
                }
                nucleus
                    .current_thread_table_mut()
                    .unwrap()
                    .remove(self_key, FIXTURE_GUARD)
                    .unwrap_or_else(|_| panic!("mismatched self-table removal failed"));
                self_key = nucleus
                    .current_thread_table_mut()
                    .unwrap()
                    .insert(KeySlot::SELF_KEYTABLE, original, FIXTURE_GUARD)
                    .unwrap_or_else(|_| panic!("self-table restoration failed"));
                assert_dispatch_error(nucleus, issued, 1, (9, 0, 0));
            }
        },
    );
}

#[test_case]
fn self_table_capability_remains_the_source_of_the_caller_guard() {
    with_nucleus(|nucleus, table_addr, _second, fixture_as, _second_as| {
        let issued = nucleus
            .create_console_thread(fixture_as)
            .expect("bootstrap console key missing");
        nucleus.current_thread = nucleus.fixture_thread(0);
        let self_key = RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, KeySlot::SELF_KEYTABLE.0, 1);
        let changed_guard = FIXTURE_GUARD ^ 1;
        // Deliberately replace private fixture metadata to test the lookup
        // source; this is not a public guard-mutation or table-rebinding API.
        let table = nucleus.current_thread_table_mut().unwrap();
        table
            .remove(self_key, FIXTURE_GUARD)
            .unwrap_or_else(|_| panic!("self-table removal failed"));
        table
            .insert(
                KeySlot::SELF_KEYTABLE,
                KeyEntry::new_keytable(
                    table_id(table_addr),
                    changed_guard,
                    SIZE_BITS,
                    Rights::all(),
                    0,
                ),
                changed_guard,
            )
            .unwrap_or_else(|_| panic!("changed-guard self-table installation failed"));
        let rebound_key = RawKey::from_parts(
            changed_guard,
            SIZE_BITS,
            KeySlot::DEBUG_CONSOLE.0,
            issued.incarnation(),
        );
        assert_dispatch_error(nucleus, issued, 0, (27, issued.to_wire(), 4));
        assert_dispatch_error(nucleus, rebound_key, 1, (9, 0, 0));
        let table = nucleus.current_thread_table_mut().unwrap();
        assert_eq!(table.len(), 3);
        assert_eq!(
            table.self_table_capability(),
            Some((table_id(table_addr), changed_guard, SIZE_BITS))
        );
        let cap = table
            .lookup(rebound_key, changed_guard)
            .unwrap_or_else(|_| panic!("console metadata changed"));
        assert_eq!(cap.object_type(), ObjectType::DEBUG_CONSOLE);
        assert_eq!(cap.rights(), Rights::all());
        assert_eq!(cap.badge(), 0);
    });
}

/// An `AddressSpace` whose bound table is retired can invoke nothing: syscall
/// entry rejects every invocation with `InconsistentKey`/`ObjectRetired` on
/// the invoked key, before dispatch, and the other `AddressSpace` is
/// unaffected.
#[test_case]
fn retired_bound_table_rejects_every_invocation_from_its_address_space() {
    with_nucleus(|nucleus, table_addr, _second, fixture_as, second_as| {
        let issued = nucleus
            .create_console_thread(fixture_as)
            .expect("bootstrap console key missing");
        let second_issued = nucleus
            .create_console_thread(second_as)
            .expect("second console key missing");
        nucleus.current_thread = nucleus.fixture_thread(0);
        // SAFETY: the fixture table is a live carve; no guard to it is held.
        unsafe { Carved::<KeyTable>::retire(table_id(table_addr)) }
            .unwrap_or_else(|_| panic!("table retirement failed"));
        assert!(nucleus.current_thread_table_mut().is_none());
        for key in [issued, RawKey::from_wire(0), RawKey::from_wire(u64::MAX)] {
            for op in [0, 1] {
                assert_dispatch_error(
                    nucleus,
                    key,
                    op,
                    (28, key.to_wire(), InconsistencyReason::ObjectRetired as u64),
                );
            }
        }
        nucleus.current_thread = nucleus.fixture_thread(1);
        assert!(nucleus.current_thread_table_mut().is_some());
        // Entry admits the call; the console rejects the undefined op itself.
        assert_dispatch_error(nucleus, second_issued, 1, (9, 0, 0));
    });
}

#[test_case]
fn rejects_wrong_capability_types_before_touching_write_arguments() {
    for cap in [
        KeyEntry::null(),
        KeyEntry::new_untyped(0, 12, false, Rights::all()),
        KeyEntry::new_frame(0, 12, false, Rights::all()),
    ] {
        for op in [0, u64::from(u32::MAX), u64::MAX] {
            assert!(matches!(
                invoke(&cap, op, u64::MAX, u64::MAX),
                Err(CapError::TypeMismatch { expected, found })
                    if expected == ObjectType::DEBUG_CONSOLE && found == cap.object_type()
            ));
        }
    }
}

#[test_case]
fn rejects_invalid_operations_through_shared_capability_borrows() {
    let cap = console_entry(Rights::all(), 42);
    let alias = &cap;

    // Invalid opcodes must fail before constructing an address or copying bytes.
    for op in [1, 127, 255, 256, 1 << 16, u64::from(u32::MAX), u64::MAX]
        .into_iter()
        .chain((0..64).map(|bit| 1_u64 << bit))
    {
        assert!(matches!(
            invoke(&cap, op, u64::MAX, u64::MAX),
            Err(CapError::InvalidOperation)
        ));
        assert!(matches!(
            invoke(alias, op, u64::MAX, u64::MAX),
            Err(CapError::InvalidOperation)
        ));
    }
    assert_eq!(cap.object_type(), ObjectType::DEBUG_CONSOLE);
    assert_eq!(cap.rights(), Rights::all());
    assert_eq!(cap.badge(), 42);
    assert_eq!(
        cap.object_id()
            .unwrap_or_else(|_| panic!("no identity"))
            .generation,
        0
    );
}

// Check every slot through the public API, including retained identity on deletion.
// Only console metadata is inspected; no object pointer or write buffer is accessed.
fn assert_console_table(table: &KeyTable, key: RawKey, live: Option<(Rights, u16)>) {
    // The fixture's self-table capability and provisioned Return sentinel
    // are always present alongside the console entry under test.
    assert_eq!(table.len(), 2 + usize::from(live.is_some()));
    match live {
        Some((rights, badge)) => {
            let cap = table
                .lookup(key, FIXTURE_GUARD)
                .unwrap_or_else(|_| panic!("console key changed"));
            assert_eq!(cap.object_type(), ObjectType::DEBUG_CONSOLE);
            assert_eq!(cap.rights(), rights);
            assert_eq!(cap.badge(), badge);
            assert_eq!(
                cap.object_id()
                    .unwrap_or_else(|_| panic!("no identity"))
                    .generation,
                0
            );
        }
        None => assert!(matches!(
            table.lookup(key, FIXTURE_GUARD),
            Err(CapError::InconsistentKey {
                key: submitted,
                reason: InconsistencyReason::CapabilityInvalidated,
                operand: 0,
            }) if submitted == key
        )),
    }
    let console_index = bare_index(key).0;
    let self_index = KeySlot::SELF_KEYTABLE.0;
    let return_index = KeySlot::THREAD_RETURN.0;
    for index in 0..KeyTable::capacity_for(SIZE_BITS) {
        // Skip the console slot, the fixture's self-table slot and the
        // provisioned Return sentinel (all issued); every other correctly
        // guarded probe is never issued.
        let index_u32 = u32::try_from(index).unwrap();
        if index_u32 != console_index && index_u32 != self_index && index_u32 != return_index {
            let probe = RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, index as u32, 1);
            assert!(matches!(
                table.lookup(probe, FIXTURE_GUARD),
                Err(CapError::InvalidKey {
                    key: submitted,
                    reason: InvalidKeyReason::NeverIssued,
                    operand: 0,
                }) if submitted == probe
            ));
        }
    }
}

fn assert_dispatch_error(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    key: RawKey,
    op: u64,
    expected: (u64, u64, u64),
) {
    // An erroneous Write dispatch must not get as far as copying these arguments.
    let error = match cap_invoke(nucleus, key, op, &[u64::MAX; 6]) {
        Err(error) => error,
        Ok(_) => panic!("rejected invocation succeeded"),
    };
    assert_eq!(error.code(), expected);
}

#[test_case]
fn malformed_dispatch_keys_encode_literal_error_words_without_changing_table() {
    with_nucleus(|nucleus, _table_addr, _second, fixture_as, _second_as| {
        let issued = nucleus
            .create_console_thread(fixture_as)
            .expect("bootstrap console key missing");
        nucleus.current_thread = nucleus.fixture_thread(0);
        // Literal wire words deliberately avoid deriving expectations from the
        // encoder under test. Zero incarnation wins even over a wrong guard;
        // with a nonzero table guard, every address whose guard bits do not
        // match rejects as GuardMismatch before indexing.
        for (wire, reason, details) in [
            (0x0000_0000_0000_0000, InvalidKeyReason::ZeroIncarnation, 1),
            (0x0000_0000_0000_007f, InvalidKeyReason::ZeroIncarnation, 1),
            (0x0000_0000_ffff_ffff, InvalidKeyReason::ZeroIncarnation, 1),
            (0x0000_0001_ffff_ffff, InvalidKeyReason::GuardMismatch, 4),
            (0x0000_0001_8000_007f, InvalidKeyReason::GuardMismatch, 4),
            (0xffff_ffff_ffff_ffff, InvalidKeyReason::GuardMismatch, 4),
            (0x0000_0001_0000_0000, InvalidKeyReason::GuardMismatch, 4),
            (0xffff_ffff_0000_0000, InvalidKeyReason::GuardMismatch, 4),
        ] {
            let key = RawKey::from_wire(wire);
            let words = (27, wire, details);
            for op in [0, u64::MAX] {
                assert_dispatch_error(nucleus, key, op, words);
                assert_console_table(
                    nucleus.current_thread_table_mut().unwrap(),
                    issued,
                    Some((Rights::all(), 0)),
                );
            }
            assert!(matches!(
                decode_syscall_result(words),
                Err(CapError::InvalidKey {
                    key: submitted,
                    reason: decoded,
                    operand: 0,
                }) if submitted == key && decoded == reason
            ));
        }
        // A correctly guarded key at a never-issued slot keeps its own
        // diagnostic: the guard matches and the slot was never issued.
        let never = RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, 200, 1);
        let words = (27, never.to_wire(), 3);
        for op in [0, u64::MAX] {
            assert_dispatch_error(nucleus, never, op, words);
        }
        assert!(matches!(
            decode_syscall_result(words),
            Err(CapError::InvalidKey {
                key: submitted,
                reason: InvalidKeyReason::NeverIssued,
                operand: 0,
            }) if submitted == never
        ));
    });
}

#[test_case]
fn production_dispatch_rejects_every_operation_bit_without_changing_table() {
    with_nucleus(|nucleus, _table_addr, _second, fixture_as, _second_as| {
        let issued = nucleus
            .create_console_thread(fixture_as)
            .expect("bootstrap console key missing");
        nucleus.current_thread = nucleus.fixture_thread(0);
        // Write is zero: every individual set bit is invalid, including all
        // aliases that narrowing to u8/u16/u32 would turn back into Write.
        for op in (0..64).map(|bit| 1_u64 << bit).chain([u64::MAX]) {
            assert_dispatch_error(nucleus, issued, op, (9, 0, 0));
            assert_console_table(
                nucleus.current_thread_table_mut().unwrap(),
                issued,
                Some((Rights::all(), 0)),
            );
        }
        let table = nucleus.current_thread_table_mut().unwrap();
        let entry = table
            .remove(issued, FIXTURE_GUARD)
            .unwrap_or_else(|_| panic!("console removal failed"));
        let next = table
            .insert(bare_index(issued), entry, FIXTURE_GUARD)
            .unwrap_or_else(|_| panic!("console reinstallation failed"));
        assert_eq!(
            next,
            RawKey::from_parts(
                FIXTURE_GUARD,
                SIZE_BITS,
                bare_index(issued).0,
                issued.incarnation() + 1
            )
        );
        assert_console_table(table, next, Some((Rights::all(), 0)));
    });
}

#[test_case]
fn production_dispatch_rejects_deleted_and_same_type_replaced_keys() {
    with_nucleus(|nucleus, _table_addr, _second, fixture_as, _second_as| {
        let old = nucleus
            .create_console_thread(fixture_as)
            .expect("bootstrap console key missing");
        nucleus.current_thread = nucleus.fixture_thread(0);
        assert_dispatch_error(nucleus, old, 1, (9, 0, 0));
        assert_console_table(
            nucleus.current_thread_table_mut().unwrap(),
            old,
            Some((Rights::all(), 0)),
        );
        nucleus
            .current_thread_table_mut()
            .unwrap()
            .remove(old, FIXTURE_GUARD)
            .unwrap_or_else(|_| panic!("console removal failed"));
        let future = RawKey::from_parts(
            FIXTURE_GUARD,
            SIZE_BITS,
            bare_index(old).0,
            old.incarnation() + 1,
        );
        for op in [0, u64::MAX] {
            assert_dispatch_error(nucleus, old, op, (28, old.to_wire(), 2));
            assert_console_table(nucleus.current_thread_table_mut().unwrap(), old, None);
            // Mismatch precedes invalidation even while the slot is vacant.
            assert_dispatch_error(nucleus, future, op, (28, future.to_wire(), 1));
            assert_console_table(nucleus.current_thread_table_mut().unwrap(), old, None);
        }
        let rights = Rights(Rights::READ);
        let replacement = nucleus
            .current_thread_table_mut()
            .unwrap()
            .insert(
                bare_index(old),
                console_entry(rights, 0x2222),
                FIXTURE_GUARD,
            )
            .unwrap_or_else(|_| panic!("replacement installation failed"));
        assert_eq!(replacement, future);
        for op in [0, u64::MAX] {
            assert_dispatch_error(nucleus, old, op, (28, old.to_wire(), 1));
            assert_console_table(
                nucleus.current_thread_table_mut().unwrap(),
                replacement,
                Some((rights, 0x2222)),
            );
        }
        assert_dispatch_error(nucleus, replacement, 1, (9, 0, 0));
        assert_console_table(
            nucleus.current_thread_table_mut().unwrap(),
            replacement,
            Some((rights, 0x2222)),
        );
        nucleus
            .current_thread_table_mut()
            .unwrap()
            .remove(replacement, FIXTURE_GUARD)
            .unwrap_or_else(|_| panic!("replacement removal failed"));
        assert_dispatch_error(nucleus, old, 0, (28, old.to_wire(), 1));
        assert_console_table(
            nucleus.current_thread_table_mut().unwrap(),
            replacement,
            None,
        );
        assert_dispatch_error(nucleus, replacement, 0, (28, replacement.to_wire(), 2));
        assert_console_table(
            nucleus.current_thread_table_mut().unwrap(),
            replacement,
            None,
        );
    });
}

/// 32-byte-aligned backing for the standalone table test (the carve needs
/// `KeyEntry` alignment, which a plain byte array does not provide).
#[repr(align(32))]
struct Backing<const N: usize>([u8; N]);

#[test_case]
fn table_lookup_still_requires_an_installed_capability() {
    const TABLE_BACKING_SIZE: usize = KeyTable::carve_size(SIZE_BITS);
    static mut TABLE_BACKING: Backing<TABLE_BACKING_SIZE> = Backing([0; TABLE_BACKING_SIZE]);
    // SAFETY: the backing is exclusively owned by this sequential test and
    // covers the full variable-size carve at the table's alignment.
    let table = unsafe {
        let ptr = (&raw mut TABLE_BACKING.0).cast::<u8>();
        let id = KeyTable::initialize(ptr, SIZE_BITS);
        &mut *KeyTable::at_carve(id.address)
    };
    let slot = KeySlot::DEBUG_CONSOLE;
    let never_issued = RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, slot.0, 1);
    assert!(matches!(
        table.lookup(never_issued, FIXTURE_GUARD),
        Err(CapError::InvalidKey {
            key,
            reason: InvalidKeyReason::NeverIssued,
            operand: 0,
        }) if key == never_issued
    ));
    // An address whose guard bits do not match rejects before indexing.
    let invalid = RawKey::new(KeySlot(u32::MAX), 1);
    assert!(matches!(
        table.lookup(invalid, FIXTURE_GUARD),
        Err(CapError::InvalidKey {
            key,
            reason: InvalidKeyReason::GuardMismatch,
            operand: 0,
        }) if key == invalid
    ));

    let key = table
        .insert(slot, console_entry(Rights::all(), 0), FIXTURE_GUARD)
        .unwrap_or_else(|_| panic!("console insertion failed"));
    assert_eq!(key, RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, slot.0, 1));
    assert_eq!(table.len(), 1);
    let cap = table
        .lookup(key, FIXTURE_GUARD)
        .unwrap_or_else(|_| panic!("installed console not found"));
    assert!(matches!(
        invoke(cap, 1, u64::MAX, u64::MAX),
        Err(CapError::InvalidOperation)
    ));
}

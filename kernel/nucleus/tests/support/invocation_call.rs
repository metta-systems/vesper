//! `Invocation.Call` admission/preparation/commit and `Thread.Return` tests.
//! Roots below are checked metadata only; these tests never install TTBR0.

use {
    super::{FIXTURE_GUARD, SIZE_BITS, with_nucleus},
    crate::{
        api::{self, KeyEntry},
        objects::{
            ArchObjects, ArchObjectsImpl, ExecutionContext, InvocationContinuation,
            InvocationStack, KeyTable, Nucleus, Placement, ThreadFault,
            access::Access,
            arch_objects::{AddressSpaceId, AddressSpaceObject},
            fault::FaultDelivery,
            fp::FakeFpUnit,
            invocation::{
                CallTarget, CommittedCall, CommittedReturn, PreparedCall, ReturnFault,
                ReturnRejection,
            },
            key_table::CallerTable,
            stack_pool::FaultArea,
            thread::ThreadId,
        },
    },
    aarch64_cpu::registers::{Read, SCTLR_EL1, TCR_EL1, TTBR0_EL1, Write},
    core::{cell::UnsafeCell, num::NonZero},
    libexception::arch::aarch64::{FpState, SavedContext},
    libobject::{
        CapError, INVOCATION_STACK_DEPTH, InconsistencyReason, InvalidKeyReason, KeySlot,
        ObjectType, PlacementWindow, RawKey, Rights, StackPoolHandle,
        fault::{
            FAULT_FRAME_BYTES, FAULT_FRAME_FP, FAULT_FRAME_VERSION, FaultAction, FaultFrame,
            FaultInfo, FaultKind,
        },
        scheduler_record::ThreadFpCounters,
        stack_pool::FAULT_SLOT_BYTES,
    },
};

const ROOT: u64 = 0x2100_0000;
const SOURCE_ROOT: u64 = 0x2200_0000;
const FUNCTION: u64 = 0x8_1000;
/// The fixture target's stack pool: twenty stacks of 0x70 bytes from
/// `STACK_BASE`, so the first call's stack pointer is `STACK_END`. Twenty stacks
/// let sixteen nested calls take one each with stacks to spare.
const STACK_BASE: u64 = 0x1010;
const STACK_BYTES: u64 = 0x70;
const POOL_STACKS: u64 = 20;
const STACK_END: u64 = STACK_BASE + STACK_BYTES;
/// A value for the saved `x9`, which Call ignores. It is also a plausible stack
/// pointer, so a Call that used it would show.
const IGNORED_X9: u64 = STACK_BASE + 0x30;
const INVOCATION_SLOT: KeySlot = KeySlot(10);

struct CallFixture {
    source_as: AddressSpaceId,
    target_as: AddressSpaceId,
    source_table: u64,
    target_table: u64,
    key: RawKey,
    /// The target's stack pool, which `key` names.
    pool: StackPoolHandle,
    /// `CurrentReturnOnly` sentinel at Slot(1) in each AS table.
    source_return_key: RawKey,
    target_return_key: RawKey,
}

fn with_call(test: impl FnOnce(&mut Nucleus<ArchObjectsImpl>, CallFixture)) {
    // The serial harness runs without translation; provide the backend's
    // supported TCR profile for metadata validation only. Never install these
    // synthetic roots, enable the MMU, or leak the profile to another test.
    assert_eq!(SCTLR_EL1.get() & 1, 0);
    let original_tcr = TCR_EL1.get();
    TCR_EL1.set(16);
    with_nucleus(
        |nucleus, source_table, target_table, source_as, target_as| {
            let source = nucleus
                .create_thread(source_as)
                .expect("source Thread allocation");
            nucleus.this_core.current_thread = Some(source);
            let address_space = nucleus.address_space_mut(target_as).unwrap();
            address_space.set_translation_root(Some(ROOT));
            address_space.set_asid(Some(2));
            let source_address_space = nucleus.address_space_mut(source_as).unwrap();
            source_address_space.set_translation_root(Some(SOURCE_ROOT));
            source_address_space.set_asid(Some(1));
            let source_return_key = provisioned_return_key(source_table);
            let target_return_key = provisioned_return_key(target_table);
            let pool = nucleus
                .address_space_mut(target_as)
                .unwrap()
                .stack_pools_mut()
                .create(
                    STACK_BASE,
                    STACK_BYTES,
                    POOL_STACKS,
                    None,
                    ArchObjectsImpl::USER_VA_END,
                )
                .unwrap_or_else(|error| panic!("fixture stack pool: {:?}", error.code()));
            // The source AddressSpace has the same pool, for calls back into it.
            let source_pool = nucleus
                .address_space_mut(source_as)
                .unwrap()
                .stack_pools_mut()
                .create(
                    STACK_BASE,
                    STACK_BYTES,
                    POOL_STACKS,
                    None,
                    ArchObjectsImpl::USER_VA_END,
                )
                .unwrap_or_else(|error| panic!("fixture source stack pool: {:?}", error.code()));
            assert_eq!(source_pool, pool);
            let key = nucleus
                .current_thread_table_mut()
                .unwrap()
                .insert(
                    INVOCATION_SLOT,
                    KeyEntry::new_invocation(
                        target_as,
                        NonZero::new(FUNCTION).unwrap(),
                        pool,
                        PlacementWindow::NONE,
                    ),
                    FIXTURE_GUARD,
                )
                .unwrap_or_else(|error| panic!("Invocation install: {:?}", error.error.code()));
            test(
                nucleus,
                CallFixture {
                    source_as,
                    target_as,
                    source_table,
                    target_table,
                    key,
                    pool,
                    source_return_key,
                    target_return_key,
                },
            );
        },
    );
    TCR_EL1.set(original_tcr);
}

/// A saved Call frame with distinct values in every register.
/// The source execution SP is `STACK_END`, also the first stack top of the
/// target's pool, so tests can tell a Call that took its stack from the pool
/// from one that reused the source's. Call ignores `x8`, `x9` and `x10`, which
/// hold plausible stack pointers.
fn call_frame(key: RawKey) -> SavedContext {
    let mut saved = SavedContext::el1t(0x9_0000, STACK_END);
    for (index, register) in saved.gpr.iter_mut().enumerate() {
        *register = 0x5A00 + u64::try_from(index).unwrap();
    }
    saved.gpr[0] = key.to_wire();
    saved.gpr[1] = 0;
    saved.gpr[9] = IGNORED_X9;
    saved.gpr[8] = IGNORED_X9 + 0x10;
    saved.gpr[10] = STACK_END;
    saved.lr = 0x9_0040;
    saved.spsr_el1 |= 0x6000_0000;
    saved
}

fn source_mut(nucleus: &mut Nucleus<ArchObjectsImpl>) -> &mut crate::objects::Thread {
    let id = nucleus.this_core.current_thread.unwrap();
    nucleus.thread_mut(id).unwrap()
}

/// Prefill the source invocation stack with `count` distinct records.
fn fill_stack(nucleus: &mut Nucleus<ArchObjectsImpl>, count: usize) {
    let source = source_mut(nucleus);
    for stamp in 0..count {
        let record = InvocationContinuation {
            stamp: 0x700 + u64::try_from(stamp).unwrap(),
            source_sp: 0x1_0000 + 16 * u64::try_from(stamp).unwrap(),
            ..InvocationContinuation::empty()
        };
        assert!(source.invocation_stack.push(record).is_ok());
    }
    assert_eq!(source.invocation_stack.len(), count);
}

/// FNV-1a over a carved table's full backing: entries, incarnations, header.
fn table_digest(address: u64) -> u64 {
    // SAFETY: the fixture carves each table at this address with this size;
    // the backing stays live for the test and no guard aliases it mutably.
    let bytes = unsafe {
        core::slice::from_raw_parts(address as *const u8, KeyTable::carve_size(SIZE_BITS))
    };
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    current_thread: Option<ThreadId>,
    source_address_space: AddressSpaceId,
    source_context: crate::objects::ExecutionContext,
    source_stack: InvocationStack,
    target_root: Option<u64>,
    target_asid: Option<u16>,
    /// Stacks of the target's pool in use; `None` once the pool is destroyed.
    stacks_in_use: Option<u32>,
    source_table: u64,
    target_table: u64,
    ttbr0: u64,
    pending: usize,
    runnable: usize,
}

fn snapshot(nucleus: &mut Nucleus<ArchObjectsImpl>, fixture: &CallFixture) -> Snapshot {
    let target = nucleus.address_space(fixture.target_as);
    let current_thread = nucleus.this_core.current_thread;
    let target_root = target.and_then(|address_space| address_space.translation_root());
    let target_asid = target.and_then(|address_space| address_space.asid());
    let stacks_in_use =
        target.and_then(|address_space| address_space.stack_pools().in_use(fixture.pool).ok());
    let source = source_mut(nucleus);
    Snapshot {
        current_thread,
        source_address_space: source.address_space,
        source_context: source.context,
        source_stack: source.invocation_stack,
        target_root,
        target_asid,
        stacks_in_use,
        source_table: table_digest(fixture.source_table),
        target_table: table_digest(fixture.target_table),
        ttbr0: TTBR0_EL1.get(),
        pending: nucleus.pending.len(),
        runnable: nucleus.local_activations().len(),
    }
}

fn prepare(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    fixture: &CallFixture,
    saved: &SavedContext,
) -> (Result<PreparedCall, CapError>, Snapshot) {
    let before = snapshot(nucleus, fixture);
    // SAFETY: this fixture is serial and owns exclusive nucleus/pool backing;
    // no other access context or object/table guard overlaps this preparation.
    let result = {
        let access = unsafe { Access::new() };
        let caller = CallerTable {
            table: super::table_id(fixture.source_table),
            guard: FIXTURE_GUARD,
        };
        api::invocation::prepare_call(&access, caller, saved, nucleus)
    };
    (result, before)
}

/// Every rejection (and success) leaves all observed state byte-identical.
fn reject_preserved(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    fixture: &CallFixture,
    saved: &SavedContext,
    expected: CapError,
) {
    reject_preserved_code(nucleus, fixture, saved, expected.code());
}

fn reject_preserved_code(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    fixture: &CallFixture,
    saved: &SavedContext,
    expected: (u64, u64, u64),
) {
    let (result, before) = prepare(nucleus, fixture, saved);
    let error = result.expect_err("rejected Call admission prepared a migration");
    assert_eq!(error.code(), expected);
    assert_eq!(snapshot(nucleus, fixture), before);
}

/// The same rejection through the production SVC dispatch entry
/// (`api::handle_cap_invoke`, as `main.rs` calls it with the saved frame):
/// caller-table resolution from the current Thread, kind routing, admission
/// and commit. A rejected Call must report the same error and commit nothing
/// — no continuation push, no AddressSpace switch, no table or TTBR0 change.
fn dispatch_rejected(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    fixture: &CallFixture,
    saved: &SavedContext,
    expected: (u64, u64, u64),
) {
    let before = snapshot(nucleus, fixture);
    let Err(error) = api::handle_cap_invoke(nucleus, saved) else {
        panic!("dispatched Call committed despite a rejected admission");
    };
    assert_eq!(error.code(), expected);
    assert_eq!(snapshot(nucleus, fixture), before);
}

/// Take every stack of the target's pool, as outstanding calls would.
fn exhaust_pool(nucleus: &mut Nucleus<ArchObjectsImpl>, fixture: &CallFixture) {
    let pools = nucleus
        .address_space_mut(fixture.target_as)
        .unwrap()
        .stack_pools_mut();
    for _ in 0..POOL_STACKS {
        pools
            .take(fixture.pool)
            .unwrap_or_else(|error| panic!("taking a stack: {:?}", error.code()));
    }
}

/// Give every stack of the target's pool back.
fn release_pool(nucleus: &mut Nucleus<ArchObjectsImpl>, fixture: &CallFixture) {
    let pools = nucleus
        .address_space_mut(fixture.target_as)
        .unwrap()
        .stack_pools_mut();
    for index in 0..POOL_STACKS {
        assert!(pools.give(fixture.pool.slot(), u8::try_from(index).unwrap()));
    }
}

/// Destroy the target's pool, leaving the Invocation's handle stale.
fn destroy_pool(nucleus: &mut Nucleus<ArchObjectsImpl>, fixture: &CallFixture) {
    nucleus
        .address_space_mut(fixture.target_as)
        .unwrap()
        .stack_pools_mut()
        .destroy(fixture.pool)
        .unwrap_or_else(|error| panic!("destroying the pool: {:?}", error.code()));
}

fn stale_pool() -> CapError {
    // The slot's generation after the pool's destruction.
    CapError::StaleGeneration {
        current: u64::from(libobject::StackPoolHandle::new(0, 1).generation()) + 1,
    }
}

fn no_free_stack() -> CapError {
    CapError::NoFreeStack { count: POOL_STACKS }
}

fn break_translation(nucleus: &mut Nucleus<ArchObjectsImpl>, fixture: &CallFixture) {
    let address_space = nucleus.address_space_mut(fixture.target_as).unwrap();
    address_space.set_translation_root(None);
    address_space.set_asid(None);
}

#[test_case]
fn call_preparation_describes_the_migration_without_mutating_anything() {
    with_call(|nucleus, fixture| {
        fill_stack(nucleus, 3);
        let saved = call_frame(fixture.key);
        let (result, before) = prepare(nucleus, &fixture, &saved);
        let prepared =
            result.unwrap_or_else(|error| panic!("valid Call rejected: {:?}", error.code()));
        assert_eq!(snapshot(nucleus, &fixture), before);
        assert_eq!(source_mut(nucleus).invocation_stack.len(), 3);

        let current = nucleus.this_core.current_thread.unwrap();
        assert_eq!(prepared.source_thread(), current);
        assert_eq!(prepared.entry_pc().get(), FUNCTION);
        assert_eq!(prepared.stack_pool(), fixture.pool);
        assert_eq!(
            prepared.arguments(),
            [0x5A02, 0x5A03, 0x5A04, 0x5A05, 0x5A06, 0x5A07]
        );
        assert_eq!(prepared.depth(), 3);
        assert_eq!(prepared.translation().address_space(), fixture.target_as);
        assert_eq!(prepared.translation().root(), ROOT);
        assert_eq!(prepared.translation().asid(), 2);
        assert_eq!(
            prepared.translation().keytable().address(),
            fixture.target_table
        );
        // The stamp is the real clock at preparation.
        let stamp = prepared.continuation().stamp;
        let expected = InvocationContinuation::from_saved(fixture.source_as, saved, stamp);
        assert_eq!(*prepared.continuation(), expected);
        assert_eq!(prepared.continuation().source_pc, 0x9_0000);
        assert_eq!(prepared.continuation().source_sp, STACK_END);
        assert_eq!(prepared.continuation().source_spsr, saved.spsr_el1);
        assert_eq!(prepared.continuation().source_x19_x30[11], saved.lr);
    });
}

#[test_case]
fn call_ignores_saved_x9_and_the_registers_around_it() {
    with_call(|nucleus, fixture| {
        // Whatever the caller leaves in x8, x9 and x10, the call is admitted
        // against the pool alone.
        for submitted in [0, STACK_BASE, IGNORED_X9 + 8, u64::MAX] {
            let mut saved = call_frame(fixture.key);
            saved.gpr[9] = submitted;
            saved.gpr[8] = submitted;
            saved.gpr[10] = submitted;
            let (result, before) = prepare(nucleus, &fixture, &saved);
            assert_eq!(admitted(result).stack_pool(), fixture.pool);
            assert_eq!(snapshot(nucleus, &fixture), before);
        }
    });
}

#[test_case]
fn a_stale_or_exhausted_pool_is_reported_before_unready_translation_or_full_depth() {
    for stale in [true, false] {
        for (unready, full) in [(true, false), (false, true), (true, true)] {
            with_call(|nucleus, fixture| {
                if stale {
                    destroy_pool(nucleus, &fixture);
                } else {
                    exhaust_pool(nucleus, &fixture);
                }
                if unready {
                    break_translation(nucleus, &fixture);
                }
                if full {
                    fill_stack(nucleus, INVOCATION_STACK_DEPTH);
                }
                let saved = call_frame(fixture.key);
                let expected = if stale { stale_pool() } else { no_free_stack() }.code();
                reject_preserved_code(nucleus, &fixture, &saved, expected);
                dispatch_rejected(nucleus, &fixture, &saved, expected);
            });
        }
    }
}

#[test_case]
fn calls_take_the_lowest_free_stack_and_returns_give_it_back() {
    with_call(|nucleus, fixture| {
        let first = committed(call(nucleus, &fixture, &call_frame(fixture.key)));
        assert_eq!(first.target.sp, STACK_END, "the first stack's top");
        assert_eq!(snapshot(nucleus, &fixture).stacks_in_use, Some(1));
        let top = *source_mut(nucleus).invocation_stack.top().unwrap();
        assert!(top.stack.taken);
        assert_eq!((top.stack.pool, top.stack.index), (0, 0));

        // The Thread now runs in the target; its next call takes the next stack.
        let second = committed(call(nucleus, &fixture, &call_frame(fixture.key)));
        assert_eq!(second.target.sp, STACK_END + STACK_BYTES);
        assert_eq!(snapshot(nucleus, &fixture).stacks_in_use, Some(2));
    });
}

#[test_case]
fn translation_failure_is_reported_before_depth_exhaustion() {
    for (root, asid, expected) in [
        (None, None, CapError::NotMapped),
        (Some(ROOT), None, CapError::NotMapped),
        (None, Some(2), CapError::NotMapped),
        (Some(ROOT + 1), Some(2), CapError::InvalidPointer),
        (Some(ROOT), Some(0), CapError::InvalidOperation),
    ] {
        with_call(|nucleus, fixture| {
            fill_stack(nucleus, INVOCATION_STACK_DEPTH);
            let address_space = nucleus.address_space_mut(fixture.target_as).unwrap();
            address_space.set_translation_root(root);
            address_space.set_asid(asid);
            let saved = call_frame(fixture.key);
            let expected = expected.code();
            reject_preserved_code(nucleus, &fixture, &saved, expected);
            dispatch_rejected(nucleus, &fixture, &saved, expected);
        });
    }
}

#[test_case]
fn depth_sixteen_is_rejected_with_current_count_and_fifteen_is_admitted() {
    with_call(|nucleus, fixture| {
        fill_stack(nucleus, INVOCATION_STACK_DEPTH - 1);
        let saved = call_frame(fixture.key);
        let (result, before) = prepare(nucleus, &fixture, &saved);
        assert_eq!(admitted(result).depth(), INVOCATION_STACK_DEPTH - 1);
        assert_eq!(snapshot(nucleus, &fixture), before);

        fill_stack_one_more(nucleus);
        assert_eq!(CapError::NestingDepth { count: 16 }.code(), (34, 16, 0));
        reject_preserved(
            nucleus,
            &fixture,
            &saved,
            CapError::NestingDepth { count: 16 },
        );
        dispatch_rejected(
            nucleus,
            &fixture,
            &saved,
            CapError::NestingDepth { count: 16 }.code(),
        );
        assert_eq!(source_mut(nucleus).invocation_stack.len(), 16);
    });
}

/// Positive control for the dispatch-level rejections: at depth 15 the same
/// saved frame through `api::handle_cap_invoke` commits the Call — the
/// sixteenth continuation is pushed and the Thread migrates to the target.
#[test_case]
fn dispatched_call_at_depth_fifteen_commits() {
    with_call(|nucleus, fixture| {
        fill_stack(nucleus, INVOCATION_STACK_DEPTH - 1);
        let saved = call_frame(fixture.key);
        match api::handle_cap_invoke(nucleus, &saved) {
            Ok(api::InvokeOutcome::Call(_)) => {}
            Ok(_) => panic!("dispatched Call produced a non-Call outcome"),
            Err(error) => panic!("dispatched Call rejected: {:?}", error.code()),
        }
        let source = source_mut(nucleus);
        assert_eq!(source.invocation_stack.len(), INVOCATION_STACK_DEPTH);
        assert_eq!(source.address_space, fixture.target_as);
    });
}

fn fill_stack_one_more(nucleus: &mut Nucleus<ArchObjectsImpl>) {
    let source = source_mut(nucleus);
    let record = InvocationContinuation {
        stamp: 0xF00,
        ..InvocationContinuation::empty()
    };
    assert!(source.invocation_stack.push(record).is_ok());
    assert!(source.invocation_stack.is_full());
}

#[test_case]
fn stale_target_identity_precedes_invalid_sp() {
    for reuse in [false, true] {
        with_call(|nucleus, fixture| {
            let binding = nucleus
                .address_space(fixture.target_as)
                .unwrap()
                .keytable()
                .unwrap();
            crate::retire_address_space(fixture.target_as)
                .unwrap_or_else(|e| panic!("AS retirement: {e:?}"));
            if reuse {
                let mut replacement =
                    crate::objects::arch_objects::bound_address_space::<ArchObjectsImpl>(binding);
                replacement.set_translation_root(Some(ROOT));
                replacement.set_asid(Some(3));
                let replacement_id =
                    crate::objects::nucleus::test_arena::carve(replacement).unwrap();
                assert_ne!(replacement_id, fixture.target_as);
            }
            fill_stack(nucleus, INVOCATION_STACK_DEPTH);
            let saved = call_frame(fixture.key);
            reject_preserved(
                nucleus,
                &fixture,
                &saved,
                CapError::InconsistentKey {
                    key: fixture.key,
                    reason: InconsistencyReason::ObjectRetired,
                    operand: 0,
                },
            );
        });
    }
}

#[test_case]
fn operation_kind_and_authority_precede_supplied_values() {
    with_call(|nucleus, fixture| {
        // Invalid SP and full depth must not mask key-stage rejections.
        fill_stack(nucleus, INVOCATION_STACK_DEPTH);
        let mut saved = call_frame(fixture.key);
        saved.gpr[1] = 2;
        reject_preserved(nucleus, &fixture, &saved, CapError::InvalidOperation);
        // Operation 1 is CallWithKey: this Invocation accepts no key, which is
        // also a key-stage rejection ahead of the stack and depth checks.
        saved.gpr[1] = 1;
        reject_preserved(nucleus, &fixture, &saved, CapError::KeyNotAccepted);

        let attenuated = {
            let table = nucleus.current_thread_table_mut().unwrap();
            let entry = *table
                .lookup(fixture.key, FIXTURE_GUARD)
                .unwrap_or_else(|error| panic!("Invocation lookup: {:?}", error.code()));
            assert!(entry.rights().has(Rights::CALL));
            table
                .insert(KeySlot(11), entry.derive(Rights::empty()), FIXTURE_GUARD)
                .unwrap_or_else(|error| panic!("attenuated install: {:?}", error.error.code()))
        };
        let saved = call_frame(attenuated);
        reject_preserved(nucleus, &fixture, &saved, CapError::InsufficientRights);

        let wrong_kind = nucleus
            .current_thread_table_mut()
            .unwrap()
            .insert(
                KeySlot(12),
                KeyEntry::new_carved(fixture.target_as, Rights::all(), 0),
                FIXTURE_GUARD,
            )
            .unwrap_or_else(|error| panic!("wrong-kind install: {:?}", error.error.code()));
        let saved = call_frame(wrong_kind);
        reject_preserved(
            nucleus,
            &fixture,
            &saved,
            CapError::TypeMismatch {
                expected: ObjectType::INVOCATION,
                found: ObjectType::ADDRESS_SPACE,
            },
        );
    });
}

fn admitted(result: Result<PreparedCall, CapError>) -> PreparedCall {
    result.unwrap_or_else(|error| panic!("valid Call rejected: {:?}", error.code()))
}

/// SPSR condition flags N, Z, C and V.
const NZCV: u64 = 0xF000_0000;
/// A source Thread's EL0 TLS value, and one a target sets during a call.
const SOURCE_TLS: u64 = 0x7150_0000_0000_1500;
const TARGET_TLS: u64 = 0x7a40_0000_0000_7a40;

fn caller(fixture: &CallFixture) -> CallerTable {
    CallerTable {
        table: super::table_id(fixture.source_table),
        guard: FIXTURE_GUARD,
    }
}

fn call(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    fixture: &CallFixture,
    saved: &SavedContext,
) -> Result<CommittedCall, CapError> {
    // SAFETY: serial fixture with exclusive nucleus/pool backing; no other
    // access context or object/table guard overlaps this transaction.
    let access = unsafe { Access::new() };
    api::invocation::call(&access, caller(fixture), saved, nucleus)
}

fn prepare_direct(
    nucleus: &Nucleus<ArchObjectsImpl>,
    target_as: AddressSpaceId,
    saved: &SavedContext,
) -> Result<PreparedCall, CapError> {
    let target = CallTarget {
        address_space: target_as,
        function_address: NonZero::new(FUNCTION).unwrap(),
        placement_window: PlacementWindow::NONE,
        badge: 0,
        // The first pool the fixture's target AddressSpace made.
        stack_pool: StackPoolHandle::new(0, 1),
    };
    // SAFETY: as in `call`.
    let access = unsafe { Access::new() };
    nucleus.prepare_call(&access, saved, target, None)
}

fn committed(result: Result<CommittedCall, CapError>) -> CommittedCall {
    result.unwrap_or_else(|error| panic!("valid Call commit rejected: {:?}", error.code()))
}

#[test_case]
fn call_commit_pushes_continuation_and_migrates_into_the_target_table() {
    with_call(|nucleus, fixture| {
        fill_stack(nucleus, 2);
        let saved = call_frame(fixture.key);
        let before = snapshot(nucleus, &fixture);
        let expected = *admitted(prepare(nucleus, &fixture, &saved).0).continuation();
        let commit = committed(call(nucleus, &fixture, &saved));

        let source = source_mut(nucleus);
        assert_eq!(source.address_space, fixture.target_as);
        assert_eq!(source.context, before.source_context);
        assert_eq!(source.invocation_stack.len(), 3);
        // The commit stamps the continuation with the real clock, after the
        // preparation `expected` came from: equal except for a later stamp.
        let top = *source.invocation_stack.top().unwrap();
        assert!(top.stamp >= expected.stamp, "continuation stamp went back");
        // The commit also records the stack it took from the target's pool.
        assert_eq!(
            (top.stack.pool, top.stack.index, top.stack.taken),
            (0, 0, true)
        );
        assert_eq!(
            InvocationContinuation {
                stamp: expected.stamp,
                stack: expected.stack,
                ..top
            },
            expected
        );
        assert_eq!(expected.source_address_space, fixture.source_as);
        // Earlier records are untouched beneath the new top.
        let mut below = source.invocation_stack;
        assert!(below.pop().is_some());
        assert_eq!(below, before.source_stack);

        // Key resolution now selects the target AddressSpace's table.
        assert_eq!(nucleus.this_core.current_thread, before.current_thread);
        assert_eq!(
            nucleus.current_thread_table_addr(),
            Some(fixture.target_table)
        );
        assert_eq!(Some(commit.source_thread), nucleus.this_core.current_thread);
        assert_eq!(commit.translation.address_space(), fixture.target_as);
        assert_eq!(commit.translation.root(), ROOT);
        assert_eq!(commit.translation.asid(), 2);

        // Commit touches no hardware, table, target metadata or scheduling.
        let after = snapshot(nucleus, &fixture);
        assert_eq!(after.stacks_in_use, Some(1), "the commit took one stack");
        assert_eq!(after.ttbr0, before.ttbr0);
        assert_eq!(after.source_table, before.source_table);
        assert_eq!(after.target_table, before.target_table);
        assert_eq!(after.target_root, before.target_root);
        assert_eq!(after.target_asid, before.target_asid);
        assert_eq!(after.pending, before.pending);
        assert_eq!(after.runnable, before.runnable);
    });
}

#[test_case]
fn target_entry_frame_keeps_only_inputs_and_inherits_non_nzcv_status() {
    with_call(|nucleus, fixture| {
        let mut saved = call_frame(fixture.key);
        // Every condition flag and an unmasked IRQ bit set in the source.
        saved.spsr_el1 = (saved.spsr_el1 | NZCV) & !0x80;
        saved.tpidr_el0 = SOURCE_TLS;
        let target = committed(call(nucleus, &fixture, &saved)).target;

        assert_eq!(&target.gpr[0..2], &[0, 0]);
        assert_eq!(&target.gpr[2..8], &saved.gpr[2..8]);
        assert!(target.gpr[8..].iter().all(|&register| register == 0));
        assert_eq!(target.gpr[9], 0, "consumed x9 leaked to the target");
        assert_eq!(target.gpr[18], 0);
        assert_eq!(target.lr, 0);
        assert_eq!(target.elr_el1, FUNCTION);
        assert_eq!(
            target.sp, STACK_END,
            "the call runs on the pool's first stack"
        );
        assert_eq!(target.spsr_el1 & NZCV, 0);
        assert_eq!(target.spsr_el1, saved.spsr_el1 & !NZCV);
        assert_eq!(target.spsr_el1 & 0x3c0, saved.spsr_el1 & 0x3c0);
        assert_eq!(target.origin, saved.origin);
        assert_eq!(target.tpidr_el0, 0, "the source's TLS leaked to the target");

        // The source keeps every scrubbed value privately in its record.
        let record = *source_mut(nucleus).invocation_stack.top().unwrap();
        assert_eq!(record.source_spsr, saved.spsr_el1);
        assert_eq!(&record.source_x19_x30[..11], &saved.gpr[19..30]);
        assert_eq!(record.source_x19_x30[11], saved.lr);
        assert_eq!(record.source_pc, saved.elr_el1);
        assert_eq!(record.source_sp, saved.sp);
        assert_eq!(record.source_tpidr_el0, SOURCE_TLS);
    });
}

#[test_case]
fn nested_commits_fill_sixteen_records_then_reject_the_seventeenth() {
    with_call(|nucleus, fixture| {
        for depth in 0..INVOCATION_STACK_DEPTH {
            let mut saved = call_frame(fixture.key);
            saved.elr_el1 = 0x9_0000 + 4 * u64::try_from(depth).unwrap();
            let prepared = admitted(prepare_direct(nucleus, fixture.target_as, &saved));
            assert_eq!(prepared.depth(), depth);
            committed(commit(nucleus, prepared));
        }
        let stack = source_mut(nucleus).invocation_stack;
        assert!(stack.is_full());
        let mut records = stack;
        for depth in (0..INVOCATION_STACK_DEPTH).rev() {
            let record = records.pop().unwrap();
            assert_eq!(
                record.source_pc,
                0x9_0000 + 4 * u64::try_from(depth).unwrap()
            );
            // Only the first Call left the source AS; the rest nest in the target.
            let source = if depth == 0 {
                fixture.source_as
            } else {
                fixture.target_as
            };
            assert_eq!(record.source_address_space, source);
        }

        let saved = call_frame(fixture.key);
        let error = prepare_direct(nucleus, fixture.target_as, &saved)
            .expect_err("seventeenth Call admitted");
        assert_eq!(error.code(), (34, 16, 0));
        assert_eq!(source_mut(nucleus).invocation_stack, stack);
        assert_eq!(source_mut(nucleus).address_space, fixture.target_as);
    });
}

#[test_case]
fn stale_prepared_call_is_rejected_without_mutation() {
    for staleness in 0..3 {
        with_call(|nucleus, fixture| {
            let saved = call_frame(fixture.key);
            let prepared = admitted(prepare(nucleus, &fixture, &saved).0);
            match staleness {
                // Another push happened after admission.
                0 => fill_stack(nucleus, 1),
                // The Thread already migrated elsewhere.
                1 => source_mut(nucleus).address_space = fixture.target_as,
                // A different Thread is current.
                _ => {
                    let other = nucleus.create_thread(fixture.source_as).unwrap();
                    nucleus.this_core.current_thread = Some(other);
                }
            }
            let before = snapshot(nucleus, &fixture);
            let error = nucleus
                .commit_call(&unsafe { Access::new() }, prepared)
                .expect_err("stale Call description committed");
            assert_eq!(error.code(), CapError::InvalidOperation.code());
            assert_eq!(snapshot(nucleus, &fixture), before);
        });
    }
}

#[test_case]
fn rejected_call_commits_nothing() {
    with_call(|nucleus, fixture| {
        fill_stack(nucleus, 4);
        let saved = call_frame(fixture.key);
        fill_stack_one_more_until_full(nucleus);
        let before = snapshot(nucleus, &fixture);
        let error = call(nucleus, &fixture, &saved).expect_err("full stack committed");
        assert_eq!(error.code(), (34, 16, 0));
        assert_eq!(snapshot(nucleus, &fixture), before);

        // An exhausted pool is reported first, and commits nothing.
        exhaust_pool(nucleus, &fixture);
        let exhausted = snapshot(nucleus, &fixture);
        assert_eq!(
            exhausted.stacks_in_use,
            Some(u32::try_from(POOL_STACKS).unwrap())
        );
        let error = call(nucleus, &fixture, &saved).expect_err("exhausted pool committed");
        assert_eq!(error.code(), no_free_stack().code());
        assert_eq!(snapshot(nucleus, &fixture), exhausted);
    });
}

fn fill_stack_one_more_until_full(nucleus: &mut Nucleus<ArchObjectsImpl>) {
    while !source_mut(nucleus).invocation_stack.is_full() {
        fill_stack_one_more_unchecked(nucleus);
    }
}

fn fill_stack_one_more_unchecked(nucleus: &mut Nucleus<ArchObjectsImpl>) {
    let record = InvocationContinuation {
        stamp: 0xE00,
        ..InvocationContinuation::empty()
    };
    assert!(source_mut(nucleus).invocation_stack.push(record).is_ok());
}

// ═════════════════════════════
// THREAD.RETURN
// ═════════════════════════════

/// The deterministic key of the Return sentinel that provisioning installed,
/// checked to resolve to a `CurrentReturnOnly` entry.
fn provisioned_return_key(table_address: u64) -> RawKey {
    let key = RawKey::from_parts(
        FIXTURE_GUARD,
        SIZE_BITS,
        KeySlot::THREAD_RETURN.0,
        KeySlot::THREAD_RETURN_INCARNATION,
    );
    // SAFETY: serial fixture; the carved table is live and no other guard
    // aliases it during this read.
    let access = unsafe { Access::new() };
    let table = access
        .resolve_object::<KeyTable>(super::table_id(table_address))
        .unwrap_or_else(|error| panic!("fixture table: {error:?}"));
    let entry = table
        .lookup(key, FIXTURE_GUARD)
        .unwrap_or_else(|error| panic!("provisioned Return key: {:?}", error.code()));
    assert!(entry.is_thread_return_key());
    key
}

/// A target-side Return frame: payload `r0`/`r1` in x2/x3 and `r2..r5`
/// (`extra`) in x4..x7, and target sentinels everywhere else that must never
/// reach the source.
fn return_frame(key: RawKey, payload: [u64; 2], extra: [u64; 4]) -> SavedContext {
    let mut saved = SavedContext::el1t(FUNCTION + 0x40, STACK_END - 16);
    for (index, register) in saved.gpr.iter_mut().enumerate() {
        *register = 0x7E00 + u64::try_from(index).unwrap();
    }
    saved.gpr[0] = key.to_wire();
    saved.gpr[1] = 0;
    saved.gpr[2] = payload[0];
    saved.gpr[3] = payload[1];
    saved.gpr[4..8].copy_from_slice(&extra);
    saved.lr = 0x7E_0030;
    saved.spsr_el1 |= 0x9000_0000;
    saved
}

fn current_caller(nucleus: &Nucleus<ArchObjectsImpl>) -> CallerTable {
    CallerTable {
        table: super::table_id(nucleus.current_thread_table_addr().unwrap()),
        guard: FIXTURE_GUARD,
    }
}

fn thread_return(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    saved: &SavedContext,
) -> Result<CommittedReturn, ReturnRejection> {
    let caller = current_caller(nucleus);
    // SAFETY: serial fixture with exclusive nucleus/pool backing; no other
    // access context or object/table guard overlaps this transaction.
    let access = unsafe { Access::new() };
    api::thread::return_from_call(&access, caller, saved, nucleus)
}

fn rejection_name(rejection: ReturnRejection) -> (u64, u64, u64) {
    match rejection {
        ReturnRejection::Error(error) => error.code(),
        ReturnRejection::Fault(fault) => panic!("unexpected Return fault {fault:?}"),
    }
}

fn returned(result: Result<CommittedReturn, ReturnRejection>) -> CommittedReturn {
    result.unwrap_or_else(|rejection| match rejection {
        ReturnRejection::Error(error) => panic!("valid Return rejected: {:?}", error.code()),
        ReturnRejection::Fault(fault) => panic!("valid Return faulted: {fault:?}"),
    })
}

/// Return is rejected with an ordinary error and nothing changes.
fn return_rejected(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    fixture: &CallFixture,
    saved: &SavedContext,
    expected: (u64, u64, u64),
) {
    let before = snapshot(nucleus, fixture);
    let rejection = thread_return(nucleus, saved).expect_err("rejected Return committed");
    assert_eq!(rejection_name(rejection), expected);
    assert_eq!(snapshot(nucleus, fixture), before);
}

/// Return is classified as a protocol fault, with no pop and no change.
fn return_faulted(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    fixture: &CallFixture,
    saved: &SavedContext,
    expected: ReturnFault,
) {
    let before = snapshot(nucleus, fixture);
    match thread_return(nucleus, saved).expect_err("faulting Return committed") {
        ReturnRejection::Fault(fault) => assert_eq!(fault, expected),
        ReturnRejection::Error(error) => {
            panic!("Return fault reported as ordinary error {:?}", error.code())
        }
    }
    assert_eq!(snapshot(nucleus, fixture), before);
}

#[test_case]
fn call_then_return_restores_the_exact_source_context_and_table() {
    with_call(|nucleus, fixture| {
        fill_stack(nucleus, 2);
        let before = snapshot(nucleus, &fixture);
        let mut call_saved = call_frame(fixture.key);
        call_saved.spsr_el1 |= NZCV;
        call_saved.tpidr_el0 = SOURCE_TLS;
        committed(call(nucleus, &fixture, &call_saved));
        assert_eq!(
            nucleus.current_thread_table_addr(),
            Some(fixture.target_table)
        );

        let payload = [0xAAAA_0001, 0xBBBB_0002];
        let mut saved = return_frame(fixture.target_return_key, payload, [1, 2, 3, 4]);
        // The target sets its own TLS during the call; Return discards it.
        saved.tpidr_el0 = TARGET_TLS;
        let commit = returned(thread_return(nucleus, &saved));
        let resumed = commit.resumed;

        assert_eq!(resumed.gpr[0], libobject::syscall_status::SUCCESS);
        assert_eq!(resumed.gpr[1], payload[0]);
        assert_eq!(resumed.gpr[2], payload[1]);
        assert_eq!(&resumed.gpr[3..7], &[1, 2, 3, 4], "r2..r5 in x3..x6");
        assert!(resumed.gpr[7..19].iter().all(|&register| register == 0));
        assert_eq!(&resumed.gpr[19..30], &call_saved.gpr[19..30]);
        assert_eq!(resumed.lr, call_saved.lr);
        assert_eq!(resumed.sp, call_saved.sp);
        assert_eq!(resumed.elr_el1, call_saved.elr_el1);
        // Exact raw source SPSR, including its original NZCV.
        assert_eq!(resumed.spsr_el1, call_saved.spsr_el1);
        assert_eq!(resumed.origin, call_saved.origin);
        assert_eq!(
            resumed.tpidr_el0, SOURCE_TLS,
            "Return must restore the source's TLS"
        );

        assert_eq!(commit.translation.address_space(), fixture.source_as);
        assert_eq!(commit.translation.root(), SOURCE_ROOT);
        assert_eq!(commit.translation.asid(), 1);
        assert_eq!(Some(commit.source_thread), nucleus.this_core.current_thread);
        // Back in the source AS and table, with the prior stack exactly restored.
        assert_eq!(
            nucleus.current_thread_table_addr(),
            Some(fixture.source_table)
        );
        assert_eq!(snapshot(nucleus, &fixture), before);
    });
}

#[test_case]
fn return_carries_x4_to_x7_to_the_callers_x3_to_x6() {
    for extra in [[0; 4], [u64::MAX; 4], [0x4, 0, u64::MAX, 0x7777]] {
        with_call(|nucleus, fixture| {
            committed(call(nucleus, &fixture, &call_frame(fixture.key)));
            let saved = return_frame(fixture.target_return_key, [5, 6], extra);
            let resumed = returned(thread_return(nucleus, &saved)).resumed;
            assert_eq!(&resumed.gpr[1..3], &[5, 6]);
            assert_eq!(&resumed.gpr[3..7], &extra, "r2..r5 in x3..x6");
            assert_eq!(resumed.gpr[7], 0, "x7 is zeroed");
            assert!(source_mut(nucleus).invocation_stack.is_empty());
        });
    }
}

#[test_case]
fn nested_returns_pop_in_lifo_order() {
    with_call(|nucleus, fixture| {
        let first = call_frame(fixture.key);
        committed(call(nucleus, &fixture, &first));
        let mut second = call_frame(fixture.key);
        second.elr_el1 = 0x9_1000;
        second.gpr[19] = 0x1919;
        let prepared = admitted(prepare_direct(nucleus, fixture.target_as, &second));
        committed(commit(nucleus, prepared));
        assert_eq!(source_mut(nucleus).invocation_stack.len(), 2);

        let saved = return_frame(fixture.target_return_key, [1, 2], [0; 4]);
        let inner = returned(thread_return(nucleus, &saved));
        assert_eq!(inner.resumed.elr_el1, 0x9_1000);
        assert_eq!(inner.resumed.gpr[19], 0x1919);
        // The inner Call was made from the target AS, so the Thread stays there.
        assert_eq!(inner.translation.address_space(), fixture.target_as);
        assert_eq!(source_mut(nucleus).address_space, fixture.target_as);

        let outer = returned(thread_return(nucleus, &saved));
        assert_eq!(outer.resumed.elr_el1, first.elr_el1);
        assert_eq!(outer.translation.address_space(), fixture.source_as);
        assert_eq!(source_mut(nucleus).address_space, fixture.source_as);
        assert!(source_mut(nucleus).invocation_stack.is_empty());
    });
}

#[test_case]
fn underflow_is_an_illegal_return_fault_without_mutation() {
    with_call(|nucleus, fixture| {
        let saved = return_frame(fixture.source_return_key, [1, 2], [0; 4]);
        return_faulted(nucleus, &fixture, &saved, ReturnFault::IllegalReturn);
        // Also after a full round trip has emptied the stack again.
        committed(call(nucleus, &fixture, &call_frame(fixture.key)));
        let target_saved = return_frame(fixture.target_return_key, [1, 2], [0; 4]);
        returned(thread_return(nucleus, &target_saved));
        return_faulted(nucleus, &fixture, &saved, ReturnFault::IllegalReturn);
    });
}

#[test_case]
fn retired_source_is_a_return_target_retired_fault_without_pop() {
    for reuse in [false, true] {
        with_call(|nucleus, fixture| {
            committed(call(nucleus, &fixture, &call_frame(fixture.key)));
            let binding = nucleus
                .address_space(fixture.source_as)
                .unwrap()
                .keytable()
                .unwrap();
            crate::retire_address_space(fixture.source_as)
                .unwrap_or_else(|e| panic!("source AS retirement: {e:?}"));
            if reuse {
                let mut replacement =
                    crate::objects::arch_objects::bound_address_space::<ArchObjectsImpl>(binding);
                replacement.set_translation_root(Some(SOURCE_ROOT));
                replacement.set_asid(Some(1));
                let replacement_id =
                    crate::objects::nucleus::test_arena::carve(replacement).unwrap();
                assert_ne!(replacement_id, fixture.source_as);
            }
            let saved = return_frame(fixture.target_return_key, [1, 2], [0; 4]);
            return_faulted(nucleus, &fixture, &saved, ReturnFault::ReturnTargetRetired);
            assert_eq!(source_mut(nucleus).invocation_stack.len(), 1);
        });
    }
}

#[test_case]
fn live_but_unready_source_is_rejected_without_pop() {
    with_call(|nucleus, fixture| {
        committed(call(nucleus, &fixture, &call_frame(fixture.key)));
        nucleus
            .address_space_mut(fixture.source_as)
            .unwrap()
            .set_asid(None);
        let saved = return_frame(fixture.target_return_key, [1, 2], [0; 4]);
        return_rejected(nucleus, &fixture, &saved, CapError::NotMapped.code());
        assert_eq!(source_mut(nucleus).invocation_stack.len(), 1);
    });
}

#[test_case]
fn return_key_form_and_lookup_errors_precede_the_pop() {
    with_call(|nucleus, fixture| {
        committed(call(nucleus, &fixture, &call_frame(fixture.key)));
        let thread = nucleus.this_core.current_thread.unwrap();
        let named = nucleus
            .current_thread_table_mut()
            .unwrap()
            .insert(
                KeySlot(20),
                KeyEntry::new_thread(thread, Rights::all(), 0),
                FIXTURE_GUARD,
            )
            .unwrap_or_else(|error| panic!("named Thread install: {:?}", error.error.code()));

        // A named Thread does not authorize Return.
        let saved = return_frame(named, [1, 2], [0; 4]);
        return_rejected(nucleus, &fixture, &saved, CapError::InvalidOperation.code());

        // Thread management opcodes on the sentinel are not Return.
        let mut saved = return_frame(fixture.target_return_key, [1, 2], [0; 4]);
        saved.gpr[1] = 4;
        return_rejected(nucleus, &fixture, &saved, CapError::InvalidOperation.code());

        // The Invocation key in the target table is not a Thread.
        let wrong_kind = nucleus
            .current_thread_table_mut()
            .unwrap()
            .insert(
                KeySlot(21),
                KeyEntry::new_invocation(
                    fixture.target_as,
                    NonZero::new(FUNCTION).unwrap(),
                    fixture.pool,
                    PlacementWindow::NONE,
                ),
                FIXTURE_GUARD,
            )
            .unwrap_or_else(|error| panic!("Invocation install: {:?}", error.error.code()));
        let saved = return_frame(wrong_kind, [1, 2], [0; 4]);
        return_rejected(
            nucleus,
            &fixture,
            &saved,
            CapError::TypeMismatch {
                expected: ObjectType::THREAD,
                found: ObjectType::INVOCATION,
            }
            .code(),
        );

        // A stale incarnation and an empty-slot key keep their ordinary
        // lookup failures.
        let key = fixture.target_return_key;
        let stale = RawKey::new(key.slot(), key.incarnation() + 1);
        for candidate in [stale, RawKey::from_wire(0)] {
            let expected = {
                let table = nucleus.current_thread_table_mut().unwrap();
                match table.lookup(candidate, FIXTURE_GUARD) {
                    Ok(_) => panic!("fixture key unexpectedly valid"),
                    Err(error) => error.with_key_operand(0).code(),
                }
            };
            let saved = return_frame(candidate, [1, 2], [0; 4]);
            return_rejected(nucleus, &fixture, &saved, expected);
        }
        assert_eq!(source_mut(nucleus).invocation_stack.len(), 1);
    });
}

#[test_case]
fn stale_prepared_return_is_rejected_without_mutation() {
    for staleness in 0..3 {
        with_call(|nucleus, fixture| {
            committed(call(nucleus, &fixture, &call_frame(fixture.key)));
            let saved = return_frame(fixture.target_return_key, [1, 2], [0; 4]);
            let caller = current_caller(nucleus);
            let prepared = {
                // SAFETY: serial fixture; no overlapping access context.
                let access = unsafe { Access::new() };
                match api::thread::prepare_return(&access, caller, &saved, nucleus) {
                    Ok(prepared) => prepared,
                    Err(_) => panic!("valid Return not admitted"),
                }
            };
            match staleness {
                0 => fill_stack_one_more_unchecked(nucleus),
                1 => source_mut(nucleus).address_space = fixture.source_as,
                _ => {
                    let other = nucleus.create_thread(fixture.source_as).unwrap();
                    nucleus.this_core.current_thread = Some(other);
                }
            }
            let before = snapshot(nucleus, &fixture);
            let error = nucleus
                .commit_return(prepared)
                .expect_err("stale Return description committed");
            assert_eq!(error.code(), CapError::InvalidOperation.code());
            assert_eq!(snapshot(nucleus, &fixture), before);
        });
    }
}

#[test_case]
fn dispatch_routes_call_and_return_from_saved_frames() {
    with_call(|nucleus, fixture| {
        let before = snapshot(nucleus, &fixture);
        let call_saved = call_frame(fixture.key);
        let call = match api::handle_cap_invoke(nucleus, &call_saved) {
            Ok(api::InvokeOutcome::Call(committed)) => committed,
            Ok(_) => panic!("Invocation.Call dispatched to a non-Call outcome"),
            Err(error) => panic!("dispatched Call rejected: {:?}", error.code()),
        };
        assert_eq!(call.target.sp, STACK_END);
        assert_eq!(call.translation.address_space(), fixture.target_as);
        assert_eq!(
            nucleus.current_thread_table_addr(),
            Some(fixture.target_table)
        );

        // The target's own Return frame, built from the scrubbed entry frame
        // as target code would leave it before its Return SVC.
        let mut return_saved = call.target;
        return_saved.gpr[0] = fixture.target_return_key.to_wire();
        return_saved.gpr[1] = 0;
        return_saved.gpr[2] = 0x1111;
        return_saved.gpr[3] = 0x2222;
        let returned = match api::handle_cap_invoke(nucleus, &return_saved) {
            Ok(api::InvokeOutcome::Return(committed)) => committed,
            Ok(_) => panic!("Thread.Return dispatched to a non-Return outcome"),
            Err(error) => panic!("dispatched Return rejected: {:?}", error.code()),
        };
        assert_eq!(&returned.resumed.gpr[0..3], &[0, 0x1111, 0x2222]);
        assert_eq!(returned.resumed.elr_el1, call_saved.elr_el1);
        assert_eq!(returned.resumed.spsr_el1, call_saved.spsr_el1);
        assert_eq!(returned.translation.address_space(), fixture.source_as);
        assert_eq!(snapshot(nucleus, &fixture), before);

        // An op-0 dispatch through an empty key is an ordinary lookup
        // rejection, never a Return fault.
        let saved = return_frame(fixture.source_return_key, [0; 2], [0; 4]);
        let mut empty_key = saved;
        empty_key.gpr[0] = RawKey::from_wire(0).to_wire();
        match api::handle_cap_invoke(nucleus, &empty_key) {
            Err(error) => assert_ne!(error.code().0, 0),
            Ok(_) => panic!("empty key dispatched successfully"),
        }
        assert_eq!(snapshot(nucleus, &fixture), before);
    });
}

// ═════════════════════════════
// FAULT DELIVERY
// ═════════════════════════════

/// The faulting instruction's PC in the fault tests.
const FAULT_PC: u64 = 0x9_1230;

/// The fault handler's pool: two stacks of `HANDLER_STACK_BYTES`.
const HANDLER_STACK_BASE: u64 = 0x40_0000;
const HANDLER_STACK_BYTES: u64 = 0x400;
/// The first handler stack's end.
const HANDLER_STACK_END: u64 = HANDLER_STACK_BASE + HANDLER_STACK_BYTES;
/// Where the handler's `AddressSpace` maps the fault areas.
const FAULT_AREA_VA: u64 = 0x50_0000;
/// Fault-area slots of the test buffer: two per handler pool, two pools.
const FAULT_AREA_SLOTS: usize = 4;

/// The memory of the test handler pools' fault areas. The unit tests run
/// without translation, so the buffer's address is its physical address and
/// the kernel writes it directly.
#[repr(C, align(4096))]
struct FaultAreaBytes(UnsafeCell<[u8; FAULT_AREA_SLOTS * FAULT_SLOT_BYTES as usize]>);

// SAFETY: the nucleus tests run on one core, one test at a time, and no
// reference into the cell outlives a single call.
unsafe impl Sync for FaultAreaBytes {}

static FAULT_AREA_BYTES: FaultAreaBytes = FaultAreaBytes(UnsafeCell::new(
    [0; FAULT_AREA_SLOTS * FAULT_SLOT_BYTES as usize],
));

/// The physical address of fault-area slot `slot` of the test buffer.
fn fault_slot_paddr(slot: usize) -> u64 {
    FAULT_AREA_BYTES.0.get() as u64 + slot as u64 * FAULT_SLOT_BYTES
}

/// The fault area of `slots` slots from slot `first` of the test buffer,
/// zeroed.
fn fault_area(first: usize, slots: usize) -> FaultArea {
    let offset = first * FAULT_SLOT_BYTES as usize;
    let length = slots * FAULT_SLOT_BYTES as usize;
    // SAFETY: see `FaultAreaBytes`.
    let buffer = unsafe { &mut *FAULT_AREA_BYTES.0.get() };
    buffer[offset..offset + length].fill(0);
    FaultArea {
        paddr: fault_slot_paddr(first),
        vaddr: FAULT_AREA_VA + offset as u64,
        bytes: length as u64,
    }
}

/// Install a fault handler Invocation (into the target `AddressSpace`, with a
/// pool of its own and a fault area) at `FAULT_HANDLER` in the current
/// Thread's table.
fn install_fault_handler(nucleus: &mut Nucleus<ArchObjectsImpl>, fixture: &CallFixture) {
    install_fault_handler_with(nucleus, fixture, Some(fault_area(0, 2)));
}

fn install_fault_handler_with(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    fixture: &CallFixture,
    fault_area: Option<FaultArea>,
) {
    let pool = nucleus
        .address_space_mut(fixture.target_as)
        .unwrap()
        .stack_pools_mut()
        .create(
            HANDLER_STACK_BASE,
            HANDLER_STACK_BYTES,
            2,
            fault_area,
            ArchObjectsImpl::USER_VA_END,
        )
        .unwrap_or_else(|error| panic!("handler stack pool: {:?}", error.code()));
    nucleus
        .current_thread_table_mut()
        .unwrap()
        .insert(
            KeySlot::FAULT_HANDLER,
            KeyEntry::new_invocation(
                fixture.target_as,
                NonZero::new(FUNCTION).unwrap(),
                pool,
                PlacementWindow::NONE,
            ),
            FIXTURE_GUARD,
        )
        .unwrap_or_else(|error| panic!("fault handler install: {:?}", error.error.code()));
}

/// The fault frame as it is in the first stack's fault slot.
fn frame_in_slot() -> FaultFrame {
    let mut bytes = [0; FAULT_FRAME_BYTES];
    // SAFETY: see `FaultAreaBytes`.
    let buffer = unsafe { &*FAULT_AREA_BYTES.0.get() };
    bytes.copy_from_slice(&buffer[..FAULT_FRAME_BYTES]);
    FaultFrame::from_bytes(bytes)
}

/// Replace the fault frame in the first stack's fault slot, as a handler
/// would.
fn rewrite_frame(frame: &FaultFrame) {
    // SAFETY: see `FaultAreaBytes`.
    let buffer = unsafe { &mut *FAULT_AREA_BYTES.0.get() };
    buffer[..FAULT_FRAME_BYTES].copy_from_slice(&frame.to_bytes());
}

/// A faulting EL0 frame with distinct values in every register.
fn faulting_frame() -> SavedContext {
    let mut saved = SavedContext::el0(FAULT_PC, STACK_END - 0x20, 0);
    for (index, register) in saved.gpr.iter_mut().enumerate() {
        *register = 0xFA00 + u64::try_from(index).unwrap();
    }
    saved.lr = 0xFA_0030;
    saved.spsr_el1 |= 0x2000_0000;
    saved.tpidr_el0 = 0x7150_FA17;
    saved
}

fn fault_info(depth: u64) -> FaultInfo {
    FaultInfo {
        kind: FaultKind::CpuException,
        esr: 0xF200_0007,
        far: 0xDEAD_0000,
        pc: FAULT_PC,
        sp: STACK_END - 0x20,
        depth,
    }
}

fn deliver(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    faulting: SavedContext,
    info: FaultInfo,
) -> FaultDelivery {
    // SAFETY: serial fixture with exclusive nucleus/pool backing; no other
    // access context or object/table guard overlaps this transaction.
    let access = unsafe { Access::new() };
    nucleus
        .deliver_fault(&access, faulting, info)
        .unwrap_or_else(|error| panic!("fault delivery failed: {:?}", error.code()))
}

fn handler_state(nucleus: &Nucleus<ArchObjectsImpl>, address_space: AddressSpaceId) -> (bool, u64) {
    let space = nucleus.address_space(address_space).unwrap();
    (space.fault_handler_busy(), space.unhandled_faults())
}

/// Queue a never-run Thread so an unhandled fault has somewhere to switch.
fn queue_next_thread(nucleus: &mut Nucleus<ArchObjectsImpl>, fixture: &CallFixture) -> ThreadId {
    let next = nucleus.create_thread(fixture.source_as).unwrap();
    nucleus.thread_mut(next).unwrap().context = ExecutionContext::NotStarted {
        saved: SavedContext::el0(0xA_0000, STACK_END, 0),
    };
    assert!(nucleus.local_activations().push(next));
    next
}

fn expect_unhandled(
    delivery: FaultDelivery,
    nucleus: &Nucleus<ArchObjectsImpl>,
    faulted: ThreadId,
    faulting: SavedContext,
    next: ThreadId,
) {
    let FaultDelivery::Unhandled(switch) = delivery else {
        panic!("the fault must be unhandled");
    };
    let resumed = switch.expect_resume();
    assert_eq!(resumed.current, Some(faulted));
    assert_eq!(resumed.next, next);
    let thread = nucleus.thread(faulted).unwrap();
    assert_eq!(
        thread.context,
        ExecutionContext::Faulted { saved: faulting }
    );
    assert_eq!(thread.fault, None);
    assert_eq!(nucleus.this_core.current_thread, Some(next));
}

#[test_case]
fn fault_enters_the_handler_with_the_fault_words_at_its_stack_top() {
    with_call(|nucleus, fixture| {
        install_fault_handler(nucleus, &fixture);
        let faulting = faulting_frame();
        let info = fault_info(0);
        let FaultDelivery::Handler(committed) = deliver(nucleus, faulting, info) else {
            panic!("the fault must reach the handler");
        };
        let target = committed.target;
        assert_eq!(&target.gpr[2..8], &info.to_arguments());
        assert_eq!(
            &target.gpr[0..2],
            &[FAULT_AREA_VA, 0],
            "x0 is the fault frame, in the first stack's fault slot"
        );
        assert!(target.gpr[8..].iter().all(|&register| register == 0));
        assert_eq!(
            target.sp, HANDLER_STACK_END,
            "the handler starts at its stack top: the stack stays untouched"
        );
        // The frame holds the faulting registers, and no FP part: the
        // faulting AddressSpace is integer-only.
        let frame = frame_in_slot();
        assert_eq!((frame.size, frame.version), (832, FAULT_FRAME_VERSION));
        assert_eq!(frame.flags, 0);
        assert_eq!(&frame.gpr[..30], &faulting.gpr);
        assert_eq!(frame.gpr[30], faulting.lr);
        assert_eq!(
            (frame.sp, frame.pc, frame.pstate, frame.tpidr_el0),
            (
                faulting.sp,
                faulting.elr_el1,
                faulting.spsr_el1,
                faulting.tpidr_el0
            )
        );
        assert!(frame.vectors.iter().all(|&register| register == 0));
        assert_eq!(target.elr_el1, FUNCTION);
        assert_eq!(
            target.origin, faulting.origin,
            "the handler runs at the faulting EL"
        );
        assert_eq!(target.tpidr_el0, 0);
        assert_eq!(committed.translation.address_space(), fixture.target_as);

        let thread = source_mut(nucleus);
        assert_eq!(thread.address_space, fixture.target_as);
        assert_eq!(thread.invocation_stack.len(), 1);
        assert_eq!(
            thread.fault,
            Some(ThreadFault {
                frame: faulting,
                address_space: fixture.source_as,
                depth: 1,
                frame_paddr: fault_slot_paddr(0),
            })
        );
        assert_eq!(handler_state(nucleus, fixture.source_as), (true, 0));
    });
}

#[test_case]
fn a_handler_may_change_the_frame_and_retry_resumes_from_it() {
    with_call(|nucleus, fixture| {
        install_fault_handler(nucleus, &fixture);
        let faulting = faulting_frame();
        let FaultDelivery::Handler(_) = deliver(nucleus, faulting, fault_info(0)) else {
            panic!("the fault must reach the handler");
        };
        // The handler emulates the instruction: a result in x0, the PC past
        // it, and the condition flags it sets.
        let mut frame = frame_in_slot();
        frame.gpr[0] = 0x0E_0001;
        frame.pc = FAULT_PC + 4;
        frame.pstate = (frame.pstate & !0xF000_0000) | 0x8000_0000;
        rewrite_frame(&frame);
        let saved = return_frame(
            fixture.target_return_key,
            [FaultAction::Retry as u64, 0],
            [0; 4],
        );
        let committed = returned(thread_return(nucleus, &saved));
        assert!(!committed.terminate);
        let resumed = committed.resumed;
        assert_eq!(resumed.gpr[0], 0x0E_0001);
        assert_eq!(resumed.elr_el1, FAULT_PC + 4);
        assert_eq!(resumed.spsr_el1, frame.pstate);
        assert_eq!(&resumed.gpr[1..], &faulting.gpr[1..]);
        assert_eq!(resumed.origin, faulting.origin);
    });
}

#[test_case]
fn a_frame_that_would_leave_el0_or_lost_its_header_terminates() {
    // A changed exception level or interrupt mask is refused; so is a frame
    // whose header was overwritten.
    let refusals: [fn(&mut FaultFrame); 3] = [
        |frame| frame.pstate |= 0b0101, // EL1h
        |frame| frame.pstate |= 1 << 7, // IRQs masked
        |frame| frame.version = 0,
    ];
    for refuse in refusals {
        with_call(|nucleus, fixture| {
            install_fault_handler(nucleus, &fixture);
            let faulting = faulting_frame();
            let FaultDelivery::Handler(_) = deliver(nucleus, faulting, fault_info(0)) else {
                panic!("the fault must reach the handler");
            };
            let mut frame = frame_in_slot();
            refuse(&mut frame);
            rewrite_frame(&frame);
            let saved = return_frame(
                fixture.target_return_key,
                [FaultAction::Retry as u64, 0],
                [0; 4],
            );
            let committed = returned(thread_return(nucleus, &saved));
            assert!(committed.terminate, "a refused frame terminates");
            assert_eq!(
                committed.resumed, faulting,
                "the kernel's own copy is the faulted state"
            );
            assert_eq!(handler_state(nucleus, fixture.source_as), (false, 0));
        });
    }
}

#[test_case]
fn a_handler_pool_without_a_fault_area_leaves_the_fault_unhandled() {
    // The frame has nowhere to go: nothing commits.
    with_call(|nucleus, fixture| {
        install_fault_handler_with(nucleus, &fixture, None);
        let faulted = nucleus.this_core.current_thread.unwrap();
        let next = queue_next_thread(nucleus, &fixture);
        let faulting = faulting_frame();
        let delivery = deliver(nucleus, faulting, fault_info(0));
        expect_unhandled(delivery, nucleus, faulted, faulting, next);
        assert_eq!(handler_state(nucleus, fixture.source_as), (false, 1));
        let thread = nucleus.thread(faulted).unwrap();
        assert_eq!(thread.invocation_stack.len(), 0);
    });
}

#[test_case]
fn an_fp_fault_carries_its_registers_and_retry_restores_them() {
    with_call(|nucleus, fixture| {
        set_fp_frame(nucleus, fixture.source_as, true);
        set_fp_frame(nucleus, fixture.target_as, true);
        install_fault_handler(nucleus, &fixture);
        let thread = nucleus.this_core.current_thread.unwrap();
        let faulted_values = fp_tagged(0xF);
        FakeFpUnit::set_registers(faulted_values);
        let FaultDelivery::Handler(_) = deliver(nucleus, faulting_frame(), fault_info(0)) else {
            panic!("the fault must reach the handler");
        };
        // The handler starts zeroed, and finds the faulting registers in the
        // frame.
        assert_eq!(FakeFpUnit::registers(), FpState::ZERO);
        let mut frame = frame_in_slot();
        assert_eq!(frame.flags, FAULT_FRAME_FP);
        assert_eq!(frame.vectors, faulted_values.vectors);
        assert_eq!(
            (frame.fpcr, frame.fpsr),
            (faulted_values.fpcr, faulted_values.fpsr)
        );

        // It fixes one register; the handler's own values must not leak back.
        frame.vectors[3] = 0x3333;
        rewrite_frame(&frame);
        FakeFpUnit::set_registers(fp_tagged(0xE));
        let saved = return_frame(
            fixture.target_return_key,
            [FaultAction::Retry as u64, 0],
            [0; 4],
        );
        returned(thread_return(nucleus, &saved));
        let mut expected = faulted_values;
        expected.vectors[3] = 0x3333;
        assert_eq!(FakeFpUnit::registers(), expected);
        let stats = fp_stats(nucleus, thread);
        assert_eq!((stats.fp_saves, stats.fp_loads), (1, 1));
    });
}

#[test_case]
fn handler_return_actions_resume_or_terminate_and_free_the_handler() {
    for (action, expected_pc, terminate) in [
        (FaultAction::Retry as u64, FAULT_PC, false),
        (FaultAction::Skip as u64, FAULT_PC + 4, false),
        (FaultAction::Terminate as u64, FAULT_PC, true),
        // Unknown actions terminate.
        (7, FAULT_PC, true),
    ] {
        with_call(|nucleus, fixture| {
            install_fault_handler(nucleus, &fixture);
            let faulting = faulting_frame();
            let FaultDelivery::Handler(_) = deliver(nucleus, faulting, fault_info(0)) else {
                panic!("the fault must reach the handler");
            };
            let saved = return_frame(fixture.target_return_key, [action, 0x55], [1, 2, 3, 4]);
            let committed = returned(thread_return(nucleus, &saved));

            let mut expected = faulting;
            expected.elr_el1 = expected_pc;
            assert_eq!(committed.resumed, expected, "action {action}");
            assert_eq!(committed.terminate, terminate, "action {action}");
            assert_eq!(committed.translation.address_space(), fixture.source_as);
            let thread = source_mut(nucleus);
            assert_eq!(thread.fault, None);
            assert_eq!(thread.address_space, fixture.source_as);
            assert_eq!(thread.invocation_stack.len(), 0);
            assert_eq!(handler_state(nucleus, fixture.source_as), (false, 0));
        });
    }
}

#[test_case]
fn an_ordinary_return_inside_the_handler_does_not_end_the_fault() {
    with_call(|nucleus, fixture| {
        install_fault_handler(nucleus, &fixture);
        let FaultDelivery::Handler(_) = deliver(nucleus, faulting_frame(), fault_info(0)) else {
            panic!("the fault must reach the handler");
        };
        // The handler makes its own Call and Returns from it: depth 2 → 1.
        committed(call(nucleus, &fixture, &call_frame(fixture.key)));
        let saved = return_frame(fixture.target_return_key, [0xAA, 0xBB], [1, 2, 3, 4]);
        let committed = returned(thread_return(nucleus, &saved));
        assert!(!committed.terminate);
        assert_eq!(committed.resumed.gpr[1], 0xAA, "an ordinary Return result");
        let thread = source_mut(nucleus);
        assert!(thread.fault.is_some(), "the fault is still being handled");
        assert_eq!(thread.invocation_stack.len(), 1);
        assert_eq!(handler_state(nucleus, fixture.source_as), (true, 0));
    });
}

#[test_case]
fn a_fault_without_a_handler_parks_the_thread_and_counts_it() {
    with_call(|nucleus, fixture| {
        let faulted = nucleus.this_core.current_thread.unwrap();
        let next = queue_next_thread(nucleus, &fixture);
        let faulting = faulting_frame();
        let delivery = deliver(nucleus, faulting, fault_info(0));
        expect_unhandled(delivery, nucleus, faulted, faulting, next);
        assert_eq!(handler_state(nucleus, fixture.source_as), (false, 1));
    });
}

#[test_case]
fn a_fault_while_the_handler_is_busy_is_unhandled() {
    with_call(|nucleus, fixture| {
        install_fault_handler(nucleus, &fixture);
        nucleus
            .address_space_mut(fixture.source_as)
            .unwrap()
            .set_fault_handler_busy(true);
        let faulted = nucleus.this_core.current_thread.unwrap();
        let next = queue_next_thread(nucleus, &fixture);
        let faulting = faulting_frame();
        let delivery = deliver(nucleus, faulting, fault_info(0));
        expect_unhandled(delivery, nucleus, faulted, faulting, next);
        // Still busy: the handler belongs to whoever holds it.
        assert_eq!(handler_state(nucleus, fixture.source_as), (true, 1));
    });
}

#[test_case]
fn a_fault_inside_the_handler_is_unhandled_and_frees_the_handler() {
    with_call(|nucleus, fixture| {
        install_fault_handler(nucleus, &fixture);
        let faulted = nucleus.this_core.current_thread.unwrap();
        let next = queue_next_thread(nucleus, &fixture);
        let FaultDelivery::Handler(_) = deliver(nucleus, faulting_frame(), fault_info(0)) else {
            panic!("the first fault must reach the handler");
        };
        // The handler (running in the target AddressSpace) faults itself.
        let mut nested = faulting_frame();
        nested.elr_el1 = FUNCTION + 0x10;
        let delivery = deliver(nucleus, nested, fault_info(1));
        expect_unhandled(delivery, nucleus, faulted, nested, next);
        // Counted where it happened; the held handler is free again.
        assert_eq!(handler_state(nucleus, fixture.target_as), (false, 1));
        assert_eq!(handler_state(nucleus, fixture.source_as), (false, 0));
    });
}

#[test_case]
fn a_fault_with_a_full_invocation_stack_is_unhandled() {
    with_call(|nucleus, fixture| {
        install_fault_handler(nucleus, &fixture);
        fill_stack(nucleus, INVOCATION_STACK_DEPTH);
        let faulted = nucleus.this_core.current_thread.unwrap();
        let next = queue_next_thread(nucleus, &fixture);
        let faulting = faulting_frame();
        let delivery = deliver(nucleus, faulting, fault_info(16));
        expect_unhandled(delivery, nucleus, faulted, faulting, next);
        assert_eq!(handler_state(nucleus, fixture.source_as), (false, 1));
    });
}

#[test_case]
fn releasing_a_thread_fault_frees_its_handler() {
    with_call(|nucleus, fixture| {
        install_fault_handler(nucleus, &fixture);
        let FaultDelivery::Handler(_) = deliver(nucleus, faulting_frame(), fault_info(0)) else {
            panic!("the fault must reach the handler");
        };
        let id = nucleus.this_core.current_thread.unwrap();
        nucleus.release_thread_fault(id);
        assert_eq!(source_mut(nucleus).fault, None);
        assert_eq!(handler_state(nucleus, fixture.source_as), (false, 0));
    });
}

fn commit(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    prepared: PreparedCall,
) -> Result<CommittedCall, CapError> {
    // SAFETY: as in `call`: this fixture is serial and owns exclusive
    // nucleus/pool backing, with no overlapping access context or guard.
    let access = unsafe { Access::new() };
    nucleus.commit_call(&access, prepared)
}

// ═════════════════════════════
// CALL WITH KEY
// ═════════════════════════════

/// The exporter's placement window in each fixture table: slots 40 and 41.
const WINDOW_FIRST: u32 = 40;
const WINDOW_COUNT: u32 = 2;
/// Slot of the window-accepting Invocation in the source table.
const KEYED_INVOCATION_SLOT: KeySlot = KeySlot(30);

/// The first pool the fixture's target `AddressSpace` made.
fn fixture_pool() -> StackPoolHandle {
    StackPoolHandle::new(0, 1)
}

fn install_entry(table_address: u64, slot: KeySlot, entry: KeyEntry) -> RawKey {
    // SAFETY: serial fixture with exclusive nucleus/pool backing; no other
    // access context or table guard overlaps this installation.
    let access = unsafe { Access::new() };
    let mut table = access
        .resolve_object::<KeyTable>(super::table_id(table_address))
        .unwrap_or_else(|error| panic!("fixture table: {error:?}"));
    table
        .insert(slot, entry, FIXTURE_GUARD)
        .unwrap_or_else(|failure| panic!("install: {:?}", failure.error.code()))
}

/// The entry `key` names in the table at `table_address`, by value.
fn entry_in(table_address: u64, key: RawKey) -> Result<KeyEntry, CapError> {
    // SAFETY: as in `install_entry`.
    let access = unsafe { Access::new() };
    let table = access
        .resolve_object::<KeyTable>(super::table_id(table_address))
        .unwrap_or_else(|error| panic!("fixture table: {error:?}"));
    table.lookup(key, FIXTURE_GUARD).copied()
}

fn remove_entry(table_address: u64, key: RawKey) -> KeyEntry {
    // SAFETY: as in `install_entry`.
    let access = unsafe { Access::new() };
    let mut table = access
        .resolve_object::<KeyTable>(super::table_id(table_address))
        .unwrap_or_else(|error| panic!("fixture table: {error:?}"));
    table
        .remove(key, FIXTURE_GUARD)
        .unwrap_or_else(|error| panic!("remove: {:?}", error.code()))
}

/// A Frame key that may be lent, and the same Frame without `LEND`.
fn lendable_frame() -> KeyEntry {
    KeyEntry::new_frame(
        0x20_0000,
        12,
        false,
        Rights(Rights::READ | Rights::WRITE | Rights::LEND),
    )
}

fn unlendable_frame() -> KeyEntry {
    KeyEntry::new_frame(0x20_0000, 12, false, Rights(Rights::READ | Rights::WRITE))
}

/// The key the callee receives for the borrowed entry in window slot `index`
/// of a fixture table, at `incarnation`.
fn placed_key(index: u32, incarnation: u32) -> RawKey {
    RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, WINDOW_FIRST + index, incarnation)
}

/// An Invocation into the target AddressSpace that accepts one lent key at a
/// time in a two-slot window, installed in the source table.
fn keyed_invocation(fixture: &CallFixture) -> RawKey {
    install_entry(
        fixture.source_table,
        KEYED_INVOCATION_SLOT,
        KeyEntry::new_invocation(
            fixture.target_as,
            NonZero::new(FUNCTION).unwrap(),
            fixture_pool(),
            PlacementWindow::new(WINDOW_FIRST, WINDOW_COUNT),
        ),
    )
}

/// A `CallWithKey` frame: operation 1 and the lent key in `x8`.
fn keyed_frame(invocation: RawKey, lent: RawKey) -> SavedContext {
    let mut saved = call_frame(invocation);
    saved.gpr[1] = 1;
    saved.gpr[8] = lent.to_wire();
    saved
}

/// Admit and commit `saved` from the table the Thread currently runs in.
fn call_current(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    saved: &SavedContext,
) -> Result<CommittedCall, CapError> {
    let caller = current_caller(nucleus);
    // SAFETY: as in `call`.
    let access = unsafe { Access::new() };
    api::invocation::call(&access, caller, saved, nucleus)
}

fn assert_borrowed(table_address: u64, key: RawKey, original: &KeyEntry) {
    let borrowed = entry_in(table_address, key)
        .unwrap_or_else(|error| panic!("borrowed entry: {:?}", error.code()));
    assert!(borrowed.is_lent());
    assert_eq!(borrowed.object_type(), original.object_type());
    assert_eq!(borrowed.rights(), original.rights());
    assert!(borrowed.rights().has(Rights::LEND));
    assert_eq!(borrowed.badge(), original.badge());
}

/// The slot of `key` has never held an entry: nothing was ever placed there.
fn assert_never_issued(table_address: u64, key: RawKey) {
    assert!(matches!(
        entry_in(table_address, key),
        Err(CapError::InvalidKey {
            reason: InvalidKeyReason::NeverIssued,
            ..
        })
    ));
}

fn assert_vacant(table_address: u64, key: RawKey) {
    assert!(matches!(
        entry_in(table_address, key),
        Err(CapError::InconsistentKey {
            reason: InconsistencyReason::CapabilityInvalidated,
            ..
        })
    ));
}

#[test_case]
fn call_with_key_places_the_borrowed_entry_and_return_removes_it() {
    with_call(|nucleus, fixture| {
        let invocation = keyed_invocation(&fixture);
        let original = lendable_frame();
        let lent = install_entry(fixture.source_table, KeySlot(20), original);

        let committed = committed(call(nucleus, &fixture, &keyed_frame(invocation, lent)));
        assert!(committed.with_key);
        // The callee receives the borrowed entry's key in x0; x1 stays zero
        // and the six arguments are untouched.
        let placed = placed_key(0, 1);
        assert_eq!(committed.target.gpr[0], placed.to_wire());
        assert_eq!(committed.target.gpr[1], 0);
        assert_eq!(committed.target.gpr[2], 0x5A00 + 2);
        assert_eq!(committed.target.gpr[7], 0x5A00 + 7);
        assert_eq!(committed.target.gpr[8], 0);
        assert_borrowed(fixture.target_table, placed, &original);
        // The lender keeps its own key, unchanged and not lent.
        let kept = entry_in(fixture.source_table, lent).must("lender's key");
        assert!(!kept.is_lent());
        assert_eq!(kept.rights(), original.rights());
        // The record remembers where the entry went.
        let record = *source_mut(nucleus).invocation_stack.top().unwrap();
        assert_eq!(
            record.placement,
            Placement {
                slot: WINDOW_FIRST,
                incarnation: 1
            }
        );

        let saved = return_frame(fixture.target_return_key, [7, 8], [0; 4]);
        let returned = returned(thread_return(nucleus, &saved));
        assert_eq!(returned.placement, record.placement);
        assert_eq!(returned.resumed.gpr[1], 7);
        assert_vacant(fixture.target_table, placed);
        assert!(source_mut(nucleus).invocation_stack.is_empty());
        entry_in(fixture.source_table, lent).must("lender's key after Return");

        // The slot's incarnation advanced: the next call issues incarnation 2,
        // and the earlier key stays stale.
        let again = committed_call_with(nucleus, &fixture, invocation, lent);
        assert_eq!(again.target.gpr[0], placed_key(0, 2).to_wire());
        assert!(matches!(
            entry_in(fixture.target_table, placed),
            Err(CapError::InconsistentKey {
                reason: InconsistencyReason::SlotIncarnationMismatch,
                ..
            })
        ));
        assert_borrowed(fixture.target_table, placed_key(0, 2), &original);
    });
}

fn committed_call_with(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    fixture: &CallFixture,
    invocation: RawKey,
    lent: RawKey,
) -> CommittedCall {
    committed(call(nucleus, fixture, &keyed_frame(invocation, lent)))
}

#[test_case]
fn dispatched_call_with_key_commits_through_the_production_entry() {
    with_call(|nucleus, fixture| {
        let invocation = keyed_invocation(&fixture);
        let lent = install_entry(fixture.source_table, KeySlot(20), lendable_frame());
        let saved = keyed_frame(invocation, lent);
        match api::handle_cap_invoke(nucleus, &saved) {
            Ok(api::InvokeOutcome::Call(committed)) => {
                assert!(committed.with_key);
                assert_eq!(committed.target.gpr[0], placed_key(0, 1).to_wire());
            }
            Ok(_) => panic!("dispatched CallWithKey produced a non-Call outcome"),
            Err(error) => panic!("dispatched CallWithKey rejected: {:?}", error.code()),
        }
        assert_eq!(source_mut(nucleus).address_space, fixture.target_as);
        entry_in(fixture.target_table, placed_key(0, 1)).must("borrowed entry");
    });
}

#[test_case]
fn plain_call_ignores_x8_and_places_nothing() {
    with_call(|nucleus, fixture| {
        let invocation = keyed_invocation(&fixture);
        let lent = install_entry(fixture.source_table, KeySlot(20), lendable_frame());
        // A plain Call on an Invocation that accepts keys: x8 is just garbage.
        let mut saved = call_frame(invocation);
        saved.gpr[8] = lent.to_wire();
        let committed = committed(call(nucleus, &fixture, &saved));
        assert!(!committed.with_key);
        assert_eq!(committed.target.gpr[0], 0);
        assert_never_issued(fixture.target_table, placed_key(0, 1));
        assert_eq!(
            source_mut(nucleus)
                .invocation_stack
                .top()
                .unwrap()
                .placement,
            Placement::NONE
        );
    });
}

#[test_case]
fn call_with_key_rejections_preserve_every_table() {
    with_call(|nucleus, fixture| {
        let invocation = keyed_invocation(&fixture);
        let lent = install_entry(fixture.source_table, KeySlot(20), lendable_frame());
        let unlendable = install_entry(fixture.source_table, KeySlot(21), unlendable_frame());
        let never = RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, 200, 1);

        // The Invocation accepts no key (empty window).
        reject_preserved(
            nucleus,
            &fixture,
            &keyed_frame(fixture.key, lent),
            CapError::KeyNotAccepted,
        );
        // A key without LEND.
        reject_preserved(
            nucleus,
            &fixture,
            &keyed_frame(invocation, unlendable),
            CapError::InsufficientRights,
        );
        // A kind that cannot be lent (an Invocation).
        reject_preserved(
            nucleus,
            &fixture,
            &keyed_frame(invocation, fixture.key),
            CapError::InvalidObjectType(ObjectType::INVOCATION),
        );
        // A key that was never issued is reported against operand x8.
        reject_preserved(
            nucleus,
            &fixture,
            &keyed_frame(invocation, never),
            CapError::InvalidKey {
                key: never,
                reason: InvalidKeyReason::NeverIssued,
                operand: 8,
            },
        );
        // A key from another table's guard is rejected the same way.
        let foreign = RawKey::from_parts(FIXTURE_GUARD ^ 1, SIZE_BITS, 20, lent.incarnation());
        reject_preserved(
            nucleus,
            &fixture,
            &keyed_frame(invocation, foreign),
            CapError::InvalidKey {
                key: foreign,
                reason: InvalidKeyReason::GuardMismatch,
                operand: 8,
            },
        );
        // An exporter whose table no longer names itself cannot receive the
        // key. The caller holds only CALL on it, so it learns NotBound, not
        // the exporter's broken self slot.
        let target_self = RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, KeySlot::SELF_KEYTABLE.0, 1);
        // SAFETY: the fixture's target table is a live private carve; no
        // dispatch runs during this borrow.
        unsafe { &mut *KeyTable::at_carve(fixture.target_table) }
            .remove(target_self, FIXTURE_GUARD)
            .unwrap_or_else(|_| panic!("target self-table removal failed"));
        reject_preserved(
            nucleus,
            &fixture,
            &keyed_frame(invocation, lent),
            CapError::NotBound,
        );
    });
}

#[test_case]
fn call_with_key_admission_order_is_pinned() {
    with_call(|nucleus, fixture| {
        let invocation = keyed_invocation(&fixture);
        let lent = install_entry(fixture.source_table, KeySlot(20), lendable_frame());
        let unlendable = install_entry(fixture.source_table, KeySlot(21), unlendable_frame());
        let never = RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, 200, 1);
        // The pool has no free stack for the whole sequence below.
        exhaust_pool(nucleus, &fixture);

        // KeyNotAccepted precedes any lent-key error.
        reject_preserved(
            nucleus,
            &fixture,
            &keyed_frame(fixture.key, never),
            CapError::KeyNotAccepted,
        );
        // Lent-key errors (lookup, then rights) precede the stack check.
        reject_preserved(
            nucleus,
            &fixture,
            &keyed_frame(invocation, never),
            CapError::InvalidKey {
                key: never,
                reason: InvalidKeyReason::NeverIssued,
                operand: 8,
            },
        );
        reject_preserved(
            nucleus,
            &fixture,
            &keyed_frame(invocation, unlendable),
            CapError::InsufficientRights,
        );
        // A full window is reported last: the stack and depth errors win.
        install_entry(
            fixture.target_table,
            KeySlot(WINDOW_FIRST),
            lendable_frame(),
        );
        install_entry(
            fixture.target_table,
            KeySlot(WINDOW_FIRST + 1),
            lendable_frame(),
        );
        reject_preserved(
            nucleus,
            &fixture,
            &keyed_frame(invocation, lent),
            no_free_stack(),
        );
        release_pool(nucleus, &fixture);
        reject_preserved(
            nucleus,
            &fixture,
            &keyed_frame(invocation, lent),
            CapError::NoPlacementSlot,
        );
        fill_stack(nucleus, INVOCATION_STACK_DEPTH);
        reject_preserved(
            nucleus,
            &fixture,
            &keyed_frame(invocation, lent),
            CapError::NestingDepth {
                count: INVOCATION_STACK_DEPTH as u64,
            },
        );
    });
}

#[test_case]
fn placement_skips_occupied_slots_and_fills_the_window_in_order() {
    with_call(|nucleus, fixture| {
        let invocation = keyed_invocation(&fixture);
        let original = lendable_frame();
        let lent = install_entry(fixture.source_table, KeySlot(20), original);
        // The exporter's own key sits in the first window slot.
        install_entry(
            fixture.target_table,
            KeySlot(WINDOW_FIRST),
            unlendable_frame(),
        );

        let first = committed_call_with(nucleus, &fixture, invocation, lent);
        assert_eq!(first.target.gpr[0], placed_key(1, 1).to_wire());
        assert_borrowed(fixture.target_table, placed_key(1, 1), &original);
        // The window is now full for a concurrent second call.
        let (result, before) = prepare(nucleus, &fixture, &keyed_frame(invocation, lent));
        assert_eq!(
            result.expect_err("full window").code(),
            CapError::NoPlacementSlot.code()
        );
        assert_eq!(snapshot(nucleus, &fixture), before);
        // The callee's own entry was never touched.
        assert!(
            !entry_in(fixture.target_table, placed_key(0, 1))
                .must("exporter's key")
                .is_lent()
        );
    });
}

#[test_case]
fn return_tolerates_a_callee_that_deleted_or_replaced_the_borrowed_entry() {
    for replace in [false, true] {
        with_call(|nucleus, fixture| {
            let invocation = keyed_invocation(&fixture);
            let lent = install_entry(fixture.source_table, KeySlot(20), lendable_frame());
            let committed = committed_call_with(nucleus, &fixture, invocation, lent);
            let placed = RawKey::from_wire(committed.target.gpr[0]);
            remove_entry(fixture.target_table, placed);
            let replacement = if replace {
                // The callee puts a key of its own into the freed slot: a new
                // incarnation, which Return must leave alone.
                Some(install_entry(
                    fixture.target_table,
                    KeySlot(WINDOW_FIRST),
                    unlendable_frame(),
                ))
            } else {
                None
            };
            let saved = return_frame(fixture.target_return_key, [1, 2], [0; 4]);
            returned(thread_return(nucleus, &saved));
            match replacement {
                Some(key) => {
                    let kept = entry_in(fixture.target_table, key).must("callee's own key");
                    assert!(!kept.is_lent());
                }
                None => assert_vacant(fixture.target_table, placed),
            }
            assert!(source_mut(nucleus).invocation_stack.is_empty());
        });
    }
}

#[test_case]
fn a_borrowed_entry_can_be_lent_onward_without_copies() {
    with_call(|nucleus, fixture| {
        let invocation = keyed_invocation(&fixture);
        let original = lendable_frame();
        let lent = install_entry(fixture.source_table, KeySlot(20), original);
        let first = committed_call_with(nucleus, &fixture, invocation, lent);
        let borrowed = RawKey::from_wire(first.target.gpr[0]);

        // From inside the target, call back into the source AddressSpace (a
        // second exporter with its own window at slots 60 and 61), lending the
        // borrowed entry itself.
        let onward = install_entry(
            fixture.target_table,
            KeySlot(50),
            KeyEntry::new_invocation(
                fixture.source_as,
                NonZero::new(FUNCTION).unwrap(),
                fixture_pool(),
                PlacementWindow::new(60, 2),
            ),
        );
        let saved = keyed_frame(onward, borrowed);
        let second = committed(call_current(nucleus, &saved));
        assert!(second.with_key);
        let second_key = RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, 60, 1);
        assert_eq!(second.target.gpr[0], second_key.to_wire());
        assert_borrowed(fixture.source_table, second_key, &original);
        assert_eq!(source_mut(nucleus).invocation_stack.len(), 2);

        // Returning from the second callee removes only its own entry.
        let inner = return_frame(fixture.source_return_key, [1, 2], [0; 4]);
        returned(thread_return(nucleus, &inner));
        assert_vacant(fixture.source_table, second_key);
        assert_borrowed(fixture.target_table, borrowed, &original);
        // Returning from the first removes the first.
        let outer = return_frame(fixture.target_return_key, [3, 4], [0; 4]);
        returned(thread_return(nucleus, &outer));
        assert_vacant(fixture.target_table, borrowed);
        entry_in(fixture.source_table, lent).must("the lender's key throughout");
    });
}

#[test_case]
fn thread_teardown_removes_every_outstanding_borrowed_entry() {
    with_call(|nucleus, fixture| {
        let invocation = keyed_invocation(&fixture);
        let lent = install_entry(fixture.source_table, KeySlot(20), lendable_frame());
        let first = committed_call_with(nucleus, &fixture, invocation, lent);
        let borrowed = RawKey::from_wire(first.target.gpr[0]);
        let onward = install_entry(
            fixture.target_table,
            KeySlot(50),
            KeyEntry::new_invocation(
                fixture.source_as,
                NonZero::new(FUNCTION).unwrap(),
                fixture_pool(),
                PlacementWindow::new(60, 2),
            ),
        );
        let second = committed(call_current(nucleus, &keyed_frame(onward, borrowed)));
        let second_key = RawKey::from_wire(second.target.gpr[0]);
        entry_in(fixture.target_table, borrowed).must("first borrowed entry");
        entry_in(fixture.source_table, second_key).must("second borrowed entry");

        let thread = nucleus.this_core.current_thread.unwrap();
        // SAFETY: serial fixture; no other access context or guard overlaps.
        let access = unsafe { Access::new() };
        nucleus.release_thread_placements(&access, thread);
        assert_vacant(fixture.target_table, borrowed);
        assert_vacant(fixture.source_table, second_key);
        // The lender's key is untouched, and a second release is a no-op.
        entry_in(fixture.source_table, lent).must("the lender's key");
        nucleus.release_thread_placements(&access, thread);
    });
}

#[test_case]
fn a_stale_prepared_call_with_key_installs_nothing() {
    with_call(|nucleus, fixture| {
        let invocation = keyed_invocation(&fixture);
        let lent = install_entry(fixture.source_table, KeySlot(20), lendable_frame());
        let (result, _) = prepare(nucleus, &fixture, &keyed_frame(invocation, lent));
        let prepared = admitted(result);
        // The depth changed after admission: commit refuses before touching
        // either table.
        fill_stack(nucleus, 1);
        let before = snapshot(nucleus, &fixture);
        let error = commit(nucleus, prepared).expect_err("stale CallWithKey committed");
        assert_eq!(error.code(), CapError::InvalidOperation.code());
        assert_eq!(snapshot(nucleus, &fixture), before);
        assert_never_issued(fixture.target_table, placed_key(0, 1));
    });
}

/// `expect` for results whose error type has no `Debug`: panics with the wire
/// code of the error.
trait Must<T> {
    fn must(self, what: &str) -> T;
}

impl<T> Must<T> for Result<T, CapError> {
    fn must(self, what: &str) -> T {
        self.unwrap_or_else(|error| panic!("{what}: {:?}", error.code()))
    }
}

#[test_case]
fn a_return_is_a_preemption_point_like_any_other_kernel_exit() {
    with_call(|nucleus, fixture| {
        fill_stack(nucleus, 2);
        let source = nucleus
            .this_core
            .current_thread
            .expect("the calling Thread");
        let call_saved = call_frame(fixture.key);
        committed(call(nucleus, &fixture, &call_saved));
        // While the callee ran, the dispatcher was activated (its limit
        // elapsed, say); the Thread keeps running until it leaves the kernel.
        nucleus.this_core.preempt_requested = true;
        let next = queue_next_thread(nucleus, &fixture);

        let saved = return_frame(fixture.target_return_key, [1, 2], [0; 4]);
        let commit = returned(thread_return(nucleus, &saved));
        assert_eq!(commit.source_thread, source);
        // The Return changes nothing about the pending preemption: the
        // timer is asked to fire at once, as after any other exit.
        assert!(nucleus.this_core.preempt_requested);
        let since = nucleus.this_core.current_since_ticks;
        assert_eq!(nucleus.timer_deadline_ticks(1_000_000), Some(since));

        // The tick right after the Return stops the caller in exactly the
        // state the Return resumed, and runs the next Thread.
        // SAFETY: serial fixture with exclusive nucleus/pool backing; no
        // other access context or object/table guard overlaps this call.
        let resumed = {
            let access = unsafe { Access::new() };
            nucleus
                .preempt_and_select(&access, commit.resumed)
                .unwrap_or_else(|error| panic!("tick: {:?}", error.code()))
                .expect("the queued Thread preempts")
                .expect_resume()
        };
        assert_eq!(resumed.next, next);
        assert_eq!(
            nucleus.thread(source).unwrap().context,
            ExecutionContext::Preempted {
                saved: commit.resumed
            }
        );
        assert!(!nucleus.this_core.preempt_requested);
    });
}

// ═════════════════════════════════════════════════════════════════
// FP/SIMD STATE ACROSS CALL, RETURN AND THREAD SWITCHES
// ═════════════════════════════════════════════════════════════════

/// A register file in which every register holds a distinct value marked
/// with `tag`, so a test can tell whose values the registers hold.
fn fp_tagged(tag: u128) -> FpState {
    let mut state = FpState::ZERO;
    for (index, register) in state.vectors.iter_mut().enumerate() {
        *register = tag << 96 | tag << 32 | (u128::try_from(index).unwrap() + 1);
    }
    state.fpcr = 0x0040_0000;
    state.fpsr = u32::try_from(tag & 0x9f).unwrap();
    state
}

fn set_fp_frame(nucleus: &mut Nucleus<ArchObjectsImpl>, address_space: AddressSpaceId, fp: bool) {
    nucleus
        .address_space_mut(address_space)
        .unwrap()
        .set_fp_simd_for_test(fp);
}

fn fp_stats(nucleus: &Nucleus<ArchObjectsImpl>, thread: ThreadId) -> ThreadFpCounters {
    nucleus.thread(thread).unwrap().fp_stats
}

/// The top call record of the current Thread.
fn top_record(nucleus: &mut Nucleus<ArchObjectsImpl>) -> InvocationContinuation {
    *source_mut(nucleus).invocation_stack.top().unwrap()
}

#[test_case]
fn a_call_between_fp_frames_hides_each_side_from_the_other() {
    with_call(|nucleus, fixture| {
        set_fp_frame(nucleus, fixture.source_as, true);
        set_fp_frame(nucleus, fixture.target_as, true);
        let thread = nucleus.this_core.current_thread.unwrap();
        let caller = fp_tagged(0xC);
        FakeFpUnit::set_registers(caller);

        committed(call(nucleus, &fixture, &call_frame(fixture.key)));
        assert_eq!(
            FakeFpUnit::registers(),
            FpState::ZERO,
            "the target must start with none of its caller's values"
        );
        let record = top_record(nucleus);
        assert!(record.fp_preserved);
        assert_eq!(record.source_fp, caller.preserved());

        // The callee computes with its own values, then returns.
        FakeFpUnit::set_registers(fp_tagged(0xE));
        let saved = return_frame(fixture.target_return_key, [1, 2], [0; 4]);
        returned(thread_return(nucleus, &saved));
        assert_eq!(
            FakeFpUnit::registers(),
            FpState::from_preserved(&caller.preserved()),
            "the caller gets V8-V15, FPCR and FPSR back and zeros elsewhere"
        );
        let stats = fp_stats(nucleus, thread);
        assert_eq!(
            (
                stats.preserved_saves,
                stats.fp_entries,
                stats.fp_zeroes,
                stats.fp_saves,
                stats.fp_loads
            ),
            (1, 2, 1, 0, 0)
        );
    });
}

#[test_case]
fn an_integer_target_leaves_the_registers_alone_and_return_restores_the_caller() {
    with_call(|nucleus, fixture| {
        set_fp_frame(nucleus, fixture.source_as, true);
        let thread = nucleus.this_core.current_thread.unwrap();
        let caller = fp_tagged(0xC);
        FakeFpUnit::set_registers(caller);

        committed(call(nucleus, &fixture, &call_frame(fixture.key)));
        // FP traps in the target, so nothing is written for it.
        assert_eq!(FakeFpUnit::registers(), caller);
        assert!(top_record(nucleus).fp_preserved);

        let saved = return_frame(fixture.target_return_key, [1, 2], [0; 4]);
        returned(thread_return(nucleus, &saved));
        assert_eq!(
            FakeFpUnit::registers(),
            FpState::from_preserved(&caller.preserved())
        );
        let stats = fp_stats(nucleus, thread);
        assert_eq!(
            (stats.preserved_saves, stats.fp_entries, stats.fp_zeroes),
            (1, 1, 0)
        );
    });
}

#[test_case]
fn an_fp_target_called_from_integer_code_starts_zeroed() {
    with_call(|nucleus, fixture| {
        set_fp_frame(nucleus, fixture.target_as, true);
        let thread = nucleus.this_core.current_thread.unwrap();
        // Whatever an earlier FP frame left in the registers.
        FakeFpUnit::set_registers(fp_tagged(0x5));

        committed(call(nucleus, &fixture, &call_frame(fixture.key)));
        assert_eq!(FakeFpUnit::registers(), FpState::ZERO);
        assert!(!top_record(nucleus).fp_preserved);

        // Back in the integer caller nothing is loaded: FP traps there, and
        // the next FP entry overwrites the whole register file.
        let callee = fp_tagged(0xE);
        FakeFpUnit::set_registers(callee);
        let saved = return_frame(fixture.target_return_key, [1, 2], [0; 4]);
        returned(thread_return(nucleus, &saved));
        assert_eq!(FakeFpUnit::registers(), callee);
        let stats = fp_stats(nucleus, thread);
        assert_eq!(
            (stats.preserved_saves, stats.fp_entries, stats.fp_zeroes),
            (0, 1, 1)
        );
    });
}

#[test_case]
fn calls_between_integer_frames_never_touch_the_registers() {
    with_call(|nucleus, fixture| {
        let thread = nucleus.this_core.current_thread.unwrap();
        let left = fp_tagged(0x5);
        FakeFpUnit::set_registers(left);
        committed(call(nucleus, &fixture, &call_frame(fixture.key)));
        assert!(!top_record(nucleus).fp_preserved);
        let saved = return_frame(fixture.target_return_key, [1, 2], [0; 4]);
        returned(thread_return(nucleus, &saved));
        assert_eq!(FakeFpUnit::registers(), left);
        assert_eq!(fp_stats(nucleus, thread), ThreadFpCounters::ZERO);
    });
}

#[test_case]
fn preempted_fp_threads_keep_their_own_registers() {
    with_call(|nucleus, fixture| {
        // Both Threads run in the source AddressSpace, which allows FP.
        set_fp_frame(nucleus, fixture.source_as, true);
        let first = nucleus.this_core.current_thread.unwrap();
        let second = queue_next_thread(nucleus, &fixture);
        let first_values = fp_tagged(0xA);
        FakeFpUnit::set_registers(first_values);
        let totals_before = nucleus.this_core.fp_totals;

        // A tick that preempts: the dispatcher asked for it. The nucleus keeps
        // no run queue, so the preempted Thread is queued again by the test,
        // as a dispatcher would.
        let tick = |nucleus: &mut Nucleus<ArchObjectsImpl>| {
            let preempted = nucleus.this_core.current_thread.unwrap();
            nucleus.this_core.preempt_requested = true;
            // SAFETY: serial fixture with exclusive nucleus/pool backing; no
            // other access context or object/table guard overlaps this call.
            let access = unsafe { Access::new() };
            let next = nucleus
                .preempt_and_select(&access, SavedContext::el0(0xA_0000, STACK_END, 0))
                .unwrap_or_else(|error| panic!("tick: {:?}", error.code()))
                .expect("the queued Thread preempts")
                .expect_resume()
                .next;
            assert!(nucleus.local_activations().push(preempted));
            next
        };

        // The second Thread never ran in an FP frame: it starts zeroed.
        assert_eq!(tick(nucleus), second);
        assert_eq!(nucleus.thread(first).unwrap().fp, first_values);
        assert_eq!(FakeFpUnit::registers(), FpState::ZERO);

        // It computes, and is preempted in turn: the first Thread gets its
        // own values back, and the second keeps its own.
        let second_values = fp_tagged(0xB);
        FakeFpUnit::set_registers(second_values);
        assert_eq!(tick(nucleus), first);
        assert_eq!(FakeFpUnit::registers(), first_values);
        assert_eq!(nucleus.thread(second).unwrap().fp, second_values);

        for thread in [first, second] {
            let stats = fp_stats(nucleus, thread);
            assert_eq!((stats.fp_saves, stats.fp_loads), (1, 1));
            assert_eq!(stats.fp_entries, 1);
        }
        assert_eq!(
            nucleus.this_core.fp_totals.thread_switches - totals_before.thread_switches,
            2
        );
        assert_eq!(
            nucleus.this_core.fp_totals.fp_thread_switches - totals_before.fp_thread_switches,
            2
        );
    });
}

/// Bounded model check of the FP/SIMD transitions (`simd-design.md`,
/// "Proof obligations"): two Threads, the fixture's two `AddressSpace`s S
/// and T with every combination of FP flags, calls nested up to
/// `MODEL_DEPTH`, and every sequence of `MODEL_STEPS` steps, each a Call into
/// the other `AddressSpace`, a Return, a preemption, a fault (handled in the
/// other `AddressSpace`, whose Return resumes the faulting frame) or the
/// running Thread's exit.
///
/// Before each step the running frame computes: an FP frame writes values no
/// other frame ever wrote. A reference model tracks what each frame is owed
/// (its own last values; zeros at a Call or fault entry; its preserved
/// registers with the rest zero after its call returns; its whole register
/// file after its fault is handled), and after every step the register file
/// of an FP frame must hold exactly that. This checks integrity and
/// confidentiality together: no frame ever sees another frame's values. The
/// `FPEN` gate (SIMD-14) needs the hardware switching of P-SIMD-2 and is not
/// modelled.
/// Blocking on a wait parks through the same save as a preemption and is not
/// a separate step.
#[test_case]
fn every_bounded_sequence_of_calls_returns_and_preemptions_keeps_fp_state() {
    // Thousands of fixtures in a row would exhaust the test arena, which
    // otherwise only grows. Nothing carved for one sequence is named again.
    let mark = crate::objects::nucleus::test_arena::mark();
    for flags in 0..4_u8 {
        for code in 0..MODEL_STEP_KINDS.pow(MODEL_STEPS) {
            run_model_sequence([flags & 1 != 0, flags & 2 != 0], code);
            crate::objects::nucleus::test_arena::rewind(mark);
        }
    }
}

const MODEL_STEPS: u32 = 5;
const MODEL_STEP_KINDS: u32 = 5;
const MODEL_DEPTH: usize = 3;

/// The model's view of one frame: the `AddressSpace` it runs in (0 for S,
/// 1 for T) and the register file it is owed.
#[derive(Clone, Copy)]
struct ModelFrame {
    side: usize,
    owed: FpState,
}

struct ModelThread {
    id: ThreadId,
    frames: [ModelFrame; MODEL_DEPTH + 1],
    depth: usize,
    /// The depth of the frame handling this Thread's fault, if any.
    fault_depth: Option<usize>,
    alive: bool,
}

impl ModelThread {
    fn top(&mut self) -> &mut ModelFrame {
        &mut self.frames[self.depth]
    }
}

/// Install `side`'s fault handler: an Invocation into the other
/// `AddressSpace`, with a pool of two `HANDLER_STACK_BYTES` stacks at `base`
/// and fault-area slots `2 * side` and `2 * side + 1`.
fn install_model_fault_handler(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    fixture: &CallFixture,
    side: usize,
    base: u64,
) {
    let (table, handler_as) = if side == 0 {
        (fixture.source_table, fixture.target_as)
    } else {
        (fixture.target_table, fixture.source_as)
    };
    let pool = nucleus
        .address_space_mut(handler_as)
        .unwrap()
        .stack_pools_mut()
        .create(
            base,
            HANDLER_STACK_BYTES,
            2,
            Some(fault_area(2 * side, 2)),
            ArchObjectsImpl::USER_VA_END,
        )
        .unwrap_or_else(|error| panic!("model handler pool: {:?}", error.code()));
    install_entry(
        table,
        KeySlot::FAULT_HANDLER,
        KeyEntry::new_invocation(
            handler_as,
            NonZero::new(FUNCTION).unwrap(),
            pool,
            PlacementWindow::NONE,
        ),
    );
}

/// Run the step sequence `code` (base-5 digits: 0 Call, 1 Return,
/// 2 preempt, 3 fault, 4 exit) with `fp[side]` the FP flag of S and T,
/// stopping at the first step the state does not allow.
fn run_model_sequence(fp: [bool; 2], mut code: u32) {
    with_call(|nucleus, fixture| {
        set_fp_frame(nucleus, fixture.source_as, fp[0]);
        set_fp_frame(nucleus, fixture.target_as, fp[1]);
        // From T, calls go back into S through this key.
        let back = install_entry(
            fixture.target_table,
            KeySlot(50),
            KeyEntry::new_invocation(
                fixture.source_as,
                NonZero::new(FUNCTION).unwrap(),
                fixture.pool,
                PlacementWindow::NONE,
            ),
        );
        // A fault in either AddressSpace goes to a handler in the other; the
        // two handler pools share the test fault-area buffer.
        install_model_fault_handler(nucleus, &fixture, 0, HANDLER_STACK_BASE);
        install_model_fault_handler(
            nucleus,
            &fixture,
            1,
            HANDLER_STACK_BASE + 2 * HANDLER_STACK_BYTES,
        );
        let call_keys = [fixture.key, back];
        let return_keys = [fixture.source_return_key, fixture.target_return_key];
        let base = ModelFrame {
            side: 0,
            owed: FpState::ZERO,
        };
        let first = nucleus.this_core.current_thread.unwrap();
        let second = queue_next_thread(nucleus, &fixture);
        let mut threads = [
            ModelThread {
                id: first,
                frames: [base; MODEL_DEPTH + 1],
                depth: 0,
                fault_depth: None,
                alive: true,
            },
            ModelThread {
                id: second,
                frames: [base; MODEL_DEPTH + 1],
                depth: 0,
                fault_depth: None,
                alive: true,
            },
        ];
        // The first Thread is running; whatever is in the registers is its.
        let mut running = 0;
        FakeFpUnit::set_registers(FpState::ZERO);
        let mut tag = 1_u128;

        for _ in 0..MODEL_STEPS {
            let step = code % MODEL_STEP_KINDS;
            code /= MODEL_STEP_KINDS;

            // The running frame computes.
            let thread = &mut threads[running];
            if fp[thread.top().side] {
                tag += 1;
                let values = fp_tagged(tag);
                FakeFpUnit::set_registers(values);
                thread.top().owed = values;
            }

            match step {
                0 => {
                    if thread.depth == MODEL_DEPTH {
                        return;
                    }
                    let side = thread.top().side;
                    let saved = call_frame(call_keys[side]);
                    committed(call_current(nucleus, &saved));
                    thread.depth += 1;
                    *thread.top() = ModelFrame {
                        side: 1 - side,
                        owed: FpState::ZERO,
                    };
                }
                1 => {
                    if thread.depth == 0 {
                        return;
                    }
                    let side = thread.top().side;
                    // Skip: a fault Return resumes the faulting frame after its
                    // instruction; an ordinary Return ignores the word.
                    let saved =
                        return_frame(return_keys[side], [FaultAction::Skip as u64, 2], [0; 4]);
                    returned(thread_return(nucleus, &saved));
                    let handled_fault = thread.fault_depth == Some(thread.depth);
                    thread.depth -= 1;
                    if handled_fault {
                        // The fault frame gave back the whole register file.
                        thread.fault_depth = None;
                    } else {
                        // The caller keeps V8-V15, FPCR and FPSR; the rest is
                        // zero.
                        let caller = thread.top();
                        caller.owed = FpState::from_preserved(&caller.owed.preserved());
                    }
                }
                3 => {
                    // One fault at a time: a second would find its handler
                    // busy and stop the Thread for good.
                    if thread.depth == MODEL_DEPTH
                        || threads.iter().any(|thread| thread.fault_depth.is_some())
                    {
                        return;
                    }
                    let thread = &mut threads[running];
                    let side = thread.top().side;
                    let info = fault_info(u64::try_from(thread.depth).unwrap());
                    let faulting = SavedContext::el0(FAULT_PC, STACK_END - 0x20, 0);
                    let FaultDelivery::Handler(_) = deliver(nucleus, faulting, info) else {
                        panic!("the model's fault must reach its handler");
                    };
                    thread.depth += 1;
                    thread.fault_depth = Some(thread.depth);
                    *thread.top() = ModelFrame {
                        side: 1 - side,
                        owed: FpState::ZERO,
                    };
                }
                4 => {
                    let other = 1 - running;
                    if !threads[other].alive {
                        return;
                    }
                    // SAFETY: serial fixture with exclusive nucleus/pool
                    // backing; no other access context or guard overlaps.
                    let access = unsafe { Access::new() };
                    let next = nucleus
                        .exit_current_thread(&access)
                        .unwrap_or_else(|error| panic!("exit: {:?}", error.code()))
                        .expect_resume()
                        .next;
                    threads[running].alive = false;
                    running = other;
                    assert_eq!(next, threads[running].id);
                }
                _ => {
                    if !threads[1 - running].alive {
                        return;
                    }
                    let thread = &mut threads[running];
                    let preempted = thread.id;
                    nucleus.this_core.preempt_requested = true;
                    // SAFETY: serial fixture with exclusive nucleus/pool
                    // backing; no other access context or guard overlaps.
                    let access = unsafe { Access::new() };
                    let next = nucleus
                        .preempt_and_select(&access, SavedContext::el0(0xA_0000, STACK_END, 0))
                        .unwrap_or_else(|error| panic!("tick: {:?}", error.code()))
                        .expect("the other Thread preempts")
                        .expect_resume()
                        .next;
                    assert!(nucleus.local_activations().push(preempted));
                    running = 1 - running;
                    assert_eq!(next, threads[running].id);
                }
            }

            // Whatever frame now runs, an FP frame sees exactly what it is owed.
            let frame = *threads[running].top();
            if fp[frame.side] {
                assert_eq!(
                    FakeFpUnit::registers(),
                    frame.owed,
                    "flags {fp:?}: an FP frame was not given what it is owed"
                );
            }
        }
    });
}

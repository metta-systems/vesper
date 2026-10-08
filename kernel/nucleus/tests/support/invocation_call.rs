//! `Invocation.Call` admission/preparation/commit and `Thread.Return` tests.
//! Roots below are checked metadata only; these tests never install TTBR0.

use {
    super::{FIXTURE_GUARD, SIZE_BITS, with_nucleus},
    crate::{
        api::{self, KeyEntry},
        objects::{
            ArchObjects, ArchObjectsImpl, ExecutionContext, InvocationContinuation,
            InvocationStack, KeyTable, Nucleus, ThreadFault,
            access::{Access, ObjectId},
            arch_objects::AddressSpaceObject,
            fault::FaultDelivery,
            invocation::{
                CallTarget, CommittedCall, CommittedReturn, InvocationStackExtent, PreparedCall,
                ReturnFault, ReturnRejection,
            },
            key_table::CallerTable,
        },
    },
    aarch64_cpu::registers::{Readable, SCTLR_EL1, TCR_EL1, TTBR0_EL1, Writeable},
    core::num::NonZero,
    libexception::arch::aarch64::SavedContext,
    libobject::{
        CapError, INVOCATION_STACK_DEPTH, InconsistencyReason, InvalidStackReason, KeySlot,
        ObjectType, RawKey, Rights,
        fault::{FaultAction, FaultInfo, FaultKind},
    },
};

const ROOT: u64 = 0x2100_0000;
const SOURCE_ROOT: u64 = 0x2200_0000;
const FUNCTION: u64 = 0x8_1000;
const STACK_BASE: u64 = 0x1010;
const STACK_END: u64 = 0x1080;
const MINIMUM_HEADROOM: u64 = 48;
/// Exactly the minimum headroom above base: the lowest admissible SP.
const LOWEST_SP: u64 = STACK_BASE + MINIMUM_HEADROOM;
const INVOCATION_SLOT: KeySlot = KeySlot(10);

struct CallFixture {
    source_as: ObjectId,
    target_as: ObjectId,
    source_table: u64,
    target_table: u64,
    key: RawKey,
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
            nucleus.current_thread = Some(u32::from(source.index));
            let address_space = nucleus
                .pools
                .arch
                .address_spaces
                .get_live_mut(usize::from(target_as.index))
                .unwrap();
            address_space.set_translation_root(Some(ROOT));
            address_space.set_asid(Some(2));
            let source_address_space = nucleus
                .pools
                .arch
                .address_spaces
                .get_live_mut(usize::from(source_as.index))
                .unwrap();
            source_address_space.set_translation_root(Some(SOURCE_ROOT));
            source_address_space.set_asid(Some(1));
            let source_return_key = provisioned_return_key(source_table);
            let target_return_key = provisioned_return_key(target_table);
            let extent = InvocationStackExtent::new(
                STACK_BASE,
                STACK_END,
                MINIMUM_HEADROOM,
                ArchObjectsImpl::USER_VA_END,
            )
            .unwrap_or_else(|error| panic!("fixture stack extent: {:?}", error.code()));
            let key = nucleus
                .current_thread_table_mut()
                .unwrap()
                .insert(
                    INVOCATION_SLOT,
                    KeyEntry::new_invocation(target_as, NonZero::new(FUNCTION).unwrap(), extent),
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
                    source_return_key,
                    target_return_key,
                },
            );
        },
    );
    TCR_EL1.set(original_tcr);
}

/// A saved Call frame with distinct values in every register.
/// `sp` is the source execution SP, deliberately a valid target SP too,
/// so tests can tell saved-`x9` consumption from `SP` reuse.
fn call_frame(key: RawKey, target_sp: u64) -> SavedContext {
    let mut saved = SavedContext::el1t(0x9_0000, STACK_END);
    for (index, register) in saved.gpr.iter_mut().enumerate() {
        *register = 0x5A00 + u64::try_from(index).unwrap();
    }
    saved.gpr[0] = key.to_wire();
    saved.gpr[1] = 0;
    saved.gpr[9] = target_sp;
    // Valid target SPs in x8/x10 must not stand in for x9.
    saved.gpr[8] = LOWEST_SP;
    saved.gpr[10] = STACK_END;
    saved.lr = 0x9_0040;
    saved.spsr_el1 |= 0x6000_0000;
    saved
}

fn source_mut(nucleus: &mut Nucleus<ArchObjectsImpl>) -> &mut crate::objects::Thread {
    let index = usize::try_from(nucleus.current_thread.unwrap()).unwrap();
    nucleus.pools.threads.get_live_mut(index).unwrap()
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
    current_thread: Option<u32>,
    source_address_space: ObjectId,
    source_context: crate::objects::ExecutionContext,
    source_stack: InvocationStack,
    target_root: Option<u64>,
    target_asid: Option<u16>,
    source_table: u64,
    target_table: u64,
    ttbr0: u64,
    pending: usize,
    runnable: usize,
}

fn snapshot(nucleus: &mut Nucleus<ArchObjectsImpl>, fixture: &CallFixture) -> Snapshot {
    let target = nucleus
        .pools
        .arch
        .address_spaces
        .get_live(usize::from(fixture.target_as.index));
    let current_thread = nucleus.current_thread;
    let target_root = target.and_then(|address_space| address_space.translation_root());
    let target_asid = target.and_then(|address_space| address_space.asid());
    let source = source_mut(nucleus);
    Snapshot {
        current_thread,
        source_address_space: source.address_space,
        source_context: source.context,
        source_stack: source.invocation_stack,
        target_root,
        target_asid,
        source_table: table_digest(fixture.source_table),
        target_table: table_digest(fixture.target_table),
        ttbr0: TTBR0_EL1.get(),
        pending: nucleus.pending.len(),
        runnable: nucleus.scheduler.len(),
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
            addr: fixture.source_table,
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

fn invalid_stack(value: u64, reason: InvalidStackReason) -> CapError {
    CapError::InvalidStack { value, reason }
}

fn break_translation(nucleus: &mut Nucleus<ArchObjectsImpl>, fixture: &CallFixture) {
    let address_space = nucleus
        .pools
        .arch
        .address_spaces
        .get_live_mut(usize::from(fixture.target_as.index))
        .unwrap();
    address_space.set_translation_root(None);
    address_space.set_asid(None);
}

#[test_case]
fn call_preparation_describes_the_migration_without_mutating_anything() {
    with_call(|nucleus, fixture| {
        fill_stack(nucleus, 3);
        let saved = call_frame(fixture.key, LOWEST_SP);
        let (result, before) = prepare(nucleus, &fixture, &saved);
        let prepared =
            result.unwrap_or_else(|error| panic!("valid Call rejected: {:?}", error.code()));
        assert_eq!(snapshot(nucleus, &fixture), before);
        assert_eq!(source_mut(nucleus).invocation_stack.len(), 3);

        let current = nucleus.current_thread.unwrap();
        assert_eq!(u32::from(prepared.source_thread().index), current);
        assert_eq!(
            Some(prepared.source_thread().generation),
            nucleus
                .pools
                .threads
                .generation_of(usize::try_from(current).unwrap())
        );
        assert_eq!(prepared.entry_pc().get(), FUNCTION);
        assert_eq!(prepared.target_sp(), LOWEST_SP);
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
        let expected = InvocationContinuation::from_saved(fixture.source_as, saved, 0);
        assert_eq!(*prepared.continuation(), expected);
        assert_eq!(prepared.continuation().source_pc, 0x9_0000);
        assert_eq!(prepared.continuation().source_sp, STACK_END);
        assert_eq!(prepared.continuation().source_spsr, saved.spsr_el1);
        assert_eq!(prepared.continuation().source_x19_x30[11], saved.lr);

        // SP = end is the other admissible endpoint.
        let (result, _) = prepare(nucleus, &fixture, &call_frame(fixture.key, STACK_END));
        assert_eq!(admitted(result).target_sp(), STACK_END);
    });
}

#[test_case]
fn call_consumes_saved_x9_not_sp_or_neighbouring_registers() {
    with_call(|nucleus, fixture| {
        // SP, x8 and x10 all hold valid target SPs; only saved x9 decides.
        for (submitted, reason) in [
            (STACK_BASE, InvalidStackReason::SpOutOfRange),
            (LOWEST_SP + 8, InvalidStackReason::SpMisaligned),
            (0, InvalidStackReason::SpOutOfRange),
        ] {
            let saved = call_frame(fixture.key, submitted);
            assert_eq!(saved.sp, STACK_END);
            reject_preserved(nucleus, &fixture, &saved, invalid_stack(submitted, reason));
        }
        // Distinct valid saved x9 values are reported back verbatim.
        for submitted in [LOWEST_SP, LOWEST_SP + 16, STACK_END - 16, STACK_END] {
            let (result, _) = prepare(nucleus, &fixture, &call_frame(fixture.key, submitted));
            assert_eq!(admitted(result).target_sp(), submitted);
        }
    });
}

#[test_case]
fn invalid_sp_is_reported_before_unready_translation_or_full_depth() {
    for (submitted, reason) in [
        (LOWEST_SP + 1, InvalidStackReason::SpMisaligned),
        (STACK_BASE, InvalidStackReason::SpOutOfRange),
        (STACK_END + 16, InvalidStackReason::SpOutOfRange),
        (LOWEST_SP - 16, InvalidStackReason::SpInsufficientHeadroom),
    ] {
        for (unready, full) in [(true, false), (false, true), (true, true)] {
            with_call(|nucleus, fixture| {
                if unready {
                    break_translation(nucleus, &fixture);
                }
                if full {
                    fill_stack(nucleus, INVOCATION_STACK_DEPTH);
                }
                let saved = call_frame(fixture.key, submitted);
                let expected = invalid_stack(submitted, reason).code();
                reject_preserved_code(nucleus, &fixture, &saved, expected);
                dispatch_rejected(nucleus, &fixture, &saved, expected);
            });
        }
    }
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
            let address_space = nucleus
                .pools
                .arch
                .address_spaces
                .get_live_mut(usize::from(fixture.target_as.index))
                .unwrap();
            address_space.set_translation_root(root);
            address_space.set_asid(asid);
            let saved = call_frame(fixture.key, LOWEST_SP);
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
        let saved = call_frame(fixture.key, LOWEST_SP);
        let (result, before) = prepare(nucleus, &fixture, &saved);
        assert_eq!(admitted(result).depth(), INVOCATION_STACK_DEPTH - 1);
        assert_eq!(snapshot(nucleus, &fixture), before);

        fill_stack_one_more(nucleus);
        assert_eq!(CapError::NestingDepth { count: 16 }.code(), (33, 16, 0));
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
        let saved = call_frame(fixture.key, LOWEST_SP);
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
                .pools
                .arch
                .address_spaces
                .get_live(usize::from(fixture.target_as.index))
                .unwrap()
                .keytable();
            nucleus
                .pools
                .arch
                .address_spaces
                .deallocate(fixture.target_as)
                .unwrap_or_else(|e| panic!("AS retirement: {e:?}"));
            if reuse {
                let mut replacement = ArchObjectsImpl::new_address_space(binding);
                replacement.set_translation_root(Some(ROOT));
                replacement.set_asid(Some(3));
                let replacement_id = nucleus
                    .pools
                    .arch
                    .address_spaces
                    .allocate(replacement)
                    .unwrap()
                    .0;
                assert_eq!(replacement_id.index, fixture.target_as.index);
            }
            fill_stack(nucleus, INVOCATION_STACK_DEPTH);
            let saved = call_frame(fixture.key, STACK_BASE);
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
        let mut saved = call_frame(fixture.key, STACK_BASE);
        saved.gpr[1] = 1;
        reject_preserved(nucleus, &fixture, &saved, CapError::InvalidOperation);

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
        let saved = call_frame(attenuated, STACK_BASE);
        reject_preserved(nucleus, &fixture, &saved, CapError::InsufficientRights);

        let wrong_kind = nucleus
            .current_thread_table_mut()
            .unwrap()
            .insert(
                KeySlot(12),
                KeyEntry::new::<<ArchObjectsImpl as ArchObjects>::AddressSpace>(
                    fixture.target_as,
                    Rights::all(),
                    0,
                ),
                FIXTURE_GUARD,
            )
            .unwrap_or_else(|error| panic!("wrong-kind install: {:?}", error.error.code()));
        let saved = call_frame(wrong_kind, STACK_BASE);
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
        addr: fixture.source_table,
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
    target_as: ObjectId,
    saved: &SavedContext,
) -> Result<PreparedCall, CapError> {
    let target = CallTarget {
        address_space: target_as,
        function_address: NonZero::new(FUNCTION).unwrap(),
        stack_extent: InvocationStackExtent::new(
            STACK_BASE,
            STACK_END,
            MINIMUM_HEADROOM,
            ArchObjectsImpl::USER_VA_END,
        )
        .unwrap_or_else(|error| panic!("fixture stack extent: {:?}", error.code())),
    };
    // SAFETY: as in `call`.
    let access = unsafe { Access::new() };
    nucleus.prepare_call(&access, saved, target)
}

fn committed(result: Result<CommittedCall, CapError>) -> CommittedCall {
    result.unwrap_or_else(|error| panic!("valid Call commit rejected: {:?}", error.code()))
}

#[test_case]
fn call_commit_pushes_continuation_and_migrates_into_the_target_table() {
    with_call(|nucleus, fixture| {
        fill_stack(nucleus, 2);
        let saved = call_frame(fixture.key, LOWEST_SP);
        let before = snapshot(nucleus, &fixture);
        let expected = *admitted(prepare(nucleus, &fixture, &saved).0).continuation();
        let commit = committed(call(nucleus, &fixture, &saved));

        let source = source_mut(nucleus);
        assert_eq!(source.address_space, fixture.target_as);
        assert_eq!(source.context, before.source_context);
        assert_eq!(source.invocation_stack.len(), 3);
        assert_eq!(*source.invocation_stack.top().unwrap(), expected);
        assert_eq!(expected.source_address_space, fixture.source_as);
        // Earlier records are untouched beneath the new top.
        let mut below = source.invocation_stack;
        assert!(below.pop().is_some());
        assert_eq!(below, before.source_stack);

        // Key resolution now selects the target AddressSpace's table.
        assert_eq!(nucleus.current_thread, before.current_thread);
        assert_eq!(
            nucleus.current_thread_table_addr(),
            Some(fixture.target_table)
        );
        assert_eq!(
            u32::from(commit.source_thread.index),
            nucleus.current_thread.unwrap()
        );
        assert_eq!(commit.translation.address_space(), fixture.target_as);
        assert_eq!(commit.translation.root(), ROOT);
        assert_eq!(commit.translation.asid(), 2);

        // Commit touches no hardware, table, target metadata or scheduling.
        let after = snapshot(nucleus, &fixture);
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
        let mut saved = call_frame(fixture.key, LOWEST_SP);
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
        assert_eq!(target.sp, LOWEST_SP);
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
            let mut saved = call_frame(fixture.key, STACK_END);
            saved.elr_el1 = 0x9_0000 + 4 * u64::try_from(depth).unwrap();
            let prepared = admitted(prepare_direct(nucleus, fixture.target_as, &saved));
            assert_eq!(prepared.depth(), depth);
            committed(nucleus.commit_call(prepared));
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

        let saved = call_frame(fixture.key, STACK_END);
        let error = prepare_direct(nucleus, fixture.target_as, &saved)
            .expect_err("seventeenth Call admitted");
        assert_eq!(error.code(), (33, 16, 0));
        assert_eq!(source_mut(nucleus).invocation_stack, stack);
        assert_eq!(source_mut(nucleus).address_space, fixture.target_as);
    });
}

#[test_case]
fn stale_prepared_call_is_rejected_without_mutation() {
    for staleness in 0..3 {
        with_call(|nucleus, fixture| {
            let saved = call_frame(fixture.key, LOWEST_SP);
            let prepared = admitted(prepare(nucleus, &fixture, &saved).0);
            match staleness {
                // Another push happened after admission.
                0 => fill_stack(nucleus, 1),
                // The Thread already migrated elsewhere.
                1 => source_mut(nucleus).address_space = fixture.target_as,
                // A different Thread is current.
                _ => {
                    let other = nucleus.create_thread(fixture.source_as).unwrap();
                    nucleus.current_thread = Some(u32::from(other.index));
                }
            }
            let before = snapshot(nucleus, &fixture);
            let error = nucleus
                .commit_call(prepared)
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
        for submitted in [STACK_BASE, LOWEST_SP - 16, LOWEST_SP + 8] {
            let saved = call_frame(fixture.key, submitted);
            let before = snapshot(nucleus, &fixture);
            let error = call(nucleus, &fixture, &saved).expect_err("invalid SP committed");
            assert!(matches!(error, CapError::InvalidStack { .. }));
            assert_eq!(snapshot(nucleus, &fixture), before);
        }
        fill_stack_one_more_until_full(nucleus);
        let saved = call_frame(fixture.key, LOWEST_SP);
        let before = snapshot(nucleus, &fixture);
        let error = call(nucleus, &fixture, &saved).expect_err("full stack committed");
        assert_eq!(error.code(), (33, 16, 0));
        assert_eq!(snapshot(nucleus, &fixture), before);
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
        .resolve_carved_mut::<KeyTable>(table_address)
        .unwrap_or_else(|error| panic!("fixture table: {:?}", error.code()));
    let entry = table
        .lookup(key, FIXTURE_GUARD)
        .unwrap_or_else(|error| panic!("provisioned Return key: {:?}", error.code()));
    assert!(entry.is_thread_return_key());
    key
}

/// A target-side Return frame: payload in x2/x3, `extra` in x4..x7, and
/// target sentinels everywhere else that must never reach the source.
fn return_frame(key: RawKey, payload: [u64; 2], extra: [u64; 4]) -> SavedContext {
    let mut saved = SavedContext::el1t(FUNCTION + 0x40, LOWEST_SP - 16);
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
        addr: nucleus.current_thread_table_addr().unwrap(),
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
        let mut call_saved = call_frame(fixture.key, LOWEST_SP);
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
        assert!(resumed.gpr[3..19].iter().all(|&register| register == 0));
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
        assert_eq!(
            u32::from(commit.source_thread.index),
            nucleus.current_thread.unwrap()
        );
        // Back in the source AS and table, with the prior stack exactly restored.
        assert_eq!(
            nucleus.current_thread_table_addr(),
            Some(fixture.source_table)
        );
        assert_eq!(snapshot(nucleus, &fixture), before);
    });
}

#[test_case]
fn return_ignores_x4_to_x7() {
    let mut frames = [None; 3];
    for (run, extra) in [[0; 4], [u64::MAX; 4], [0x4, 0, u64::MAX, 0x7777]]
        .into_iter()
        .enumerate()
    {
        with_call(|nucleus, fixture| {
            committed(call(nucleus, &fixture, &call_frame(fixture.key, LOWEST_SP)));
            let saved = return_frame(fixture.target_return_key, [5, 6], extra);
            frames[run] = Some(returned(thread_return(nucleus, &saved)).resumed);
            assert!(source_mut(nucleus).invocation_stack.is_empty());
        });
    }
    assert!(frames[0].is_some());
    assert_eq!(frames[0], frames[1]);
    assert_eq!(frames[0], frames[2]);
}

#[test_case]
fn nested_returns_pop_in_lifo_order() {
    with_call(|nucleus, fixture| {
        let first = call_frame(fixture.key, LOWEST_SP);
        committed(call(nucleus, &fixture, &first));
        let mut second = call_frame(fixture.key, STACK_END);
        second.elr_el1 = 0x9_1000;
        second.gpr[19] = 0x1919;
        let prepared = admitted(prepare_direct(nucleus, fixture.target_as, &second));
        committed(nucleus.commit_call(prepared));
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
        committed(call(nucleus, &fixture, &call_frame(fixture.key, LOWEST_SP)));
        let target_saved = return_frame(fixture.target_return_key, [1, 2], [0; 4]);
        returned(thread_return(nucleus, &target_saved));
        return_faulted(nucleus, &fixture, &saved, ReturnFault::IllegalReturn);
    });
}

#[test_case]
fn retired_source_is_a_return_target_retired_fault_without_pop() {
    for reuse in [false, true] {
        with_call(|nucleus, fixture| {
            committed(call(nucleus, &fixture, &call_frame(fixture.key, LOWEST_SP)));
            let binding = nucleus
                .pools
                .arch
                .address_spaces
                .get_live(usize::from(fixture.source_as.index))
                .unwrap()
                .keytable();
            nucleus
                .pools
                .arch
                .address_spaces
                .deallocate(fixture.source_as)
                .unwrap_or_else(|e| panic!("source AS retirement: {e:?}"));
            if reuse {
                let mut replacement = ArchObjectsImpl::new_address_space(binding);
                replacement.set_translation_root(Some(SOURCE_ROOT));
                replacement.set_asid(Some(1));
                let replacement_id = nucleus
                    .pools
                    .arch
                    .address_spaces
                    .allocate(replacement)
                    .unwrap()
                    .0;
                assert_eq!(replacement_id.index, fixture.source_as.index);
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
        committed(call(nucleus, &fixture, &call_frame(fixture.key, LOWEST_SP)));
        nucleus
            .pools
            .arch
            .address_spaces
            .get_live_mut(usize::from(fixture.source_as.index))
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
        committed(call(nucleus, &fixture, &call_frame(fixture.key, LOWEST_SP)));
        let thread = {
            let index = nucleus.current_thread.unwrap();
            ObjectId {
                pool: crate::objects::access::PoolTag::Thread,
                index: u16::try_from(index).unwrap(),
                generation: nucleus
                    .pools
                    .threads
                    .generation_of(usize::try_from(index).unwrap())
                    .unwrap(),
            }
        };
        let named = nucleus
            .current_thread_table_mut()
            .unwrap()
            .insert(
                KeySlot(20),
                KeyEntry::new::<crate::objects::Thread>(thread, Rights::all(), 0),
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
                    InvocationStackExtent::new(
                        STACK_BASE,
                        STACK_END,
                        MINIMUM_HEADROOM,
                        ArchObjectsImpl::USER_VA_END,
                    )
                    .unwrap_or_else(|error| panic!("extent: {:?}", error.code())),
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
            committed(call(nucleus, &fixture, &call_frame(fixture.key, LOWEST_SP)));
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
                    nucleus.current_thread = Some(u32::from(other.index));
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
        let call_saved = call_frame(fixture.key, LOWEST_SP);
        let call = match api::handle_cap_invoke(nucleus, &call_saved) {
            Ok(api::InvokeOutcome::Call(committed)) => committed,
            Ok(_) => panic!("Invocation.Call dispatched to a non-Call outcome"),
            Err(error) => panic!("dispatched Call rejected: {:?}", error.code()),
        };
        assert_eq!(call.target.sp, LOWEST_SP);
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

/// Install a fault handler Invocation (into the target `AddressSpace`, with
/// the fixture stack extent) at `FAULT_HANDLER` in the current Thread's table.
fn install_fault_handler(nucleus: &mut Nucleus<ArchObjectsImpl>, fixture: &CallFixture) {
    let extent = InvocationStackExtent::new(
        STACK_BASE,
        STACK_END,
        MINIMUM_HEADROOM,
        ArchObjectsImpl::USER_VA_END,
    )
    .unwrap_or_else(|error| panic!("fixture stack extent: {:?}", error.code()));
    nucleus
        .current_thread_table_mut()
        .unwrap()
        .insert(
            KeySlot::FAULT_HANDLER,
            KeyEntry::new_invocation(fixture.target_as, NonZero::new(FUNCTION).unwrap(), extent),
            FIXTURE_GUARD,
        )
        .unwrap_or_else(|error| panic!("fault handler install: {:?}", error.error.code()));
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

fn handler_state(nucleus: &Nucleus<ArchObjectsImpl>, address_space: ObjectId) -> (bool, u64) {
    let space = nucleus
        .pools
        .arch
        .address_spaces
        .get_live(usize::from(address_space.index))
        .unwrap();
    (space.fault_handler_busy(), space.unhandled_faults())
}

/// Queue a never-run Thread so an unhandled fault has somewhere to switch.
fn queue_next_thread(nucleus: &mut Nucleus<ArchObjectsImpl>, fixture: &CallFixture) -> u16 {
    let next = nucleus.create_thread(fixture.source_as).unwrap();
    nucleus
        .pools
        .threads
        .get_live_mut(usize::from(next.index))
        .unwrap()
        .context = ExecutionContext::NotStarted {
        saved: SavedContext::el0(0xA_0000, STACK_END, 0),
    };
    assert!(nucleus.scheduler.push(next.index));
    next.index
}

fn expect_unhandled(
    delivery: FaultDelivery,
    nucleus: &Nucleus<ArchObjectsImpl>,
    faulted: u32,
    faulting: SavedContext,
    next: u16,
) {
    let FaultDelivery::Unhandled(resumed) = delivery else {
        panic!("the fault must be unhandled");
    };
    assert_eq!(resumed.current, faulted);
    assert_eq!(resumed.next, next);
    let thread = nucleus
        .pools
        .threads
        .get_live(usize::try_from(faulted).unwrap())
        .unwrap();
    assert_eq!(
        thread.context,
        ExecutionContext::Faulted { saved: faulting }
    );
    assert_eq!(thread.fault, None);
    assert_eq!(nucleus.current_thread, Some(u32::from(next)));
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
        assert_eq!(&target.gpr[0..2], &[0, 0]);
        assert!(target.gpr[8..].iter().all(|&register| register == 0));
        assert_eq!(target.sp, STACK_END, "the handler starts at its stack top");
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
            })
        );
        assert_eq!(handler_state(nucleus, fixture.source_as), (true, 0));
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
        committed(call(nucleus, &fixture, &call_frame(fixture.key, LOWEST_SP)));
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
        let faulted = nucleus.current_thread.unwrap();
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
            .pools
            .arch
            .address_spaces
            .get_live_mut(usize::from(fixture.source_as.index))
            .unwrap()
            .set_fault_handler_busy(true);
        let faulted = nucleus.current_thread.unwrap();
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
        let faulted = nucleus.current_thread.unwrap();
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
        let faulted = nucleus.current_thread.unwrap();
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
        let index = usize::try_from(nucleus.current_thread.unwrap()).unwrap();
        nucleus.release_thread_fault(index);
        assert_eq!(source_mut(nucleus).fault, None);
        assert_eq!(handler_state(nucleus, fixture.source_as), (false, 0));
    });
}

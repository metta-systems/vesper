//! `Invocation.Call` admission/preparation and stage-5 commit tests.
//! Roots below are checked metadata only; these tests never install TTBR0.

use {
    super::{FIXTURE_GUARD, SIZE_BITS, with_nucleus},
    crate::{
        api::{self, KeyEntry},
        objects::{
            ArchObjects, ArchObjectsImpl, InvocationContinuation, InvocationStack, KeyTable,
            Nucleus,
            access::{Access, ObjectId},
            arch_objects::AddressSpaceObject,
            invocation::{CallTarget, CommittedCall, InvocationStackExtent, PreparedCall},
            key_table::CallerTable,
        },
    },
    aarch64_cpu::registers::{Readable, SCTLR_EL1, TCR_EL1, TTBR0_EL1, Writeable},
    core::num::NonZero,
    libexception::arch::aarch64::SavedContext,
    libobject::{
        CapError, INVOCATION_STACK_DEPTH, InvalidStackReason, KeySlot, ObjectType, RawKey, Rights,
    },
};

const ROOT: u64 = 0x2100_0000;
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
    let (result, before) = prepare(nucleus, fixture, saved);
    let error = result.expect_err("rejected Call admission prepared a migration");
    assert_eq!(error.code(), expected.code());
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
                reject_preserved(nucleus, &fixture, &saved, invalid_stack(submitted, reason));
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
            reject_preserved(nucleus, &fixture, &saved, expected);
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
        assert_eq!(source_mut(nucleus).invocation_stack.len(), 16);
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
                .unwrap_or_else(|e| panic!("AS retirement: {:?}", e.code()));
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
            reject_preserved(nucleus, &fixture, &saved, CapError::InvalidDomain);
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

        // The source keeps every scrubbed value privately in its record.
        let record = *source_mut(nucleus).invocation_stack.top().unwrap();
        assert_eq!(record.source_spsr, saved.spsr_el1);
        assert_eq!(&record.source_x19_x30[..11], &saved.gpr[19..30]);
        assert_eq!(record.source_x19_x30[11], saved.lr);
        assert_eq!(record.source_pc, saved.elr_el1);
        assert_eq!(record.source_sp, saved.sp);
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

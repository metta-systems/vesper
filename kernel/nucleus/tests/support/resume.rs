//! Production resume/activation tests using the serial QEMU nucleus fixture.
//! Roots below are checked metadata only; these tests never install TTBR0.

use {
    super::{FIXTURE_GUARD, SIZE_BITS, with_nucleus},
    crate::{
        api::{self, InvokeOutcome, KeyEntry},
        objects::{
            ArchObjects, ArchObjectsImpl, ExecutionContext, Nucleus,
            access::{Access, ObjectId, PoolTag},
            arch_objects::AddressSpaceObject,
            completion::{PendingKind, PendingState},
            resume::prepare_translation_context,
        },
    },
    aarch64_cpu::registers::{Readable, SCTLR_EL1, TCR_EL1, Writeable},
    libexception::arch::aarch64::{ExceptionOrigin, SavedContext},
    libobject::{CapError, KeySlot, RawKey, Rights, syscall_status},
};

const ROOT: u64 = 0x2100_0000;
const SOURCE_SAVED: SavedContext = SavedContext::el1t(0x80000, 0x90000);

fn target_saved() -> SavedContext {
    let mut saved = SavedContext::el1t(0xA0000, 0xB0000);
    for (index, register) in saved.gpr.iter_mut().enumerate() {
        *register = 0xC000 + u64::try_from(index).unwrap();
    }
    saved.lr = 0xD0000;
    saved.spsr_el1 |= 0xA000_0000;
    saved
}

struct Waits {
    source: ObjectId,
    target: ObjectId,
    incoming: ObjectId,
    terminal: ObjectId,
    target_as: ObjectId,
}

fn with_resume(test: impl FnOnce(&mut Nucleus<ArchObjectsImpl>, Waits)) {
    // The serial harness runs without translation; provide the backend's
    // supported TCR profile for metadata validation only. Never install these
    // synthetic roots, enable the MMU, or leak the profile to another test.
    assert_eq!(SCTLR_EL1.get() & 1, 0);
    let original_tcr = TCR_EL1.get();
    TCR_EL1.set(16);
    with_nucleus(
        |nucleus, _source_table, _target_table, source_as, target_as| {
            let source = nucleus
                .create_thread(source_as)
                .expect("source Thread allocation");
            let target = nucleus
                .create_thread(target_as)
                .expect("target Thread allocation");
            nucleus
                .create_thread(source_as)
                .expect("FIFO tail Thread allocation");
            let incoming = nucleus
                .pending
                .block(source, PendingKind::NotificationWait)
                .unwrap_or_else(|e| panic!("incoming wait: {:?}", e.code()));
            let terminal = nucleus
                .pending
                .block(target, PendingKind::EventCountAwait)
                .unwrap_or_else(|e| panic!("target wait: {:?}", e.code()));
            nucleus
                .pending
                .complete_with_status(terminal, syscall_status::COUNTER_OVERFLOW, 0x1234, 0x5678)
                .unwrap_or_else(|e| panic!("target completion: {:?}", e.code()));
            nucleus
                .pools
                .threads
                .get_live_mut(usize::from(target.index))
                .unwrap()
                .context = ExecutionContext::Parked {
                saved: target_saved(),
                record: terminal,
            };
            let address_space = nucleus
                .pools
                .arch
                .address_spaces
                .get_live_mut(usize::from(target_as.index))
                .unwrap();
            address_space.set_translation_root(Some(ROOT));
            address_space.set_asid(Some(2));
            nucleus.current_thread = Some(u32::from(source.index));
            assert!(nucleus.scheduler.push(target.index));
            assert!(nucleus.scheduler.push(2));
            test(
                nucleus,
                Waits {
                    source,
                    target,
                    incoming,
                    terminal,
                    target_as,
                },
            );
        },
    );
    TCR_EL1.set(original_tcr);
}

fn reject_unchanged(nucleus: &mut Nucleus<ArchObjectsImpl>, waits: &Waits, expected: CapError) {
    let source_before = nucleus
        .pools
        .threads
        .get_live(usize::from(waits.source.index))
        .unwrap()
        .context;
    let target_before = nucleus
        .pools
        .threads
        .get_live(usize::from(waits.target.index))
        .unwrap()
        .context;
    let pending_len = nucleus.pending.len();
    let terminal_before = nucleus
        .pending
        .state(waits.terminal)
        .unwrap_or_else(|e| panic!("terminal snapshot: {:?}", e.code()));
    let parked_before = match target_before {
        ExecutionContext::Parked { record, .. } => {
            Some((record, nucleus.pending.state(record).ok()))
        }
        _ => None,
    };
    let table_before = nucleus.current_thread_table_addr();
    // SAFETY: this fixture is serial and owns exclusive nucleus/pool backing;
    // no other access context or object/table guard overlaps this transaction.
    let result = {
        let access = unsafe { Access::new() };
        nucleus.park_and_select(&access, SOURCE_SAVED, waits.incoming)
    };
    let error = result.expect_err("rejected scheduling metadata succeeded");
    assert_eq!(error.code(), expected.code());
    assert_eq!(nucleus.current_thread, Some(u32::from(waits.source.index)));
    assert_eq!(nucleus.current_thread_table_addr(), table_before);
    assert_eq!(
        nucleus
            .pools
            .threads
            .get_live(usize::from(waits.source.index))
            .unwrap()
            .context,
        source_before
    );
    assert_eq!(
        nucleus
            .pools
            .threads
            .get_live(usize::from(waits.target.index))
            .unwrap()
            .context,
        target_before
    );
    assert_eq!(nucleus.pending.len(), pending_len);
    if let Some((record, state)) = parked_before {
        assert_eq!(nucleus.pending.state(record).ok(), state);
    }
    assert_eq!(
        nucleus
            .pending
            .state(waits.incoming)
            .unwrap_or_else(|e| panic!("incoming preserved: {:?}", e.code())),
        PendingState::Waiting
    );
    assert_eq!(
        nucleus
            .pending
            .state(waits.terminal)
            .unwrap_or_else(|e| panic!("terminal preserved: {:?}", e.code())),
        terminal_before
    );
    assert_eq!(nucleus.scheduler.len(), 2);
    assert_eq!(nucleus.scheduler.peek(), Some(waits.target.index));
    assert_eq!(nucleus.scheduler.pop(), Some(waits.target.index));
    assert_eq!(nucleus.scheduler.pop(), Some(2));
}

#[test_case]
fn resume_rejects_invalid_as_identity_before_consuming_any_state() {
    for identity in [
        ObjectId {
            pool: PoolTag::Thread,
            index: 1,
            generation: 1,
        },
        ObjectId {
            pool: PoolTag::AddressSpace,
            index: u16::MAX,
            generation: 1,
        },
        ObjectId {
            pool: PoolTag::AddressSpace,
            index: 1,
            generation: 0,
        },
    ] {
        with_resume(|nucleus, waits| {
            nucleus
                .pools
                .threads
                .get_live_mut(usize::from(waits.target.index))
                .unwrap()
                .address_space = identity;
            reject_unchanged(nucleus, &waits, CapError::InvalidDomain);
        });
    }
}

#[test_case]
fn resume_rejects_retired_and_reused_as_before_consuming_completed_wait() {
    for reuse in [false, true] {
        with_resume(|nucleus, waits| {
            let binding = nucleus
                .pools
                .arch
                .address_spaces
                .get_live(usize::from(waits.target_as.index))
                .unwrap()
                .keytable();
            nucleus
                .pools
                .arch
                .address_spaces
                .deallocate(waits.target_as)
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
                assert_eq!(replacement_id.index, waits.target_as.index);
                assert_ne!(replacement_id.generation, waits.target_as.generation);
            }
            reject_unchanged(nucleus, &waits, CapError::InvalidDomain);
        });
    }
}

#[test_case]
fn resume_rejects_missing_root_or_asid_without_losing_context_or_pending() {
    for (root, asid) in [(None, None), (None, Some(2)), (Some(ROOT), None)] {
        with_resume(|nucleus, waits| {
            let address_space = nucleus
                .pools
                .arch
                .address_spaces
                .get_live_mut(usize::from(waits.target_as.index))
                .unwrap();
            address_space.set_translation_root(root);
            address_space.set_asid(asid);
            reject_unchanged(nucleus, &waits, CapError::NotMapped);
        });
    }
}

#[test_case]
fn resume_rejects_unencodable_root_or_reserved_asid_atomically() {
    for (root, asid, error) in [
        (ROOT + 1, 2, CapError::InvalidPointer),
        (1 << 52, 2, CapError::InvalidPointer),
        (ROOT, 0, CapError::InvalidOperation),
    ] {
        with_resume(|nucleus, waits| {
            let address_space = nucleus
                .pools
                .arch
                .address_spaces
                .get_live_mut(usize::from(waits.target_as.index))
                .unwrap();
            address_space.set_translation_root(Some(root));
            address_space.set_asid(Some(asid));
            reject_unchanged(nucleus, &waits, error);
        });
    }
}

#[test_case]
fn resume_rejects_unsupported_ttbr0_walk_profile_atomically() {
    for tcr in [0, 25, 16 | (1 << 7)] {
        with_resume(|nucleus, waits| {
            TCR_EL1.set(tcr);
            reject_unchanged(nucleus, &waits, CapError::InvalidOperation);
        });
    }
}

#[test_case]
fn resume_rejects_incoherent_context_and_completion_before_commit() {
    for case in 0..6 {
        with_resume(|nucleus, waits| {
            let mut saved = target_saved();
            match case {
                0 => {
                    saved.origin = ExceptionOrigin::CurrentSpx;
                    nucleus
                        .pools
                        .threads
                        .get_live_mut(usize::from(waits.target.index))
                        .unwrap()
                        .context = ExecutionContext::Parked {
                        saved,
                        record: waits.terminal,
                    };
                }
                1 => {
                    nucleus
                        .pools
                        .threads
                        .get_live_mut(usize::from(waits.target.index))
                        .unwrap()
                        .context = ExecutionContext::Running
                }
                2 | 5 => {
                    let waiting = nucleus
                        .pending
                        .block(waits.target, PendingKind::EventCountAwait)
                        .unwrap_or_else(|e| panic!("waiting record: {:?}", e.code()));
                    if case == 5 {
                        nucleus
                            .pending
                            .cancel(waiting)
                            .unwrap_or_else(|e| panic!("cancelled record: {:?}", e.code()));
                    }
                    nucleus
                        .pools
                        .threads
                        .get_live_mut(usize::from(waits.target.index))
                        .unwrap()
                        .context = ExecutionContext::Parked {
                        saved,
                        record: waiting,
                    };
                }
                3 => {
                    nucleus
                        .pools
                        .threads
                        .get_live_mut(usize::from(waits.target.index))
                        .unwrap()
                        .context = ExecutionContext::Parked {
                        saved,
                        record: ObjectId {
                            generation: 0,
                            ..waits.terminal
                        },
                    }
                }
                _ => {
                    nucleus
                        .pools
                        .threads
                        .get_live_mut(usize::from(waits.target.index))
                        .unwrap()
                        .context = ExecutionContext::Parked {
                        saved,
                        record: waits.incoming,
                    }
                }
            }
            reject_unchanged(
                nucleus,
                &waits,
                if case == 0 {
                    CapError::InvalidDomain
                } else {
                    CapError::InvalidOperation
                },
            );
        });
    }
}

#[test_case]
fn first_start_failure_preserves_its_initial_context_and_runnable_position() {
    with_resume(|nucleus, waits| {
        nucleus
            .pools
            .threads
            .get_live_mut(usize::from(waits.target.index))
            .unwrap()
            .context = ExecutionContext::NotStarted {
            saved: target_saved(),
        };
        nucleus
            .pools
            .arch
            .address_spaces
            .get_live_mut(usize::from(waits.target_as.index))
            .unwrap()
            .set_translation_root(None);
        reject_unchanged(nucleus, &waits, CapError::NotMapped);
    });
}

#[test_case]
fn resume_commits_checked_root_asid_and_current_as_table_with_full_error_result() {
    with_resume(|nucleus, waits| {
        let target_binding = nucleus
            .pools
            .arch
            .address_spaces
            .get_live(usize::from(waits.target_as.index))
            .unwrap()
            .keytable();
        // SAFETY: serial exclusive fixture; the access/guards end before any
        // subsequent state inspection, and no hardware install occurs here.
        let resumed = {
            let access = unsafe { Access::new() };
            nucleus
                .park_and_select(&access, SOURCE_SAVED, waits.incoming)
                .unwrap_or_else(|e| panic!("prepared resume: {:?}", e.code()))
        };
        assert_eq!(resumed.translation.address_space(), waits.target_as);
        assert_eq!(resumed.translation.root(), ROOT);
        assert_eq!(resumed.translation.asid(), 2);
        assert_eq!(resumed.translation.keytable(), target_binding);
        assert_eq!(resumed.current, u32::from(waits.source.index));
        assert_eq!(resumed.next, waits.target.index);
        let mut expected = target_saved();
        expected.gpr[0] = syscall_status::COUNTER_OVERFLOW;
        expected.gpr[1] = 0x1234;
        expected.gpr[2] = 0x5678;
        assert_eq!(resumed.saved, expected);
        assert_eq!(
            resumed.completion.unwrap().kind,
            PendingKind::EventCountAwait
        );
        assert_eq!(nucleus.current_thread, Some(u32::from(waits.target.index)));
        assert_eq!(
            nucleus.current_thread_table_addr(),
            Some(target_binding.address())
        );
        assert_eq!(
            nucleus
                .pools
                .threads
                .get_live(usize::from(waits.source.index))
                .unwrap()
                .context,
            ExecutionContext::Parked {
                saved: SOURCE_SAVED,
                record: waits.incoming
            }
        );
        assert_eq!(
            nucleus
                .pools
                .threads
                .get_live(usize::from(waits.target.index))
                .unwrap()
                .context,
            ExecutionContext::Running
        );
        assert!(nucleus.pending.state(waits.terminal).is_err());
        assert_eq!(nucleus.pending.len(), 1);
        assert_eq!(nucleus.scheduler.pop(), Some(2));
        assert!(nucleus.scheduler.is_empty());
    });
}

#[test_case]
fn first_start_preparation_preserves_all_saved_registers_and_selects_its_table() {
    with_resume(|nucleus, waits| {
        nucleus
            .pools
            .threads
            .get_live_mut(usize::from(waits.target.index))
            .unwrap()
            .context = ExecutionContext::NotStarted {
            saved: target_saved(),
        };
        // SAFETY: exclusive serial fixture, no overlapping context/guard.
        let resumed = {
            let access = unsafe { Access::new() };
            nucleus
                .park_and_select(&access, SOURCE_SAVED, waits.incoming)
                .unwrap_or_else(|e| panic!("prepared first start: {:?}", e.code()))
        };
        assert_eq!(resumed.saved, target_saved());
        assert_eq!(resumed.completion, None);
        assert_eq!(resumed.translation.root(), ROOT);
        assert_eq!(resumed.translation.asid(), 2);
        assert_eq!(
            nucleus.current_thread_table_addr(),
            Some(resumed.translation.keytable().address())
        );
        // Unrelated pending records are never destroyed by a first start.
        assert!(nucleus.pending.state(waits.terminal).is_ok());
        assert_eq!(nucleus.pending.len(), 2);
    });
}

#[test_case]
fn activate_returns_checked_metadata_without_a_hardware_transition_or_state_change() {
    with_resume(|nucleus, waits| {
        let source_as = nucleus
            .pools
            .threads
            .get_live(usize::from(waits.source.index))
            .unwrap()
            .address_space;
        let source_address_space = nucleus
            .pools
            .arch
            .address_spaces
            .get_live_mut(usize::from(source_as.index))
            .unwrap();
        source_address_space.set_translation_root(Some(ROOT + 0x1000));
        source_address_space.set_asid(Some(1));
        let source_binding = source_address_space.keytable();
        let key = nucleus
            .current_thread_table_mut()
            .unwrap()
            .insert(
                KeySlot::SELF_ADDRESS_SPACE,
                KeyEntry::new::<<ArchObjectsImpl as ArchObjects>::AddressSpace>(
                    source_as,
                    Rights::all(),
                    0,
                ),
                FIXTURE_GUARD,
            )
            .unwrap_or_else(|e| panic!("AS cap: {:?}", e.error.code()));
        let ttbr_before = {
            use aarch64_cpu::registers::{Readable, TTBR0_EL1};
            TTBR0_EL1.get()
        };
        let prepared = match api::handle_cap_invoke(nucleus, key, 0, &[0; 6]) {
            Ok(InvokeOutcome::Activate(prepared)) => prepared,
            Err(e) => panic!("activation preparation: {:?}", e.code()),
            Ok(_) => panic!("activation reported completion before installing hardware"),
        };
        assert_eq!(prepared.address_space(), source_as);
        assert_eq!(prepared.root(), ROOT + 0x1000);
        assert_eq!(prepared.asid(), 1);
        assert_eq!(prepared.keytable(), source_binding);
        {
            use aarch64_cpu::registers::{Readable, TTBR0_EL1};
            assert_eq!(TTBR0_EL1.get(), ttbr_before);
        }
        assert_eq!(nucleus.current_thread, Some(u32::from(waits.source.index)));
        assert_eq!(nucleus.pending.len(), 2);
        assert_eq!(nucleus.scheduler.len(), 2);
        // Same prepared helper also rejects missing metadata through Activate.
        nucleus
            .pools
            .arch
            .address_spaces
            .get_live_mut(usize::from(source_as.index))
            .unwrap()
            .set_asid(None);
        assert!(matches!(
            api::handle_cap_invoke(nucleus, key, 0, &[0; 6]),
            Err(CapError::NotMapped)
        ));
        let foreign_key = nucleus
            .current_thread_table_mut()
            .unwrap()
            .insert(
                KeySlot(20),
                KeyEntry::new::<<ArchObjectsImpl as ArchObjects>::AddressSpace>(
                    waits.target_as,
                    Rights::all(),
                    0,
                ),
                FIXTURE_GUARD,
            )
            .unwrap_or_else(|e| panic!("foreign AS cap: {:?}", e.error.code()));
        assert!(matches!(
            api::handle_cap_invoke(nucleus, foreign_key, 0, &[0; 6]),
            Err(CapError::InvalidOperation)
        ));
        let no_map = nucleus
            .current_thread_table_mut()
            .unwrap()
            .insert(
                KeySlot(21),
                KeyEntry::new::<<ArchObjectsImpl as ArchObjects>::AddressSpace>(
                    source_as,
                    Rights(Rights::READ),
                    0,
                ),
                FIXTURE_GUARD,
            )
            .unwrap_or_else(|e| panic!("attenuated AS cap: {:?}", e.error.code()));
        assert!(matches!(
            api::handle_cap_invoke(nucleus, no_map, 0, &[0; 6]),
            Err(CapError::InsufficientRights)
        ));
        // Foreign guarded table selectors stay ordinary errors, not activation.
        let foreign_guard =
            RawKey::from_parts(FIXTURE_GUARD ^ 1, SIZE_BITS, 20, foreign_key.incarnation());
        assert!(api::handle_cap_invoke(nucleus, foreign_guard, 0, &[0; 6]).is_err());
        // SAFETY: exclusive serial fixture, dispatch's Access has ended.
        let access = unsafe { Access::new() };
        assert!(matches!(
            prepare_translation_context::<ArchObjectsImpl>(
                &access,
                &nucleus.pools.arch.address_spaces,
                source_as
            ),
            Err(CapError::NotMapped)
        ));
    });
}

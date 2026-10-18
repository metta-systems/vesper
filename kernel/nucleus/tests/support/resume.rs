//! Production resume/activation tests using the serial QEMU nucleus fixture.
//! Roots below are checked metadata only; these tests never install TTBR0.

use {
    super::{FIXTURE_GUARD, SIZE_BITS, cap_invoke, with_nucleus},
    crate::{
        api::{InvokeOutcome, KeyEntry},
        objects::{
            ArchObjectsImpl, ExecutionContext, Nucleus, Scheduler, Time,
            access::{Access, ObjectId},
            arch_objects::{AddressSpaceId, AddressSpaceObject},
            carved::{Carved, CarvedId},
            completion::{PendingKind, PendingState},
            nucleus::test_arena,
            resume::{Switch, prepare_translation_context},
            thread::ThreadId,
        },
    },
    aarch64_cpu::registers::{Readable, SCTLR_EL1, TCR_EL1, Writeable},
    libexception::arch::aarch64::{ExceptionOrigin, SavedContext},
    libobject::{
        CapError, KeySlot, ObjectType, RawKey, Rights,
        scheduler::{SchedulerOp, activation_mask, activation_reason},
        syscall_status,
    },
};

const ROOT: u64 = 0x2100_0000;
/// Each Thread carries its own EL0 TLS (`TPIDR_EL0`) through park and resume.
const SOURCE_SAVED: SavedContext = SavedContext {
    tpidr_el0: 0x7150_0000_0000_1500,
    ..SavedContext::el1t(0x80000, 0x90000)
};

fn target_saved() -> SavedContext {
    let mut saved = SavedContext::el1t(0xA0000, 0xB0000);
    for (index, register) in saved.gpr.iter_mut().enumerate() {
        *register = 0xC000 + u64::try_from(index).unwrap();
    }
    saved.lr = 0xD0000;
    saved.spsr_el1 |= 0xA000_0000;
    saved.tpidr_el0 = 0x7a40_0000_0000_7a40;
    saved
}

struct Waits {
    source: ThreadId,
    target: ThreadId,
    /// The second queued Thread, behind `target`.
    tail: ThreadId,
    incoming: ObjectId,
    terminal: ObjectId,
    target_as: AddressSpaceId,
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
            let tail = nucleus
                .create_thread(source_as)
                .expect("FIFO tail Thread allocation");
            let incoming = nucleus
                .pending
                .block(
                    source,
                    PendingKind::NotificationWait,
                    crate::objects::carved::test_id(libobject::ObjectType::NOTIFICATION, 0),
                )
                .unwrap_or_else(|e| panic!("incoming wait: {:?}", e.code()));
            let terminal = nucleus
                .pending
                .block(
                    target,
                    PendingKind::EventCountAwait,
                    crate::objects::carved::test_id(libobject::ObjectType::EVENT_COUNT, 0),
                )
                .unwrap_or_else(|e| panic!("target wait: {:?}", e.code()));
            nucleus
                .pending
                .complete_with_status(terminal, syscall_status::COUNTER_OVERFLOW, 0x1234, 0x5678)
                .unwrap_or_else(|e| panic!("target completion: {:?}", e.code()));
            nucleus.thread_mut(target).unwrap().context = ExecutionContext::Parked {
                saved: target_saved(),
                record: terminal,
            };
            let address_space = nucleus.address_space_mut(target_as).unwrap();
            address_space.set_translation_root(Some(ROOT));
            address_space.set_asid(Some(2));
            nucleus.current_thread = Some(source);
            assert!(nucleus.activations.push(target));
            assert!(nucleus.activations.push(tail));
            test(
                nucleus,
                Waits {
                    source,
                    target,
                    tail,
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
    let source_before = nucleus.thread(waits.source).unwrap().context;
    let target_before = nucleus.thread(waits.target).unwrap().context;
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
    assert_eq!(nucleus.current_thread, Some(waits.source));
    assert_eq!(nucleus.current_thread_table_addr(), table_before);
    assert_eq!(nucleus.thread(waits.source).unwrap().context, source_before);
    assert_eq!(nucleus.thread(waits.target).unwrap().context, target_before);
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
    assert_eq!(nucleus.activations.len(), 2);
    assert_eq!(nucleus.activations.peek(), Some(waits.target));
    assert_eq!(nucleus.activations.pop(), Some(waits.target));
    assert_eq!(nucleus.activations.pop(), Some(waits.tail));
}

#[test_case]
fn resume_rejects_invalid_as_identity_before_consuming_any_state() {
    for invalid in 0..4 {
        with_resume(|nucleus, waits| {
            let live = waits.target_as;
            let identity = match invalid {
                // Another kind at the AddressSpace's address.
                0 => AddressSpaceId {
                    kind: ObjectType::PAGE_TABLE,
                    ..live
                },
                // No object at all.
                1 => AddressSpaceId::UNBOUND,
                // An address no carve could have produced.
                2 => AddressSpaceId {
                    address: live.address + 8,
                    ..live
                },
                // A generation the record never had.
                _ => AddressSpaceId {
                    generation: live.generation + 1,
                    ..live
                },
            };
            nucleus.thread_mut(waits.target).unwrap().address_space = identity;
            reject_unchanged(nucleus, &waits, CapError::InvalidAddressSpace);
        });
    }
}

#[test_case]
fn resume_rejects_retired_and_reused_as_before_consuming_completed_wait() {
    for reuse in [false, true] {
        with_resume(|nucleus, waits| {
            let binding = nucleus
                .address_space(waits.target_as)
                .unwrap()
                .keytable()
                .unwrap();
            crate::retire_address_space(waits.target_as)
                .unwrap_or_else(|e| panic!("AS retirement: {e:?}"));
            if reuse {
                let mut replacement =
                    crate::objects::arch_objects::bound_address_space::<ArchObjectsImpl>(binding);
                replacement.set_translation_root(Some(ROOT));
                replacement.set_asid(Some(3));
                let replacement_id =
                    crate::objects::nucleus::test_arena::carve(replacement).unwrap();
                assert_ne!(replacement_id, waits.target_as);
                assert_ne!(replacement_id.address, waits.target_as.address);
            }
            reject_unchanged(nucleus, &waits, CapError::InvalidAddressSpace);
        });
    }
}

#[test_case]
fn resume_rejects_missing_root_or_asid_without_losing_context_or_pending() {
    for (root, asid) in [(None, None), (None, Some(2)), (Some(ROOT), None)] {
        with_resume(|nucleus, waits| {
            let address_space = nucleus.address_space_mut(waits.target_as).unwrap();
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
            let address_space = nucleus.address_space_mut(waits.target_as).unwrap();
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
                    nucleus.thread_mut(waits.target).unwrap().context = ExecutionContext::Parked {
                        saved,
                        record: waits.terminal,
                    };
                }
                1 => nucleus.thread_mut(waits.target).unwrap().context = ExecutionContext::Running,
                2 | 5 => {
                    let waiting = nucleus
                        .pending
                        .block(
                            waits.target,
                            PendingKind::EventCountAwait,
                            crate::objects::carved::test_id(libobject::ObjectType::EVENT_COUNT, 0),
                        )
                        .unwrap_or_else(|e| panic!("waiting record: {:?}", e.code()));
                    if case == 5 {
                        nucleus
                            .pending
                            .cancel(waiting)
                            .unwrap_or_else(|e| panic!("cancelled record: {:?}", e.code()));
                    }
                    nucleus.thread_mut(waits.target).unwrap().context = ExecutionContext::Parked {
                        saved,
                        record: waiting,
                    };
                }
                3 => {
                    nucleus.thread_mut(waits.target).unwrap().context = ExecutionContext::Parked {
                        saved,
                        record: ObjectId {
                            generation: 0,
                            ..waits.terminal
                        },
                    }
                }
                _ => {
                    nucleus.thread_mut(waits.target).unwrap().context = ExecutionContext::Parked {
                        saved,
                        record: waits.incoming,
                    }
                }
            }
            reject_unchanged(
                nucleus,
                &waits,
                if case == 0 {
                    CapError::InvalidThread
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
        nucleus.thread_mut(waits.target).unwrap().context = ExecutionContext::NotStarted {
            saved: target_saved(),
        };
        nucleus
            .address_space_mut(waits.target_as)
            .unwrap()
            .set_translation_root(None);
        reject_unchanged(nucleus, &waits, CapError::NotMapped);
    });
}

#[test_case]
fn resume_commits_checked_root_asid_and_current_as_table_with_full_error_result() {
    with_resume(|nucleus, waits| {
        let target_binding = nucleus
            .address_space(waits.target_as)
            .unwrap()
            .keytable()
            .unwrap();
        // SAFETY: serial exclusive fixture; the access/guards end before any
        // subsequent state inspection, and no hardware install occurs here.
        let resumed = {
            let access = unsafe { Access::new() };
            nucleus
                .park_and_select(&access, SOURCE_SAVED, waits.incoming)
                .unwrap_or_else(|e| panic!("prepared resume: {:?}", e.code()))
                .expect_resume()
        };
        assert_eq!(resumed.translation.address_space(), waits.target_as);
        assert_eq!(resumed.translation.root(), ROOT);
        assert_eq!(resumed.translation.asid(), 2);
        assert_eq!(resumed.translation.keytable(), target_binding);
        assert_eq!(resumed.current, Some(waits.source));
        assert_eq!(resumed.next, waits.target);
        let mut expected = target_saved();
        expected.gpr[0] = syscall_status::COUNTER_OVERFLOW;
        expected.gpr[1] = 0x1234;
        expected.gpr[2] = 0x5678;
        assert_eq!(resumed.saved, expected);
        assert_eq!(
            resumed.completion.unwrap().kind,
            PendingKind::EventCountAwait
        );
        assert_eq!(nucleus.current_thread, Some(waits.target));
        assert_eq!(
            nucleus.current_thread_table_addr(),
            Some(target_binding.address())
        );
        assert_eq!(
            nucleus.thread(waits.source).unwrap().context,
            ExecutionContext::Parked {
                saved: SOURCE_SAVED,
                record: waits.incoming
            }
        );
        assert_eq!(
            nucleus.thread(waits.target).unwrap().context,
            ExecutionContext::Running
        );
        assert!(nucleus.pending.state(waits.terminal).is_err());
        assert_eq!(nucleus.pending.len(), 1);
        assert_eq!(nucleus.activations.pop(), Some(waits.tail));
        assert!(nucleus.activations.is_empty());
    });
}

#[test_case]
fn first_start_preparation_preserves_all_saved_registers_and_selects_its_table() {
    with_resume(|nucleus, waits| {
        nucleus.thread_mut(waits.target).unwrap().context = ExecutionContext::NotStarted {
            saved: target_saved(),
        };
        // SAFETY: exclusive serial fixture, no overlapping context/guard.
        let resumed = {
            let access = unsafe { Access::new() };
            nucleus
                .park_and_select(&access, SOURCE_SAVED, waits.incoming)
                .unwrap_or_else(|e| panic!("prepared first start: {:?}", e.code()))
                .expect_resume()
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
        let source_as = nucleus.thread(waits.source).unwrap().address_space;
        let source_address_space = nucleus.address_space_mut(source_as).unwrap();
        source_address_space.set_translation_root(Some(ROOT + 0x1000));
        source_address_space.set_asid(Some(1));
        let source_binding = source_address_space.keytable().unwrap();
        let key = nucleus
            .current_thread_table_mut()
            .unwrap()
            .insert(
                KeySlot::SELF_ADDRESS_SPACE,
                KeyEntry::new_carved(source_as, Rights::all(), 0),
                FIXTURE_GUARD,
            )
            .unwrap_or_else(|e| panic!("AS cap: {:?}", e.error.code()));
        let ttbr_before = {
            use aarch64_cpu::registers::{Readable, TTBR0_EL1};
            TTBR0_EL1.get()
        };
        let prepared = match cap_invoke(nucleus, key, 0, &[0; 6]) {
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
        assert_eq!(nucleus.current_thread, Some(waits.source));
        assert_eq!(nucleus.pending.len(), 2);
        assert_eq!(nucleus.activations.len(), 2);
        // Same prepared helper also rejects missing metadata through Activate.
        nucleus.address_space_mut(source_as).unwrap().set_asid(None);
        assert!(matches!(
            cap_invoke(nucleus, key, 0, &[0; 6]),
            Err(CapError::NotMapped)
        ));
        let foreign_key = nucleus
            .current_thread_table_mut()
            .unwrap()
            .insert(
                KeySlot(20),
                KeyEntry::new_carved(waits.target_as, Rights::all(), 0),
                FIXTURE_GUARD,
            )
            .unwrap_or_else(|e| panic!("foreign AS cap: {:?}", e.error.code()));
        assert!(matches!(
            cap_invoke(nucleus, foreign_key, 0, &[0; 6]),
            Err(CapError::InvalidOperation)
        ));
        let no_map = nucleus
            .current_thread_table_mut()
            .unwrap()
            .insert(
                KeySlot(21),
                KeyEntry::new_carved(source_as, Rights(Rights::READ), 0),
                FIXTURE_GUARD,
            )
            .unwrap_or_else(|e| panic!("attenuated AS cap: {:?}", e.error.code()));
        assert!(matches!(
            cap_invoke(nucleus, no_map, 0, &[0; 6]),
            Err(CapError::InsufficientRights)
        ));
        // Foreign guarded table selectors stay ordinary errors, not activation.
        let foreign_guard =
            RawKey::from_parts(FIXTURE_GUARD ^ 1, SIZE_BITS, 20, foreign_key.incarnation());
        assert!(cap_invoke(nucleus, foreign_guard, 0, &[0; 6]).is_err());
        // SAFETY: exclusive serial fixture, dispatch's Access has ended.
        let access = unsafe { Access::new() };
        assert!(matches!(
            prepare_translation_context::<ArchObjectsImpl>(&access, source_as),
            Err(CapError::NotMapped)
        ));
    });
}

/// Map the source `AddressSpace` so Threads there can be selected.
fn map_source_address_space(nucleus: &mut Nucleus<ArchObjectsImpl>, waits: &Waits) {
    let source_as = nucleus.thread(waits.source).unwrap().address_space;
    let address_space = nucleus.address_space_mut(source_as).unwrap();
    address_space.set_translation_root(Some(ROOT + 0x1000));
    address_space.set_asid(Some(1));
}

/// The interrupted state of a preempted source Thread.
const PREEMPTED_SAVED: SavedContext = SavedContext {
    gpr: [0x5A; 30],
    ..SavedContext::el0(0x40_1000, 0x80_0000, 0)
};

#[test_case]
fn preemption_switches_to_the_next_activation_and_resumes_a_preempted_thread_unchanged() {
    with_resume(|nucleus, waits| {
        nucleus.thread_mut(waits.target).unwrap().context = ExecutionContext::Preempted {
            saved: target_saved(),
        };
        // SAFETY: exclusive serial fixture, no overlapping context/guard.
        let resumed = {
            let access = unsafe { Access::new() };
            nucleus
                .preempt_and_select(&access, PREEMPTED_SAVED)
                .unwrap_or_else(|e| panic!("preemption: {:?}", e.code()))
                .expect("another Thread is runnable")
                .expect_resume()
        };
        assert_eq!(resumed.current, Some(waits.source));
        assert_eq!(resumed.next, waits.target);
        // A preempted Thread owes no completion: its registers come back as-is.
        assert_eq!(resumed.saved, target_saved());
        assert_eq!(resumed.completion, None);
        assert_eq!(nucleus.current_thread, Some(waits.target));
        assert_eq!(
            nucleus.thread(waits.source).unwrap().context,
            ExecutionContext::Preempted {
                saved: PREEMPTED_SAVED
            }
        );
        // The preempted Thread belongs to no Scheduler, so nothing queues it
        // again; the other activation still waits.
        assert_eq!(nucleus.activations.pop(), Some(waits.tail));
        assert!(nucleus.activations.is_empty());
        // Unrelated pending records are untouched.
        assert_eq!(nucleus.pending.len(), 2);
        // The switch charged the interval since the fixture started the
        // source (at counter zero) to it alone, and restarted the clock.
        let charged = nucleus.run_time_ticks(waits.source).unwrap();
        assert!(charged > 0);
        assert_eq!(nucleus.run_time_ticks(waits.target), Some(0));
        assert!(nucleus.current_since_ticks >= charged);
    });
}

#[test_case]
fn preemption_with_nothing_runnable_keeps_the_current_thread() {
    with_resume(|nucleus, waits| {
        while nucleus.activations.pop().is_some() {}
        // SAFETY: exclusive serial fixture, no overlapping context/guard.
        let resumed = {
            let access = unsafe { Access::new() };
            nucleus
                .preempt_and_select(&access, PREEMPTED_SAVED)
                .unwrap_or_else(|e| panic!("preemption: {:?}", e.code()))
        };
        assert_eq!(resumed, None);
        assert_eq!(nucleus.current_thread, Some(waits.source));
        assert_eq!(
            nucleus.thread(waits.source).unwrap().context,
            ExecutionContext::Running
        );
        assert!(nucleus.activations.is_empty());
        // No switch happened, but the tick charged the interval the Thread
        // ran alone and restarted the clock.
        assert!(nucleus.run_time_ticks(waits.source).unwrap() > 0);
        assert!(nucleus.current_since_ticks > 0);
    });
}

#[test_case]
fn preemption_rejects_kernel_mode_state_without_changes() {
    with_resume(|nucleus, waits| {
        let kernel_mode = SavedContext {
            origin: ExceptionOrigin::CurrentSpx,
            ..PREEMPTED_SAVED
        };
        // SAFETY: exclusive serial fixture, no overlapping context/guard.
        let result = {
            let access = unsafe { Access::new() };
            nucleus.preempt_and_select(&access, kernel_mode)
        };
        assert_eq!(
            result.expect_err("kernel-mode preemption").code(),
            CapError::InvalidThread.code()
        );
        assert_eq!(nucleus.current_thread, Some(waits.source));
        assert_eq!(nucleus.activations.len(), 2);
    });
}

#[test_case]
fn parking_with_nothing_runnable_idles_and_the_next_runnable_thread_ends_it() {
    with_resume(|nucleus, waits| {
        map_source_address_space(nucleus, &waits);
        while nucleus.activations.pop().is_some() {}
        // SAFETY: exclusive serial fixture, no overlapping context/guard.
        let access = unsafe { Access::new() };
        let switch = nucleus
            .park_and_select(&access, SOURCE_SAVED, waits.incoming)
            .unwrap_or_else(|e| panic!("park into idle: {:?}", e.code()));
        // The park is committed, and no Thread is current.
        assert_eq!(switch, Switch::Idle);
        assert_eq!(nucleus.current_thread, None);
        assert!(matches!(
            nucleus.thread(waits.source).unwrap().context,
            ExecutionContext::Parked { .. }
        ));
        // Nothing runnable yet: idle goes on.
        assert!(
            nucleus
                .select_after_idle(&access)
                .unwrap_or_else(|e| panic!("idle: {:?}", e.code()))
                .is_none()
        );
        assert_eq!(nucleus.current_thread, None);

        // A Thread becoming runnable ends idle; the selection is not owed a
        // stopped Thread.
        nucleus.thread_mut(waits.target).unwrap().context = ExecutionContext::Preempted {
            saved: target_saved(),
        };
        assert!(nucleus.activations.push(waits.target));
        let resumed = nucleus
            .select_after_idle(&access)
            .unwrap_or_else(|e| panic!("leave idle: {:?}", e.code()))
            .expect("the target is runnable");
        assert_eq!(resumed.current, None);
        assert_eq!(resumed.next, waits.target);
        assert_eq!(resumed.saved, target_saved());
        assert_eq!(nucleus.current_thread, Some(waits.target));
        assert!(nucleus.activations.is_empty());
    });
}

#[test_case]
fn idle_time_is_metered_apart_from_interrupt_time() {
    with_resume(|nucleus, waits| {
        map_source_address_space(nucleus, &waits);
        while nucleus.activations.pop().is_some() {}
        // SAFETY: exclusive serial fixture, no overlapping context/guard.
        let access = unsafe { Access::new() };
        let switch = nucleus
            .park_and_select(&access, SOURCE_SAVED, waits.incoming)
            .unwrap_or_else(|e| panic!("park into idle: {:?}", e.code()));
        assert_eq!(switch, Switch::Idle);
        assert_eq!(nucleus.idle_ticks, 0);
        // Pretend 10 ms went by, 4 ms of them in interrupt handlers.
        nucleus.current_since_ticks = nucleus
            .counter_ticks()
            .saturating_sub(ticks_for_ms(nucleus, 10));
        nucleus.account_interrupts(&[], ticks_for_ms(nucleus, 4));
        // The interrupts moved the start of the idle interval 4 ms ahead.
        let since_before = nucleus.current_since_ticks - ticks_for_ms(nucleus, 4);
        nucleus.thread_mut(waits.target).unwrap().context = ExecutionContext::Preempted {
            saved: target_saved(),
        };
        assert!(nucleus.activations.push(waits.target));
        nucleus
            .select_after_idle(&access)
            .unwrap_or_else(|e| panic!("leave idle: {:?}", e.code()))
            .expect("the target is runnable");
        // Idle time is what is left of the interval, and the interrupts are
        // counted where they are.
        assert!(nucleus.idle_ticks >= ticks_for_ms(nucleus, 6));
        assert!(nucleus.idle_ticks < ticks_for_ms(nucleus, 7));
        assert_eq!(nucleus.interrupt_ticks, ticks_for_ms(nucleus, 4));
        // The next Thread is charged from now, not from before the wait.
        assert!(nucleus.current_since_ticks >= since_before + ticks_for_ms(nucleus, 10));
    });
}

// ═══════════════════════════════════════════════════════════════
// TIME CHARGING AND BUDGET ELIGIBILITY
// ═══════════════════════════════════════════════════════════════

/// Counter ticks in `milliseconds` of the running counter.
fn ticks_for_ms(nucleus: &Nucleus<ArchObjectsImpl>, milliseconds: u64) -> u64 {
    nucleus.counter_frequency() / 1_000 * milliseconds
}

/// Pretend the current Thread has been running for `milliseconds`.
fn run_for_ms(nucleus: &mut Nucleus<ArchObjectsImpl>, milliseconds: u64) {
    let elapsed = ticks_for_ms(nucleus, milliseconds);
    nucleus.current_since_ticks = nucleus.counter_ticks().saturating_sub(elapsed);
}

/// Bind a fresh Time holding `budget_ns` to `thread`; its identity.
fn bind_time(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    thread: ThreadId,
    budget_ns: u64,
) -> crate::objects::carved::CarvedId {
    let id = test_arena::carve(Time::root(0, budget_ns)).expect("arena exhausted");
    nucleus.thread_mut(thread).unwrap().time = Some(id);
    id
}

/// The remaining budget of the Time `id`, in nanoseconds.
fn remaining_ns(nucleus: &Nucleus<ArchObjectsImpl>, id: crate::objects::carved::CarvedId) -> u64 {
    // SAFETY: a test-arena record nothing else references right now.
    let time = unsafe { Carved::<Time>::validate(id) }
        .ok()
        .expect("Time carved");
    // SAFETY: as above.
    unsafe { (*time).remaining_ns(nucleus.counter_frequency()) }
}

#[test_case]
fn a_tick_charges_a_thread_running_alone_to_its_bound_time() {
    with_resume(|nucleus, waits| {
        while nucleus.activations.pop().is_some() {}
        let time = bind_time(nucleus, waits.source, 1_000_000_000);
        run_for_ms(nucleus, 3);
        // SAFETY: exclusive serial fixture, no overlapping context/guard.
        let resumed = {
            let access = unsafe { Access::new() };
            nucleus
                .preempt_and_select(&access, PREEMPTED_SAVED)
                .unwrap_or_else(|e| panic!("tick: {:?}", e.code()))
        };
        // Nothing else is runnable and the budget holds, so it keeps running.
        assert_eq!(resumed, None);
        assert_eq!(nucleus.current_thread, Some(waits.source));
        let remaining = remaining_ns(nucleus, time);
        assert!(remaining <= 1_000_000_000 - 3_000_000, "{remaining}");
        assert!(remaining > 900_000_000, "{remaining}");
        assert!(nucleus.run_time_ticks(waits.source).unwrap() >= ticks_for_ms(nucleus, 3));
        assert!(!nucleus.thread(waits.source).unwrap().awaiting_budget);
    });
}

#[test_case]
fn an_exhausted_thread_is_switched_away_from_and_kept_off_the_queue() {
    with_resume(|nucleus, waits| {
        map_source_address_space(nucleus, &waits);
        while nucleus.activations.pop().is_some() {}
        bind_time(nucleus, waits.source, 1_000_000);
        run_for_ms(nucleus, 5);
        // SAFETY: exclusive serial fixture, no overlapping context/guard.
        let resumed = {
            let access = unsafe { Access::new() };
            nucleus
                .preempt_and_select(&access, PREEMPTED_SAVED)
                .unwrap_or_else(|e| panic!("tick: {:?}", e.code()))
                .expect("an exhausted Thread must stop")
        };
        // Nothing can take over: the nucleus idles.
        assert_eq!(resumed, Switch::Idle);
        assert_eq!(nucleus.current_thread, None);
        // It stays runnable but waits for budget instead of queueing.
        assert!(nucleus.activations.is_empty());
        let source = nucleus.thread(waits.source).unwrap();
        assert!(source.awaiting_budget);
        assert_eq!(
            source.context,
            ExecutionContext::Preempted {
                saved: PREEMPTED_SAVED
            }
        );
    });
}

#[test_case]
fn an_exhausted_thread_with_a_queue_behind_it_is_not_requeued() {
    with_resume(|nucleus, waits| {
        bind_time(nucleus, waits.source, 1_000_000);
        run_for_ms(nucleus, 5);
        nucleus.thread_mut(waits.target).unwrap().context = ExecutionContext::Preempted {
            saved: target_saved(),
        };
        // SAFETY: exclusive serial fixture, no overlapping context/guard.
        let resumed = {
            let access = unsafe { Access::new() };
            nucleus
                .preempt_and_select(&access, PREEMPTED_SAVED)
                .unwrap_or_else(|e| panic!("tick: {:?}", e.code()))
                .expect("another Thread is runnable")
                .expect_resume()
        };
        assert_eq!(resumed.next, waits.target);
        // Only the former tail remains queued; the exhausted source waits.
        assert_eq!(nucleus.activations.pop(), Some(waits.tail));
        assert!(nucleus.activations.is_empty());
        assert!(nucleus.thread(waits.source).unwrap().awaiting_budget);
    });
}

#[test_case]
fn budget_arriving_clears_the_wait_for_budget_without_queueing_anything() {
    with_resume(|nucleus, waits| {
        let time = bind_time(nucleus, waits.source, 1_000_000);
        run_for_ms(nucleus, 5);
        nucleus.charge_current(waits.source);
        assert!(nucleus.thread_exhausted(waits.source));
        // A stopped, exhausted Thread waits for budget.
        nucleus.thread_mut(waits.source).unwrap().context = ExecutionContext::Preempted {
            saved: PREEMPTED_SAVED,
        };
        nucleus.current_thread = Some(waits.target);
        let queued_before = nucleus.activations.len();
        nucleus.make_runnable(waits.source);
        assert_eq!(nucleus.activations.len(), queued_before);
        assert!(nucleus.thread(waits.source).unwrap().awaiting_budget);
        // Budget has not arrived yet.
        nucleus.budget_arrived(waits.source);
        assert!(nucleus.thread(waits.source).unwrap().awaiting_budget);

        // Refill the Time the way a Transfer does.
        let frequency = nucleus.counter_frequency();
        // SAFETY: a test-arena record nothing else references right now.
        unsafe {
            let bound = Carved::<Time>::validate(time).ok().expect("Time carved");
            let (_, refilled) =
                Time::transfer(&Time::root(0, u64::MAX), &*bound, 50_000_000, frequency)
                    .ok()
                    .expect("refill");
            *bound = refilled;
        }
        nucleus.budget_arrived(waits.source);
        assert!(!nucleus.thread(waits.source).unwrap().awaiting_budget);
        // Nothing is queued: only a dispatcher runs Threads, and a Thread
        // outside every Scheduler has none.
        assert_eq!(nucleus.activations.len(), queued_before);
        assert_eq!(nucleus.activations.pop(), Some(waits.target));
        assert_eq!(nucleus.activations.pop(), Some(waits.tail));
        assert_eq!(nucleus.activations.pop(), None);
    });
}

#[test_case]
fn a_thread_with_no_bound_time_is_never_exhausted_and_its_run_time_is_metered() {
    with_resume(|nucleus, waits| {
        run_for_ms(nucleus, 4);
        nucleus.charge_current(waits.source);
        // No Time, so it is never exhausted, but its run time counts like
        // any other Thread's: it measures load.
        assert!(!nucleus.thread_exhausted(waits.source));
        assert!(nucleus.run_time_ticks(waits.source).unwrap() >= ticks_for_ms(nucleus, 4));
        assert!(nucleus.run_time_ns(waits.source).unwrap() >= 4_000_000);
        while nucleus.activations.pop().is_some() {}
        // A woken Thread outside every Scheduler is published and waits: no
        // queue runs it.
        nucleus.make_runnable(waits.target);
        assert!(nucleus.activations.is_empty());
    });
}

#[test_case]
fn interrupt_time_is_not_charged_to_the_interrupted_thread() {
    with_resume(|nucleus, waits| {
        while nucleus.activations.pop().is_some() {}
        let time = bind_time(nucleus, waits.source, 1_000_000_000);
        // The Thread started 10 ms ago, and 4 ms of that went to interrupts.
        run_for_ms(nucleus, 10);
        let started = nucleus.current_since_ticks;
        nucleus.account_interrupts(&[(5, 1_000), (6, 500)], ticks_for_ms(nucleus, 4));
        assert_eq!(
            nucleus.current_since_ticks,
            started + ticks_for_ms(nucleus, 4)
        );
        nucleus.charge_current(waits.source);
        let charged = nucleus.run_time_ticks(waits.source).unwrap();
        assert!(charged >= ticks_for_ms(nucleus, 6), "{charged}");
        assert!(charged < ticks_for_ms(nucleus, 10), "{charged}");
        let spent = 1_000_000_000 - remaining_ns(nucleus, time);
        assert!(spent >= 6_000_000 && spent < 10_000_000, "{spent}");
    });
}

#[test_case]
fn interrupt_time_never_moves_the_interval_past_now() {
    with_resume(|nucleus, waits| {
        run_for_ms(nucleus, 1);
        // More interrupt time than has elapsed (a clock skew) clamps to now.
        nucleus.account_interrupts(&[], ticks_for_ms(nucleus, 1_000));
        assert!(nucleus.current_since_ticks <= nucleus.counter_ticks());
        nucleus.charge_current(waits.source);
        assert!(nucleus.run_time_ticks(waits.source).unwrap() < ticks_for_ms(nucleus, 1));
    });
}

// ═══════════════════════════════════════════════════════════════
// TIMER DEADLINE
// ═══════════════════════════════════════════════════════════════

/// A slice of 10 ms, in ticks of the running counter.
fn slice(nucleus: &Nucleus<ArchObjectsImpl>) -> u64 {
    ticks_for_ms(nucleus, 10)
}

/// Ticks of execution a fresh `budget_ns` Time allows.
fn budget_ticks(nucleus: &Nucleus<ArchObjectsImpl>, budget_ns: u64) -> u64 {
    Time::root(0, budget_ns)
        .ticks_until_exhausted(nucleus.counter_frequency())
        .expect("a finite budget")
}

#[test_case]
fn a_thread_running_alone_on_unlimited_time_needs_no_timer() {
    with_resume(|nucleus, waits| {
        while nucleus.activations.pop().is_some() {}
        nucleus.current_since_ticks = 5_000;
        let slice = slice(nucleus);
        assert_eq!(nucleus.timer_deadline_ticks(slice), None);
        // The same on an infinite Time.
        bind_time(nucleus, waits.source, u64::MAX);
        assert_eq!(nucleus.timer_deadline_ticks(slice), None);
    });
}

#[test_case]
fn an_idle_nucleus_needs_no_timer_unless_a_deadline_is_pending() {
    with_resume(|nucleus, _waits| {
        nucleus.current_thread = None;
        let slice = slice(nucleus);
        // Idle waits for an interrupt, whatever is queued.
        assert!(!nucleus.activations.is_empty());
        assert_eq!(nucleus.timer_deadline_ticks(slice), None);
    });
}

#[test_case]
fn a_queued_thread_bounds_the_slice_from_when_the_current_thread_started() {
    with_resume(|nucleus, _waits| {
        nucleus.current_since_ticks = 5_000;
        let slice = slice(nucleus);
        assert!(!nucleus.activations.is_empty());
        assert_eq!(nucleus.timer_deadline_ticks(slice), Some(5_000 + slice));
        // A later start moves the deadline with it.
        nucleus.current_since_ticks = 9_000;
        assert_eq!(nucleus.timer_deadline_ticks(slice), Some(9_000 + slice));
    });
}

#[test_case]
fn a_finite_budget_sets_a_deadline_at_exhaustion_even_when_alone() {
    with_resume(|nucleus, waits| {
        while nucleus.activations.pop().is_some() {}
        nucleus.current_since_ticks = 5_000;
        bind_time(nucleus, waits.source, 2_000_000);
        let slice = slice(nucleus);
        let left = budget_ticks(nucleus, 2_000_000);
        assert_eq!(nucleus.timer_deadline_ticks(slice), Some(5_000 + left));
    });
}

#[test_case]
fn the_earlier_of_the_slice_and_the_exhaustion_wins() {
    with_resume(|nucleus, waits| {
        nucleus.current_since_ticks = 5_000;
        let slice = slice(nucleus);
        // Contended, with less budget than a slice: exhaustion comes first.
        bind_time(nucleus, waits.source, 2_000_000);
        let left = budget_ticks(nucleus, 2_000_000);
        assert!(left < slice);
        assert_eq!(nucleus.timer_deadline_ticks(slice), Some(5_000 + left));
        // With more budget than a slice, the slice ends first.
        bind_time(nucleus, waits.source, 1_000_000_000);
        assert_eq!(nucleus.timer_deadline_ticks(slice), Some(5_000 + slice));
    });
}

#[test_case]
fn an_exhausted_thread_with_nothing_to_switch_to_still_asks_for_a_timer() {
    with_resume(|nucleus, waits| {
        // Nothing is queued, but it stops all the same: the nucleus idles.
        while nucleus.activations.pop().is_some() {}
        bind_time(nucleus, waits.source, 1);
        run_for_ms(nucleus, 1);
        nucleus.charge_current(waits.source);
        assert!(nucleus.thread_exhausted(waits.source));
        let slice = slice(nucleus);
        assert!(nucleus.timer_deadline_ticks(slice).is_some());
    });
}

#[test_case]
fn interrupt_time_pushes_the_exhaustion_deadline_out() {
    with_resume(|nucleus, waits| {
        while nucleus.activations.pop().is_some() {}
        nucleus.current_since_ticks = 5_000;
        bind_time(nucleus, waits.source, 2_000_000);
        let slice = slice(nucleus);
        let before = nucleus.timer_deadline_ticks(slice).unwrap();
        // Time spent in interrupts is not the Thread's, so it gets it back.
        nucleus.account_interrupts(&[], 700);
        assert_eq!(nucleus.timer_deadline_ticks(slice), Some(before + 700));
    });
}

// ═══════════════════════════════════════════════════════════════
// DISPATCHER
// ═══════════════════════════════════════════════════════════════

/// A bound dispatcher (the fixture's `source` Thread, running) and two
/// Threads it dispatches: `target` (a woken waiter) and `tail`.
struct Dispatching {
    scheduler: RawKey,
    scheduler_id: CarvedId,
    target_key: RawKey,
    tail_key: RawKey,
}

fn with_dispatcher(nucleus: &mut Nucleus<ArchObjectsImpl>, waits: &Waits) -> Dispatching {
    let scheduler_id = test_arena::carve(Scheduler::root()).expect("arena exhausted");
    // The dispatcher and its Threads belong to the Scheduler; its Threads
    // are not in the first-in, first-out queue.
    for thread in [waits.source, waits.target, waits.tail] {
        nucleus.thread_mut(thread).unwrap().scheduler = Some(scheduler_id);
    }
    while nucleus.activations.pop().is_some() {}
    let source_as = nucleus.thread(waits.source).unwrap().address_space;
    let address_space = nucleus.address_space_mut(source_as).unwrap();
    address_space.set_translation_root(Some(ROOT + 0x1000));
    address_space.set_asid(Some(1));
    nucleus.thread_mut(waits.tail).unwrap().context = ExecutionContext::Preempted {
        saved: target_saved(),
    };
    // SAFETY: a test-arena Scheduler nothing else references now.
    unsafe {
        (*Carved::<Scheduler>::validate(scheduler_id)
            .ok()
            .expect("Scheduler carved"))
        .bind_dispatcher(waits.source)
        .ok()
        .expect("bind");
    }
    let mut install = |slot: u32, entry: KeyEntry| {
        nucleus
            .current_thread_table_mut()
            .unwrap()
            .insert(KeySlot(slot), entry, FIXTURE_GUARD)
            .unwrap_or_else(|failure| panic!("install: {:?}", failure.error.code()))
    };
    Dispatching {
        scheduler: install(50, KeyEntry::new_carved(scheduler_id, Rights::all(), 0)),
        scheduler_id,
        target_key: install(51, KeyEntry::new_thread(waits.target, Rights::all(), 0)),
        tail_key: install(52, KeyEntry::new_thread(waits.tail, Rights::all(), 0)),
    }
}

fn dispatch_call(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    dispatching: &Dispatching,
    thread: Option<RawKey>,
    limit_ns: u64,
    generation: u64,
) -> Result<InvokeOutcome, CapError> {
    cap_invoke(
        nucleus,
        dispatching.scheduler,
        SchedulerOp::Dispatch as u64,
        &[
            thread.map_or(0, |thread| thread.to_wire()),
            0,
            limit_ns,
            generation,
            0,
            0,
        ],
    )
}

/// Dispatch `thread` and park the dispatcher, as the entry code does; the
/// resume that results.
fn dispatch_and_park(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    dispatching: &Dispatching,
    thread: RawKey,
    limit_ns: u64,
) -> crate::objects::resume::PreparedResume {
    let Ok(InvokeOutcome::Blocked(record)) =
        dispatch_call(nucleus, dispatching, Some(thread), limit_ns, 0)
    else {
        panic!("Dispatch should block the dispatcher");
    };
    // SAFETY: exclusive serial fixture, no overlapping context/guard.
    let access = unsafe { Access::new() };
    nucleus
        .park_and_select(&access, SOURCE_SAVED, record)
        .unwrap_or_else(|e| panic!("park onto the dispatched Thread: {:?}", e.code()))
        .expect_resume()
}

fn scheduler_of(dispatching: &Dispatching) -> &'static mut Scheduler {
    // SAFETY: a test-arena Scheduler; serial tests.
    unsafe {
        &mut *Carved::<Scheduler>::validate(dispatching.scheduler_id)
            .ok()
            .expect("Scheduler carved")
    }
}

#[test_case]
fn dispatch_parks_the_dispatcher_and_runs_the_named_thread() {
    with_resume(|nucleus, waits| {
        let dispatching = with_dispatcher(nucleus, &waits);
        let resumed = dispatch_and_park(nucleus, &dispatching, dispatching.tail_key, 5_000_000);
        assert_eq!(resumed.current, Some(waits.source));
        assert_eq!(resumed.next, waits.tail);
        assert_eq!(nucleus.current_thread, Some(waits.tail));
        let dispatched = nucleus.dispatched.expect("a dispatched Thread");
        assert_eq!(dispatched.thread, waits.tail);
        assert!(dispatched.deadline_ticks.is_some());
        assert!(scheduler_of(&dispatching).parked().is_some());
        assert!(matches!(
            nucleus.thread(waits.source).unwrap().context,
            ExecutionContext::Parked { .. }
        ));
        // The dispatched Thread is not in the queue.
        assert!(nucleus.activations.is_empty());
    });
}

#[test_case]
fn the_dispatched_thread_blocking_activates_the_dispatcher() {
    with_resume(|nucleus, waits| {
        let dispatching = with_dispatcher(nucleus, &waits);
        dispatch_and_park(nucleus, &dispatching, dispatching.tail_key, 5_000_000);
        // The dispatched Thread waits on something.
        let record = nucleus
            .pending
            .block(
                waits.tail,
                PendingKind::NotificationWait,
                crate::objects::carved::test_id(ObjectType::NOTIFICATION, 0),
            )
            .unwrap_or_else(|e| panic!("wait: {:?}", e.code()));
        // SAFETY: exclusive serial fixture, no overlapping context/guard.
        let resumed = {
            let access = unsafe { Access::new() };
            nucleus
                .park_and_select(&access, SOURCE_SAVED, record)
                .unwrap_or_else(|e| panic!("park: {:?}", e.code()))
                .expect_resume()
        };
        // The dispatcher comes back, told the Thread blocked.
        assert_eq!(resumed.next, waits.source);
        let completion = resumed.completion.expect("the Dispatch call completes");
        assert_eq!(completion.kind, PendingKind::SchedulerDispatch);
        assert_eq!(completion.status, syscall_status::SUCCESS);
        assert_eq!(completion.result0, activation_reason::BLOCKED);
        assert!(nucleus.dispatched.is_none());
        assert!(scheduler_of(&dispatching).parked().is_none());
        assert!(nucleus.activations.is_empty());
    });
}

#[test_case]
fn reaching_the_limit_or_the_budget_activates_the_dispatcher() {
    // The limit.
    with_resume(|nucleus, waits| {
        let dispatching = with_dispatcher(nucleus, &waits);
        dispatch_and_park(nucleus, &dispatching, dispatching.tail_key, 5_000_000);
        nucleus.dispatched.as_mut().unwrap().deadline_ticks = Some(0);
        // SAFETY: exclusive serial fixture, no overlapping context/guard.
        let resumed = {
            let access = unsafe { Access::new() };
            nucleus
                .preempt_and_select(&access, PREEMPTED_SAVED)
                .unwrap_or_else(|e| panic!("tick: {:?}", e.code()))
                .expect("the limit stops the Thread")
                .expect_resume()
        };
        assert_eq!(resumed.next, waits.source);
        assert_eq!(
            resumed.completion.expect("completes").result0,
            activation_reason::LIMIT
        );
        // The stopped Thread is runnable but not queued: it waits for a dispatch.
        assert!(nucleus.activations.is_empty());
        assert!(!nucleus.thread(waits.tail).unwrap().queued);
    });
    // The budget.
    with_resume(|nucleus, waits| {
        let dispatching = with_dispatcher(nucleus, &waits);
        bind_time(nucleus, waits.tail, 1_000_000);
        dispatch_and_park(nucleus, &dispatching, dispatching.tail_key, u64::MAX);
        run_for_ms(nucleus, 5);
        // SAFETY: exclusive serial fixture, no overlapping context/guard.
        let resumed = {
            let access = unsafe { Access::new() };
            nucleus
                .preempt_and_select(&access, PREEMPTED_SAVED)
                .unwrap_or_else(|e| panic!("tick: {:?}", e.code()))
                .expect("exhaustion stops the Thread")
                .expect_resume()
        };
        assert_eq!(resumed.next, waits.source);
        assert_eq!(
            resumed.completion.expect("completes").result0,
            activation_reason::EXHAUSTED
        );
        assert!(nucleus.thread(waits.tail).unwrap().awaiting_budget);
    });
}

#[test_case]
fn an_activation_with_nobody_waiting_makes_the_next_dispatch_return_at_once() {
    with_resume(|nucleus, waits| {
        let dispatching = with_dispatcher(nucleus, &waits);
        // The dispatcher is running, not waiting: the activation is kept.
        nucleus.request_activation(dispatching.scheduler_id, activation_reason::EVENT, 0);
        assert_eq!(
            scheduler_of(&dispatching).pending(),
            activation_reason::EVENT
        );
        let Ok(InvokeOutcome::Complete((reasons, _events))) =
            dispatch_call(nucleus, &dispatching, Some(dispatching.tail_key), 1_000, 0)
        else {
            panic!("a pending activation returns at once");
        };
        assert_eq!(reasons, activation_reason::EVENT);
        assert_eq!(scheduler_of(&dispatching).pending(), 0);
        // Nothing was dispatched, and the next call blocks.
        assert!(nucleus.dispatch_next.is_none());
        assert!(matches!(
            dispatch_call(nucleus, &dispatching, Some(dispatching.tail_key), 1_000, 0),
            Ok(InvokeOutcome::Blocked(_))
        ));
    });
}

#[test_case]
fn dispatch_refuses_a_stranger_a_stale_generation_and_a_thread_that_cannot_run() {
    with_resume(|nucleus, waits| {
        let dispatching = with_dispatcher(nucleus, &waits);
        let invalid = |result: Result<InvokeOutcome, CapError>| {
            matches!(result, Err(CapError::InvalidOperation))
        };
        // A stale generation reports the current one and runs nothing.
        assert!(matches!(
            dispatch_call(nucleus, &dispatching, Some(dispatching.tail_key), 1_000, 99),
            Err(CapError::StaleGeneration { current: 0 })
        ));
        // A zero limit, another CPU, a Thread that is not dispatchable.
        assert!(invalid(dispatch_call(
            nucleus,
            &dispatching,
            Some(dispatching.tail_key),
            0,
            0
        )));
        assert!(invalid(cap_invoke(
            nucleus,
            dispatching.scheduler,
            SchedulerOp::Dispatch as u64,
            &[dispatching.tail_key.to_wire(), 0, 1_000, 0, 1, 0],
        )));
        nucleus.thread_mut(waits.tail).unwrap().context = ExecutionContext::Inactive;
        assert!(invalid(dispatch_call(
            nucleus,
            &dispatching,
            Some(dispatching.tail_key),
            1_000,
            0
        )));
        nucleus.thread_mut(waits.tail).unwrap().context = ExecutionContext::Preempted {
            saved: target_saved(),
        };
        // A Time that is not the Thread's own.
        let other = test_arena::carve(Time::root(0, 1_000)).expect("arena exhausted");
        let other_key = nucleus
            .current_thread_table_mut()
            .unwrap()
            .insert(
                KeySlot(53),
                KeyEntry::new_carved(other, Rights::all(), 0),
                FIXTURE_GUARD,
            )
            .unwrap_or_else(|f| panic!("install: {:?}", f.error.code()));
        assert!(invalid(cap_invoke(
            nucleus,
            dispatching.scheduler,
            SchedulerOp::Dispatch as u64,
            &[
                dispatching.tail_key.to_wire(),
                other_key.to_wire(),
                1_000,
                0,
                0,
                0
            ],
        )));
        // Only the dispatcher dispatches.
        nucleus.current_thread = Some(waits.tail);
        assert!(invalid(dispatch_call(
            nucleus,
            &dispatching,
            Some(dispatching.target_key),
            1_000,
            0
        )));
        // Nothing above parked or ran anything.
        assert!(nucleus.dispatch_next.is_none());
        assert!(scheduler_of(&dispatching).parked().is_none());
    });
}

#[test_case]
fn the_mask_decides_which_wakeups_preempt_the_dispatched_thread() {
    with_resume(|nucleus, waits| {
        let dispatching = with_dispatcher(nucleus, &waits);
        dispatch_and_park(nucleus, &dispatching, dispatching.tail_key, u64::MAX);
        // An empty mask: a Thread becoming runnable does not disturb the run.
        let target = waits.target;
        nucleus.make_runnable(target);
        assert!(scheduler_of(&dispatching).parked().is_some());
        assert!(!nucleus.preempt_requested);

        // THREAD_RUNNABLE: it activates the dispatcher at once.
        cap_invoke(
            nucleus,
            dispatching.scheduler,
            SchedulerOp::ConfigureActivation as u64,
            &[activation_mask::THREAD_RUNNABLE, 0, 0, 0, 0, 0],
        )
        .ok()
        .expect("ConfigureActivation");
        nucleus.make_runnable(target);
        assert!(scheduler_of(&dispatching).parked().is_none());
        assert!(nucleus.preempt_requested);
        assert_eq!(nucleus.activations.peek(), Some(waits.source));
        // The timer is asked to fire at once.
        let since = nucleus.current_since_ticks;
        assert_eq!(nucleus.timer_deadline_ticks(1_000_000), Some(since));
        // The tick switches to the dispatcher with the event as the reason.
        // SAFETY: exclusive serial fixture, no overlapping context/guard.
        let resumed = {
            let access = unsafe { Access::new() };
            nucleus
                .preempt_and_select(&access, PREEMPTED_SAVED)
                .unwrap_or_else(|e| panic!("tick: {:?}", e.code()))
                .expect("the dispatcher preempts")
                .expect_resume()
        };
        assert_eq!(resumed.next, waits.source);
        assert_eq!(
            resumed.completion.expect("completes").result0,
            activation_reason::EVENT
        );
        assert!(!nucleus.preempt_requested);
    });
}

#[test_case]
fn a_dispatcher_waiting_with_nothing_dispatched_wakes_for_any_runnable_thread() {
    with_resume(|nucleus, waits| {
        let dispatching = with_dispatcher(nucleus, &waits);

        let Ok(InvokeOutcome::Blocked(record)) = dispatch_call(nucleus, &dispatching, None, 1, 0)
        else {
            panic!("waiting blocks the dispatcher");
        };
        // SAFETY: exclusive serial fixture, no overlapping context/guard.
        let resumed = {
            let access = unsafe { Access::new() };
            nucleus
                .park_and_select(&access, SOURCE_SAVED, record)
                .unwrap_or_else(|e| panic!("park: {:?}", e.code()))
        };
        // Nothing runs: the nucleus idles.
        assert_eq!(resumed, Switch::Idle);
        assert!(nucleus.dispatched.is_none());
        // The mask is empty, but nothing is running: any runnable Thread wakes it.
        nucleus.make_runnable(waits.target);
        assert!(scheduler_of(&dispatching).parked().is_none());
        assert_eq!(nucleus.activations.peek(), Some(waits.source));
    });
}

#[test_case]
fn binding_a_dispatcher_runs_it_next() {
    with_resume(|nucleus, waits| {
        let scheduler_id = test_arena::carve(Scheduler::root()).expect("arena exhausted");
        // Nothing else is waiting to run.
        while nucleus.activations.pop().is_some() {}
        let source_as = nucleus.thread(waits.source).unwrap().address_space;
        let dispatcher = nucleus.create_thread(source_as).expect("dispatcher Thread");
        {
            let thread = nucleus.thread_mut(dispatcher).unwrap();
            thread.scheduler = Some(scheduler_id);
            thread.context = ExecutionContext::Configured {
                saved: SavedContext::el1t(0xC0000, 0xD0000),
            };
        }
        nucleus
            .bind_dispatcher(scheduler_id, dispatcher)
            .ok()
            .expect("bind");
        // The dispatcher runs first, and it is the only one waiting.
        assert_eq!(nucleus.activations.len(), 1);
        assert_eq!(nucleus.activations.pop(), Some(dispatcher));
        assert!(matches!(
            nucleus.thread(dispatcher).unwrap().context,
            ExecutionContext::NotStarted { .. }
        ));
        // A second bind, and a bind of a started Thread, are refused.
        assert!(nucleus.bind_dispatcher(scheduler_id, waits.source).is_err());
    });
}

fn arm_timer_call(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    dispatching: &Dispatching,
    deadline_ns: u64,
) -> Result<InvokeOutcome, CapError> {
    cap_invoke(
        nucleus,
        dispatching.scheduler,
        SchedulerOp::ArmTimer as u64,
        &[deadline_ns, 0, 0, 0, 0, 0],
    )
}

#[test_case]
fn arm_timer_belongs_to_the_dispatcher_and_replaces_its_deadline() {
    with_resume(|nucleus, waits| {
        let dispatching = with_dispatcher(nucleus, &waits);
        let far = nucleus.current_time_ns() + 3_600_000_000_000;

        // A Scheduler whose dispatcher is not the caller, or a nonzero
        // reserved register: refused.
        let other = test_arena::carve(Scheduler::root()).expect("arena exhausted");
        let other_key = nucleus
            .current_thread_table_mut()
            .unwrap()
            .insert(
                KeySlot(60),
                KeyEntry::new_carved(other, Rights::all(), 0),
                FIXTURE_GUARD,
            )
            .unwrap_or_else(|failure| panic!("install: {:?}", failure.error.code()));
        assert!(matches!(
            cap_invoke(
                nucleus,
                other_key,
                SchedulerOp::ArmTimer as u64,
                &[far, 0, 0, 0, 0, 0],
            ),
            Err(CapError::InvalidOperation)
        ));
        assert!(matches!(
            cap_invoke(
                nucleus,
                dispatching.scheduler,
                SchedulerOp::ArmTimer as u64,
                &[far, 1, 0, 0, 0, 0],
            ),
            Err(CapError::InvalidOperation)
        ));
        assert_eq!(nucleus.next_wakeup_ticks(), None);

        // Arming reports the time now and no previous deadline.
        let Ok(InvokeOutcome::Complete((now_ns, previous_ns))) =
            arm_timer_call(nucleus, &dispatching, far)
        else {
            panic!("ArmTimer completes");
        };
        assert!(now_ns > 0 && now_ns < far);
        assert_eq!(previous_ns, 0);
        assert!(nucleus.next_wakeup_ticks().is_some());

        // Re-arming replaces it and reports the earlier deadline, rounded up
        // to the counter's resolution.
        let sooner = far - 1_000_000_000;
        let Ok(InvokeOutcome::Complete((_, previous_ns))) =
            arm_timer_call(nucleus, &dispatching, sooner)
        else {
            panic!("ArmTimer completes");
        };
        assert!(previous_ns >= far && previous_ns < far + 1_000_000);
        // Zero disarms and reports what was armed.
        let Ok(InvokeOutcome::Complete((_, previous_ns))) =
            arm_timer_call(nucleus, &dispatching, 0)
        else {
            panic!("ArmTimer completes");
        };
        assert!(previous_ns >= sooner && previous_ns < sooner + 1_000_000);
        assert_eq!(nucleus.next_wakeup_ticks(), None);
        let Ok(InvokeOutcome::Complete((_, previous_ns))) =
            arm_timer_call(nucleus, &dispatching, 0)
        else {
            panic!("ArmTimer completes");
        };
        assert_eq!(previous_ns, 0);
    });
}

#[test_case]
fn an_expired_timer_activates_a_parked_dispatcher_whatever_the_mask() {
    with_resume(|nucleus, waits| {
        let dispatching = with_dispatcher(nucleus, &waits);
        // The empty mask asks for nothing; a timer is not maskable.
        arm_timer_call(nucleus, &dispatching, 1)
            .ok()
            .expect("ArmTimer");
        dispatch_and_park(nucleus, &dispatching, dispatching.tail_key, u64::MAX);
        assert!(scheduler_of(&dispatching).parked().is_some());
        // The deadline has passed: the timer interrupt's work activates the
        // dispatcher at once and disarms the timer.
        nucleus.expire_deadlines();
        assert!(scheduler_of(&dispatching).parked().is_none());
        assert!(nucleus.preempt_requested);
        assert_eq!(nucleus.next_wakeup_ticks(), None);
        // SAFETY: exclusive serial fixture, no overlapping context/guard.
        let resumed = {
            let access = unsafe { Access::new() };
            nucleus
                .preempt_and_select(&access, PREEMPTED_SAVED)
                .unwrap_or_else(|e| panic!("tick: {:?}", e.code()))
                .expect("the dispatcher preempts")
                .expect_resume()
        };
        assert_eq!(resumed.next, waits.source);
        assert_eq!(
            resumed.completion.expect("completes").result0,
            activation_reason::TIMER
        );
    });
}

#[test_case]
fn a_timer_that_expires_while_the_dispatcher_runs_is_kept_for_its_next_dispatch() {
    with_resume(|nucleus, waits| {
        let dispatching = with_dispatcher(nucleus, &waits);
        arm_timer_call(nucleus, &dispatching, 1)
            .ok()
            .expect("ArmTimer");
        nucleus.expire_deadlines();
        assert_eq!(
            scheduler_of(&dispatching).pending(),
            activation_reason::TIMER
        );
        // The next Dispatch returns the reason at once and runs nothing.
        let Ok(InvokeOutcome::Complete((reasons, _events))) =
            dispatch_call(nucleus, &dispatching, Some(dispatching.tail_key), 1_000, 0)
        else {
            panic!("a pending activation returns at once");
        };
        assert_eq!(reasons, activation_reason::TIMER);
        assert_eq!(scheduler_of(&dispatching).pending(), 0);
    });
}

#[test_case]
fn suspend_stops_a_thread_being_announced_and_resume_announces_it() {
    with_resume(|nucleus, waits| {
        let dispatching = with_dispatcher(nucleus, &waits);
        scheduler_of(&dispatching)
            .set_activation_mask(activation_mask::THREAD_RUNNABLE)
            .ok()
            .expect("mask");
        nucleus.thread_mut(waits.tail).unwrap().context = ExecutionContext::Preempted {
            saved: target_saved(),
        };
        nucleus
            .suspend_thread(waits.tail)
            .unwrap_or_else(|e| panic!("Suspend: {:?}", e.code()));
        assert!(nucleus.thread(waits.tail).unwrap().suspended);

        // Not twice, and not the caller.
        assert!(nucleus.suspend_thread(waits.tail).is_err());
        assert!(nucleus.suspend_thread(waits.source).is_err());
        // An inert Thread has nothing to suspend.
        nucleus.thread_mut(waits.tail).unwrap().suspended = false;
        nucleus.thread_mut(waits.tail).unwrap().context = ExecutionContext::Inactive;
        assert!(nucleus.suspend_thread(waits.tail).is_err());
        nucleus.thread_mut(waits.tail).unwrap().context = ExecutionContext::Preempted {
            saved: target_saved(),
        };
        nucleus.suspend_thread(waits.tail).ok().expect("Suspend");

        // A wakeup is recorded in the Thread and announced to nobody.
        scheduler_of(&dispatching).take_pending();
        nucleus.make_runnable(waits.tail);
        assert_eq!(scheduler_of(&dispatching).pending(), 0);

        // Resume announces it, as any wakeup is, and only a suspended Thread
        // can be resumed.
        nucleus
            .resume_thread(waits.tail)
            .unwrap_or_else(|e| panic!("Resume: {:?}", e.code()));
        assert!(!nucleus.thread(waits.tail).unwrap().suspended);
        assert_eq!(
            scheduler_of(&dispatching).pending(),
            activation_reason::EVENT
        );
        assert!(nucleus.resume_thread(waits.tail).is_err());
    });
}

#[test_case]
fn a_suspended_waiter_keeps_its_wait_and_resume_cannot_skip_it() {
    with_resume(|nucleus, waits| {
        let dispatching = with_dispatcher(nucleus, &waits);
        scheduler_of(&dispatching)
            .set_activation_mask(activation_mask::THREAD_RUNNABLE)
            .ok()
            .expect("mask");
        // The target's wait has completed: suspended, it keeps the
        // completion until it is resumed, and resuming it announces it.
        nucleus.suspend_thread(waits.target).ok().expect("Suspend");
        assert!(matches!(
            nucleus.pending.state(waits.terminal),
            Ok(PendingState::Completed { .. })
        ));
        scheduler_of(&dispatching).take_pending();
        nucleus.resume_thread(waits.target).ok().expect("Resume");
        assert_eq!(
            scheduler_of(&dispatching).pending(),
            activation_reason::EVENT
        );

        // A waiter whose wait is still open goes back to blocked, and
        // nothing is announced.
        let blocked = nucleus
            .create_thread(nucleus.thread(waits.source).unwrap().address_space)
            .expect("Thread");
        nucleus.thread_mut(blocked).unwrap().scheduler = Some(dispatching.scheduler_id);
        let record = nucleus
            .pending
            .block(
                blocked,
                PendingKind::NotificationWait,
                crate::objects::carved::test_id(libobject::ObjectType::NOTIFICATION, 0x100),
            )
            .unwrap_or_else(|e| panic!("wait: {:?}", e.code()));
        nucleus.thread_mut(blocked).unwrap().context = ExecutionContext::Parked {
            saved: target_saved(),
            record,
        };
        nucleus.suspend_thread(blocked).ok().expect("Suspend");
        scheduler_of(&dispatching).take_pending();
        nucleus.resume_thread(blocked).ok().expect("Resume");
        assert!(!nucleus.thread(blocked).unwrap().suspended);
        assert_eq!(
            scheduler_of(&dispatching).pending(),
            0,
            "Resume skipped the wait"
        );
        assert_eq!(
            nucleus.pending.state(record).ok(),
            Some(PendingState::Waiting)
        );
    });
}

#[test_case]
fn a_suspended_thread_cannot_be_dispatched_and_the_dispatcher_cannot_be_suspended() {
    with_resume(|nucleus, waits| {
        let dispatching = with_dispatcher(nucleus, &waits);
        nucleus.suspend_thread(waits.tail).ok().expect("Suspend");
        assert!(matches!(
            dispatch_call(nucleus, &dispatching, Some(dispatching.tail_key), 1_000, 0),
            Err(CapError::InvalidOperation)
        ));
        nucleus.resume_thread(waits.tail).ok().expect("Resume");
        assert!(dispatch_call(nucleus, &dispatching, Some(dispatching.tail_key), 1_000, 0).is_ok());
        // The bound dispatcher is never suspended.
        nucleus.current_thread = Some(waits.target);
        assert!(nucleus.suspend_thread(waits.source).is_err());
    });
}

#[test_case]
fn a_thread_retiring_itself_switches_away_and_is_gone() {
    with_resume(|nucleus, waits| {
        // SAFETY: exclusive serial fixture, no overlapping context/guard.
        let resumed = {
            let access = unsafe { Access::new() };
            nucleus
                .exit_current_thread(&access)
                .unwrap_or_else(|e| panic!("exit: {:?}", e.code()))
                .expect_resume()
        };
        assert_eq!(resumed.next, waits.target);
        assert_eq!(nucleus.current_thread, Some(waits.target));
        assert!(nucleus.thread(waits.source).is_none(), "the Thread is gone");
        // The wait the Thread was named in left no record behind.
        assert!(nucleus.pending.state(waits.incoming).is_err());
    });
}

#[test_case]
fn the_last_thread_exiting_leaves_the_nucleus_idle() {
    with_resume(|nucleus, waits| {
        while nucleus.activations.pop().is_some() {}
        // SAFETY: exclusive serial fixture, no overlapping context/guard.
        let switch = {
            let access = unsafe { Access::new() };
            nucleus
                .exit_current_thread(&access)
                .unwrap_or_else(|e| panic!("exit: {:?}", e.code()))
        };
        assert_eq!(switch, Switch::Idle);
        assert!(nucleus.thread(waits.source).is_none(), "the Thread is gone");
        assert_eq!(nucleus.current_thread, None);
    });
}

#[test_case]
fn a_dispatched_thread_exiting_activates_its_dispatcher_with_exited() {
    with_resume(|nucleus, waits| {
        let dispatching = with_dispatcher(nucleus, &waits);
        dispatch_and_park(nucleus, &dispatching, dispatching.tail_key, u64::MAX);
        assert_eq!(nucleus.current_thread, Some(waits.tail));
        // SAFETY: exclusive serial fixture, no overlapping context/guard.
        let resumed = {
            let access = unsafe { Access::new() };
            nucleus
                .exit_current_thread(&access)
                .unwrap_or_else(|e| panic!("exit: {:?}", e.code()))
                .expect_resume()
        };
        assert_eq!(resumed.next, waits.source);
        assert_eq!(
            resumed.completion.expect("completes").result0,
            activation_reason::EXITED
        );
        assert!(nucleus.thread(waits.tail).is_none());
    });
}

// ═══════════════════════════════════════════════════════════════
// CHILD DOMAINS: GRANTS
// ═══════════════════════════════════════════════════════════════

/// A root Scheduler with its dispatcher (the fixture's `source` Thread), and
/// a child Scheduler whose dispatcher `dc` is a Thread of the root, with one
/// Thread `t` of its own.
struct Domain {
    root: Dispatching,
    child_id: CarvedId,
    child: RawKey,
    dc: ThreadId,
    dc_key: RawKey,
    t: ThreadId,
    t_key: RawKey,
}

fn with_domain(nucleus: &mut Nucleus<ArchObjectsImpl>, waits: &Waits) -> Domain {
    let root = with_dispatcher(nucleus, waits);
    let space = nucleus.thread(waits.source).unwrap().address_space;
    let child_id = test_arena::carve(Scheduler::inert()).expect("arena exhausted");
    let dc = nucleus.create_thread(space).expect("child dispatcher");
    let t = nucleus.create_thread(space).expect("child Thread");
    for thread in [dc, t] {
        nucleus.thread_mut(thread).unwrap().context = ExecutionContext::Preempted {
            saved: target_saved(),
        };
    }
    nucleus.thread_mut(dc).unwrap().scheduler = Some(root.scheduler_id);
    nucleus.thread_mut(dc).unwrap().dispatches = Some(child_id);
    nucleus.thread_mut(t).unwrap().scheduler = Some(child_id);
    {
        let child = nucleus.scheduler_mut(child_id).unwrap();
        child.set_parent(root.scheduler_id, 1);
        child.bind_dispatcher(dc).ok().expect("bind");
    }
    nucleus.join_scheduler(t, child_id);
    let mut install = |slot: u32, entry: KeyEntry| {
        nucleus
            .current_thread_table_mut()
            .unwrap()
            .insert(KeySlot(slot), entry, FIXTURE_GUARD)
            .unwrap_or_else(|failure| panic!("install: {:?}", failure.error.code()))
    };
    let child = install(60, KeyEntry::new_carved(child_id, Rights::all(), 0));
    let dc_key = install(61, KeyEntry::new_thread(dc, Rights::all(), 0));
    let t_key = install(62, KeyEntry::new_thread(t, Rights::all(), 0));
    Domain {
        root,
        child_id,
        child,
        dc,
        dc_key,
        t,
        t_key,
    }
}

fn child_object(domain: &Domain) -> &'static mut Scheduler {
    // SAFETY: a test-arena Scheduler; serial tests.
    unsafe {
        &mut *Carved::<Scheduler>::validate(domain.child_id)
            .ok()
            .expect("Scheduler carved")
    }
}

/// The root dispatcher dispatches the child dispatcher for `limit_ns`; it
/// runs.
fn run_child_dispatcher(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    domain: &Domain,
    limit_ns: u64,
) -> crate::objects::resume::PreparedResume {
    dispatch_and_park(nucleus, &domain.root, domain.dc_key, limit_ns)
}

/// The child dispatcher (current) dispatches `t` for `limit_ns`; it runs.
fn child_dispatches(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    domain: &Domain,
    limit_ns: u64,
) -> crate::objects::resume::PreparedResume {
    let Ok(InvokeOutcome::Blocked(record)) = cap_invoke(
        nucleus,
        domain.child,
        SchedulerOp::Dispatch as u64,
        &[domain.t_key.to_wire(), 0, limit_ns, 0, 0, 0],
    ) else {
        panic!("Dispatch should block the child dispatcher");
    };
    // SAFETY: exclusive serial fixture, no overlapping context/guard.
    let access = unsafe { Access::new() };
    nucleus
        .park_and_select(&access, SOURCE_SAVED, record)
        .unwrap_or_else(|e| panic!("park onto the child's Thread: {:?}", e.code()))
        .expect_resume()
}

#[test_case]
fn dispatching_a_child_dispatcher_starts_its_grant_and_the_childs_limits_are_clamped() {
    with_resume(|nucleus, waits| {
        let domain = with_domain(nucleus, &waits);
        assert!(child_object(&domain).grant().is_none());

        let resumed = run_child_dispatcher(nucleus, &domain, 50_000_000);
        assert_eq!(resumed.next, domain.dc);
        let grant = child_object(&domain).grant().expect("a grant is in force");
        let end = grant.deadline_ticks.expect("a limited grant");
        let now = nucleus.counter_ticks();
        assert!(end > now);
        // The root dispatcher waits; the grant is the root's limit on the
        // child dispatcher.
        assert!(scheduler_of(&domain.root).parked().is_some());
        assert_eq!(nucleus.dispatched.map(|d| d.thread), Some(domain.dc));

        // The child dispatches its Thread for far longer than the grant: the
        // Thread runs, the grant stays, and the limit is cut to its end.
        let resumed = child_dispatches(nucleus, &domain, 10_000_000_000);
        assert_eq!(resumed.next, domain.t);
        let dispatched = nucleus.dispatched.expect("the Thread is dispatched");
        assert_eq!(dispatched.thread, domain.t);
        assert_eq!(dispatched.scheduler, domain.child_id);
        assert_eq!(dispatched.deadline_ticks, Some(end));
        assert!(child_object(&domain).grant().is_some());
        // The root dispatcher still waits: the child's switch is not a stop
        // of the domain.
        assert!(scheduler_of(&domain.root).parked().is_some());
    });
}

#[test_case]
fn a_child_threads_stop_activates_the_child_dispatcher_inside_the_grant_not_the_parent() {
    with_resume(|nucleus, waits| {
        let domain = with_domain(nucleus, &waits);
        run_child_dispatcher(nucleus, &domain, u64::MAX);
        child_dispatches(nucleus, &domain, u64::MAX);
        let grant = child_object(&domain).grant().expect("a grant");

        // The Thread faults: that is the child's event.
        // SAFETY: exclusive serial fixture, no overlapping context/guard.
        let resumed = {
            let access = unsafe { Access::new() };
            nucleus
                .park_faulted_and_select(&access, PREEMPTED_SAVED)
                .unwrap_or_else(|e| panic!("fault: {:?}", e.code()))
                .expect_resume()
        };
        assert_eq!(resumed.next, domain.dc);
        assert_eq!(
            resumed.completion.expect("completes").result0,
            activation_reason::FAULTED
        );
        // The root dispatcher was not activated, and the parent's limit
        // goes on applying to the child dispatcher.
        assert!(scheduler_of(&domain.root).parked().is_some());
        assert!(child_object(&domain).grant().is_some());
        let dispatched = nucleus
            .dispatched
            .expect("the child dispatcher is dispatched");
        assert_eq!(dispatched.thread, domain.dc);
        assert_eq!(dispatched.scheduler, domain.root.scheduler_id);
        assert_eq!(dispatched.deadline_ticks, grant.deadline_ticks);
    });
}

#[test_case]
fn the_end_of_the_grant_stops_the_childs_thread_and_activates_the_parent_with_limit() {
    with_resume(|nucleus, waits| {
        let domain = with_domain(nucleus, &waits);
        run_child_dispatcher(nucleus, &domain, 50_000_000);
        child_dispatches(nucleus, &domain, 10_000_000_000);
        // The grant ends: its deadline is now behind us.
        child_object(&domain).set_grant(Some(crate::objects::scheduler::Grant {
            deadline_ticks: Some(0),
        }));
        nucleus.dispatched.as_mut().unwrap().deadline_ticks = Some(0);

        // SAFETY: exclusive serial fixture, no overlapping context/guard.
        let resumed = {
            let access = unsafe { Access::new() };
            nucleus
                .preempt_and_select(&access, PREEMPTED_SAVED)
                .unwrap_or_else(|e| panic!("tick: {:?}", e.code()))
                .expect("the parent preempts")
                .expect_resume()
        };
        // The root dispatcher runs, told that its dispatch hit its limit.
        assert_eq!(resumed.next, waits.source);
        assert_eq!(
            resumed.completion.expect("completes").result0,
            activation_reason::LIMIT
        );
        assert!(child_object(&domain).grant().is_none());
        // The child's Thread is stopped and runnable; the child dispatcher
        // is a woken waiter of the root: its Dispatch completed with LIMIT.
        assert!(matches!(
            nucleus.thread(domain.t).unwrap().context,
            ExecutionContext::Preempted { .. }
        ));
        let Some(ExecutionContext::Parked { record, .. }) =
            nucleus.thread(domain.dc).map(|dc| dc.context)
        else {
            panic!("the child dispatcher stays parked");
        };
        assert!(matches!(
            nucleus.pending.state(record),
            Ok(PendingState::Completed { result0, .. }) if result0 == activation_reason::LIMIT
        ));

        // The root dispatcher can dispatch it again, with a new grant: the
        // parked call returns LIMIT to the child dispatcher.
        let resumed = dispatch_and_park(nucleus, &domain.root, domain.dc_key, u64::MAX);
        assert_eq!(resumed.next, domain.dc);
        assert_eq!(
            resumed.completion.expect("completes").result0,
            activation_reason::LIMIT
        );
        assert!(child_object(&domain).grant().is_some());
    });
}

#[test_case]
fn a_child_activation_without_a_grant_is_a_wakeup_the_parents_mask_decides_on() {
    with_resume(|nucleus, waits| {
        let domain = with_domain(nucleus, &waits);
        // The child dispatcher waits idle, with no grant in force.
        nucleus.thread_mut(domain.dc).unwrap().context = ExecutionContext::Parked {
            saved: target_saved(),
            record: nucleus
                .pending
                .block(domain.dc, PendingKind::SchedulerDispatch, domain.child_id)
                .unwrap_or_else(|e| panic!("park: {:?}", e.code())),
        };
        let Ok(PendingState::Waiting) = (match nucleus.thread(domain.dc).unwrap().context {
            ExecutionContext::Parked { record, .. } => {
                child_object(&domain).park_dispatcher(record, true);
                nucleus.pending.state(record)
            }
            _ => unreachable!(),
        }) else {
            panic!("the call waits");
        };
        // The root asks for nothing.
        nucleus.request_activation(domain.child_id, activation_reason::EVENT, 0);
        // The call completed and the child dispatcher is a runnable Thread
        // of the root; the root dispatcher is not activated and nothing
        // preempts.
        assert!(child_object(&domain).parked().is_none());
        assert_eq!(scheduler_of(&domain.root).pending(), 0);
        assert!(!nucleus.preempt_requested);
        assert!(!nucleus.thread(domain.dc).unwrap().queued);

        // The root asks to hear about runnable Threads: the next such
        // wakeup of the child dispatcher is an event for the root, which
        // still does not preempt by itself.
        scheduler_of(&domain.root)
            .set_activation_mask(libobject::scheduler::activation_mask::THREAD_RUNNABLE)
            .ok()
            .expect("mask");
        let record = nucleus
            .pending
            .block(domain.dc, PendingKind::SchedulerDispatch, domain.child_id)
            .unwrap_or_else(|e| panic!("park: {:?}", e.code()));
        nucleus.thread_mut(domain.dc).unwrap().context = ExecutionContext::Parked {
            saved: target_saved(),
            record,
        };
        child_object(&domain).park_dispatcher(record, true);
        nucleus.request_activation(domain.child_id, activation_reason::EVENT, 0);
        assert_eq!(
            scheduler_of(&domain.root).pending(),
            activation_reason::EVENT
        );
    });
}

#[test_case]
fn a_child_dispatcher_that_faults_suspends_its_domain_and_tells_the_parent() {
    with_resume(|nucleus, waits| {
        let domain = with_domain(nucleus, &waits);
        run_child_dispatcher(nucleus, &domain, u64::MAX);
        assert_eq!(nucleus.current_thread, Some(domain.dc));

        // The child dispatcher faults.
        // SAFETY: exclusive serial fixture, no overlapping context/guard.
        let resumed = {
            let access = unsafe { Access::new() };
            nucleus
                .park_faulted_and_select(&access, PREEMPTED_SAVED)
                .unwrap_or_else(|e| panic!("fault: {:?}", e.code()))
                .expect_resume()
        };
        // The root dispatcher runs, told of the fault and of the domain.
        assert_eq!(resumed.next, waits.source);
        assert_eq!(
            resumed.completion.expect("completes").result0,
            activation_reason::FAULTED | activation_reason::SCHED_DOMAIN_FAILED
        );
        // The child has no dispatcher and no grant; its Thread is suspended
        // with its state kept.
        assert!(child_object(&domain).dispatcher().is_none());
        assert!(child_object(&domain).grant().is_none());
        assert!(nucleus.thread(domain.t).unwrap().suspended);
        assert!(matches!(
            nucleus.thread(domain.t).unwrap().context,
            ExecutionContext::Preempted { .. }
        ));
    });
}

#[test_case]
fn retiring_a_child_dispatcher_suspends_its_domain_and_a_new_dispatcher_can_be_bound() {
    with_resume(|nucleus, waits| {
        let domain = with_domain(nucleus, &waits);
        cap_invoke(
            nucleus,
            domain.dc_key,
            libobject::thread::ThreadOp::Retire as u64,
            &[0; 6],
        )
        .ok()
        .expect("Retire");
        assert!(child_object(&domain).dispatcher().is_none());
        assert!(nucleus.thread(domain.t).unwrap().suspended);
        // The root dispatcher is running, so the failure waits for its next
        // Dispatch.
        assert_eq!(
            scheduler_of(&domain.root).pending(),
            activation_reason::SCHED_DOMAIN_FAILED
        );

        // The parent recovers: a new dispatcher, then the Thread resumes.
        let space = nucleus.thread(waits.source).unwrap().address_space;
        let replacement = nucleus.create_thread(space).expect("replacement");
        {
            let thread = nucleus.thread_mut(replacement).unwrap();
            thread.scheduler = Some(domain.root.scheduler_id);
            thread.context = ExecutionContext::Configured {
                saved: target_saved(),
            };
        }
        nucleus
            .bind_dispatcher(domain.child_id, replacement)
            .unwrap_or_else(|e| panic!("rebind: {:?}", e.code()));
        assert_eq!(child_object(&domain).dispatcher(), Some(replacement));
        nucleus.resume_thread(domain.t).ok().expect("Resume");
        assert!(!nucleus.thread(domain.t).unwrap().suspended);
    });
}

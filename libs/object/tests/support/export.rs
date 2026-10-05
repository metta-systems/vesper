use {
    super::{PpcResult, complete_with, init_return_key, return_key},
    std::{cell::Cell, panic},
    vesper_objects::{CapError, InconsistencyReason, KeySlot, RawKey, thread::ThreadReturnKey},
};

/// Arguments the injected fault step received.
type Handoff = (u64, u64, u64, u64, u64);

/// Run the adapter sequence; the fault step panics with its arguments so the
/// non-returning handoff can be observed.
fn run_adapter(result: PpcResult, rejection: CapError) -> (Handoff, Option<(RawKey, u64, u64)>) {
    let attempted = Cell::new(None);
    let key = ThreadReturnKey::provisioned(0x00AB_CDEF, 8);
    let outcome = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        // SAFETY: both injected steps are test doubles; nothing is abandoned.
        unsafe {
            complete_with(
                result,
                &key,
                |key, r0, r1| {
                    attempted.set(Some((key.raw(), r0, r1)));
                    Err(rejection)
                },
                |status, detail1, detail2, r0, r1| {
                    panic::panic_any::<Handoff>((status, detail1, detail2, r0, r1))
                },
            )
        }
    }));
    let payload = outcome.expect_err("adapter returned normally");
    let handoff = *payload
        .downcast::<Handoff>()
        .expect("fault step did not receive the handoff");
    (handoff, attempted.get())
}

#[test]
fn provisioned_key_is_guard_size_slot_one_incarnation_one() {
    let key = ThreadReturnKey::provisioned(0x00AB_CDEF, 8).raw();
    assert_eq!(key, RawKey::from_parts(0x00AB_CDEF, 8, 1, 1));
    assert_eq!(key.incarnation(), KeySlot::THREAD_RETURN_INCARNATION);
    assert_eq!(key.slot().0 & 0xff, KeySlot::THREAD_RETURN.0);
}

#[test]
fn init_records_the_builder_supplied_key() {
    let key = ThreadReturnKey::provisioned(0x0012_3456, 10);
    init_return_key(&key);
    assert_eq!(return_key().raw(), key.raw());
}

#[test]
fn adapter_returns_ordered_words_through_the_given_key_once() {
    let result = PpcResult {
        r0: 0x1111_2222_3333_4444,
        r1: u64::MAX,
    };
    let (_, attempted) = run_adapter(result, CapError::InvalidOperation);
    assert_eq!(
        attempted,
        Some((
            ThreadReturnKey::provisioned(0x00AB_CDEF, 8).raw(),
            0x1111_2222_3333_4444,
            u64::MAX
        ))
    );
}

#[test]
fn rejected_return_hands_exact_diagnostics_and_original_words_to_the_handler() {
    let stale = RawKey::new(KeySlot(0xC0FF_EE01), 2);
    let rejection = CapError::InconsistentKey {
        key: stale,
        reason: InconsistencyReason::SlotIncarnationMismatch,
        operand: 0,
    };
    let expected = (27, stale.to_wire(), 1);
    let result = PpcResult {
        r0: 0xAAAA,
        r1: 0xBBBB,
    };
    let (handoff, _) = run_adapter(result, rejection);
    assert_eq!(
        handoff,
        (expected.0, expected.1, expected.2, 0xAAAA, 0xBBBB)
    );
}

#[test]
fn unexpected_local_success_reaches_the_handler_with_original_words() {
    let result = PpcResult { r0: 7, r1: 8 };
    let (handoff, _) = run_adapter(
        result,
        CapError::UnexpectedReturn {
            word1: 0xDEAD,
            word2: 0xBEEF,
        },
    );
    assert_eq!(handoff, (34, 0xDEAD, 0xBEEF, 7, 8));
}

#[test]
fn unknown_response_diagnostics_reach_the_handler_verbatim() {
    let status = core::num::NonZeroU64::new(0x99).unwrap();
    let result = PpcResult {
        r0: u64::MAX,
        r1: 0,
    };
    let (handoff, _) = run_adapter(
        result,
        CapError::UnknownResponse {
            status,
            detail1: 0x1234_5678_9abc_def0,
            detail2: u64::MAX,
        },
    );
    assert_eq!(
        handoff,
        (0x99, 0x1234_5678_9abc_def0, u64::MAX, u64::MAX, 0)
    );
}

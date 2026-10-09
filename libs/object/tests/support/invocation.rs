use {
    super::{INVOCATION_STACK_DEPTH, InvocationKey},
    std::cell::Cell,
    vesper_objects::{CapError, InvalidStackReason, KeySlot, RawKey},
};

type Response = (u64, u64, u64);
type Request = (u64, [u64; 6], u64);

std::thread_local! {
    static RESPONSE: Cell<Option<Response>> = const { Cell::new(None) };
    static REQUEST: Cell<Option<Request>> = const { Cell::new(None) };
}

/// Recording replacement for `libsyscall::ppc_call`.
pub(super) unsafe fn ppc_call(key: u64, args: [u64; 6], target_sp: u64) -> Response {
    REQUEST.with(|recorded| assert!(recorded.replace(Some((key, args, target_sp))).is_none()));
    RESPONSE.with(|response| response.take().expect("unexpected syscall"))
}

fn call(response: Response) -> (Result<(u64, u64), CapError>, Request) {
    let key = RawKey::new(KeySlot(0xFFFF_FF0A), 0x89ab_cdef);
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
    let args = [0x2222, u64::MAX, 0, 0x5555_0000_0000_5555, 1 << 63, 0x7777];
    // SAFETY: the recording transport performs no SVC and runs no target.
    let result = unsafe { InvocationKey::from_key(key).call(args, 0x1800_1000) };
    let request = REQUEST.with(|recorded| recorded.take().expect("no syscall recorded"));
    (result, request)
}

#[test]
fn call_submits_full_key_six_inputs_and_target_sp_unchanged() {
    let (result, (key, args, target_sp)) = call((0, 0xAAAA, 0xBBBB));
    assert_eq!(
        key,
        RawKey::new(KeySlot(0xFFFF_FF0A), 0x89ab_cdef).to_wire()
    );
    assert_eq!(
        args,
        [0x2222, u64::MAX, 0, 0x5555_0000_0000_5555, 1 << 63, 0x7777]
    );
    assert_eq!(target_sp, 0x1800_1000);
    assert!(matches!(result, Ok((0xAAAA, 0xBBBB))));
}

#[test]
fn call_success_keeps_both_full_width_result_words() {
    for words in [(0, 0), (u64::MAX, 1), (0x1234_5678_9abc_def0, u64::MAX)] {
        let (result, _) = call((0, words.0, words.1));
        assert!(matches!(result, Ok(actual) if actual == words));
    }
}

#[test]
fn call_rejections_decode_through_the_shared_decoder() {
    let (result, _) = call((32, 0x1800_0ff8, 10));
    assert!(matches!(
        result,
        Err(CapError::InvalidStack {
            value: 0x1800_0ff8,
            reason: InvalidStackReason::SpMisaligned
        })
    ));
    let (result, _) = call((33, INVOCATION_STACK_DEPTH as u64, 0));
    assert!(matches!(result, Err(CapError::NestingDepth { count: 16 })));
    let (result, _) = call((8, 0, 0));
    assert!(matches!(result, Err(CapError::InvalidOperation)));
    let (result, _) = call((0x99, 1, 2));
    assert!(matches!(result, Err(CapError::UnknownResponse { .. })));
}

use {
    super::{INVOCATION_STACK_DEPTH, InvocationKey},
    std::cell::Cell,
    vesper_objects::{CapError, KeySlot, RawKey},
};

/// A Call response: `x0` and `x1..x6`.
type Response = (u64, [u64; 6]);
type Request = (u64, [u64; 6]);

/// The six inputs every test call passes.
const ARGS: [u64; 6] = [0x2222, u64::MAX, 0, 0x5555_0000_0000_5555, 1 << 63, 0x7777];

std::thread_local! {
    static RESPONSE: Cell<Option<Response>> = const { Cell::new(None) };
    static REQUEST: Cell<Option<Request>> = const { Cell::new(None) };
}

/// A rejection as the kernel leaves it: `x0..x2` the error, `x3..x6` the
/// caller's own inputs `args[1..5]`, untouched.
const fn rejected(status: u64, detail1: u64, detail2: u64) -> Response {
    (
        status,
        [detail1, detail2, ARGS[1], ARGS[2], ARGS[3], ARGS[4]],
    )
}

/// Recording replacement for `libsyscall::ppc_call`.
pub(super) fn ppc_call(key: u64, args: [u64; 6]) -> Response {
    REQUEST.with(|recorded| assert!(recorded.replace(Some((key, args))).is_none()));
    RESPONSE.with(|response| response.take().expect("unexpected syscall"))
}

fn call(response: Response) -> (Result<[u64; 6], CapError>, Request) {
    let key = RawKey::new(KeySlot(0xFFFF_FF0A), 0x89ab_cdef);
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
    let result = InvocationKey::from_key(key).call(ARGS);
    let request = REQUEST.with(|recorded| recorded.take().expect("no syscall recorded"));
    (result, request)
}

#[test]
fn call_submits_full_key_and_six_inputs_unchanged() {
    let (result, (key, args)) = call((0, [0xAAAA, 0xBBBB, 0, 0, 0, 0]));
    assert_eq!(
        key,
        RawKey::new(KeySlot(0xFFFF_FF0A), 0x89ab_cdef).to_wire()
    );
    assert_eq!(args, ARGS);
    assert!(matches!(result, Ok([0xAAAA, 0xBBBB, 0, 0, 0, 0])));
}

#[test]
fn call_success_keeps_all_six_full_width_result_words() {
    for words in [
        [0; 6],
        [u64::MAX, 1, 2, 3, 4, 5],
        [0x1234_5678_9abc_def0, u64::MAX, 1 << 63, 7, u64::MAX, 0x66],
    ] {
        let (result, _) = call((0, words));
        assert!(matches!(result, Ok(actual) if actual == words));
    }
}

#[test]
fn call_rejections_decode_through_the_shared_decoder() {
    let (result, _) = call(rejected(47, 16, 0));
    assert!(matches!(result, Err(CapError::NoFreeStack { count: 16 })));
    let (result, _) = call(rejected(34, INVOCATION_STACK_DEPTH as u64, 0));
    assert!(matches!(result, Err(CapError::NestingDepth { count: 16 })));
    let (result, _) = call(rejected(9, 0, 0));
    assert!(matches!(result, Err(CapError::InvalidOperation)));
    let (result, _) = call(rejected(0x99, 1, 2));
    assert!(matches!(result, Err(CapError::UnknownResponse { .. })));
}

type KeyedRequest = (u64, u64, [u64; 6]);

std::thread_local! {
    static KEYED_REQUEST: Cell<Option<KeyedRequest>> = const { Cell::new(None) };
}

/// Recording replacement for `libsyscall::ppc_call_with_key`.
pub(super) fn ppc_call_with_key(key: u64, lent_key: u64, args: [u64; 6]) -> Response {
    KEYED_REQUEST.with(|recorded| {
        assert!(recorded.replace(Some((key, lent_key, args))).is_none());
    });
    RESPONSE.with(|response| response.take().expect("unexpected syscall"))
}

fn call_with_key(response: Response) -> (Result<[u64; 6], CapError>, KeyedRequest) {
    let key = RawKey::new(KeySlot(0xFFFF_FF0A), 0x89ab_cdef);
    let lent = RawKey::new(KeySlot(0x0000_0107), 0x0246_8ace);
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
    let result = InvocationKey::from_key(key).call_with_key(lent, ARGS);
    let request = KEYED_REQUEST.with(|recorded| recorded.take().expect("no syscall recorded"));
    (result, request)
}

#[test]
fn call_with_key_submits_both_keys_and_six_inputs_unchanged() {
    let (result, (key, lent, args)) = call_with_key((0, [0xAAAA, 0xBBBB, 3, 4, 5, 6]));
    assert_eq!(
        key,
        RawKey::new(KeySlot(0xFFFF_FF0A), 0x89ab_cdef).to_wire()
    );
    assert_eq!(
        lent,
        RawKey::new(KeySlot(0x0000_0107), 0x0246_8ace).to_wire()
    );
    assert_eq!(args, ARGS);
    assert!(matches!(result, Ok([0xAAAA, 0xBBBB, 3, 4, 5, 6])));
}

#[test]
fn call_with_key_rejections_decode_through_the_shared_decoder() {
    let (result, _) = call_with_key(rejected(38, 0, 0));
    assert!(matches!(result, Err(CapError::KeyNotAccepted)));
    let (result, _) = call_with_key(rejected(39, 0, 0));
    assert!(matches!(result, Err(CapError::NoPlacementSlot)));
    let (result, _) = call_with_key(rejected(6, 0, 0));
    assert!(matches!(result, Err(CapError::InsufficientRights)));
    let (result, _) = call_with_key(rejected(34, INVOCATION_STACK_DEPTH as u64, 0));
    assert!(matches!(result, Err(CapError::NestingDepth { count: 16 })));
}

#[test]
fn invocation_operation_ids_are_stable() {
    use super::InvocationOp;

    assert_eq!(InvocationOp::Call as u8, 0);
    assert_eq!(InvocationOp::CallWithKey as u8, 1);
    assert!(matches!(
        InvocationOp::try_from(1_u64),
        Ok(InvocationOp::CallWithKey)
    ));
    assert!(matches!(
        InvocationOp::try_from(2_u64),
        Err(CapError::InvalidOperation)
    ));
}

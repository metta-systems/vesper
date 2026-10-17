use {
    super::{TimeKey, TimeQuery},
    std::cell::Cell,
    vesper_objects::{CapError, KeySlot, RawKey, time::TimeOp},
};

type Response = (u64, u64, u64);
type Request = (u64, u64, [u64; 6], usize);

std::thread_local! {
    static RESPONSE: Cell<Option<Response>> = const { Cell::new(None) };
    static REQUEST: Cell<Option<Request>> = const { Cell::new(None) };
}

fn respond(request: Request) -> Response {
    REQUEST.with(|recorded| assert!(recorded.replace(Some(request)).is_none()));
    RESPONSE.with(|response| response.take().expect("unexpected syscall"))
}

// Mirrors `libsyscall::protected_call0`.
pub(super) unsafe fn protected_call0(key: u64, op: u64) -> Response {
    respond((key, op, [0, 0, 0, 0, 0, 0], 0))
}

// Mirrors `libsyscall::protected_call1`.
pub(super) unsafe fn protected_call1(key: u64, op: u64, a0: u64) -> Response {
    respond((key, op, [a0, 0, 0, 0, 0, 0], 1))
}

// Mirrors `libsyscall::protected_call2`.
pub(super) unsafe fn protected_call2(key: u64, op: u64, a0: u64, a1: u64) -> Response {
    respond((key, op, [a0, a1, 0, 0, 0, 0], 2))
}

fn source_key() -> TimeKey {
    TimeKey::from_key(RawKey::new(KeySlot(17), 1))
}

fn other_key() -> TimeKey {
    TimeKey::from_key(RawKey::new(KeySlot(9), 3))
}

fn take_request() -> Request {
    REQUEST.with(|request| request.take().expect("missing syscall request"))
}

fn arm(response: Response) {
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
}

#[test]
fn time_op_ids_match_the_wire_baseline() {
    assert_eq!(TimeOp::Transfer as u64, 0);
    assert_eq!(TimeOp::Merge as u64, 1);
    assert_eq!(TimeOp::Query as u64, 2);
    for (id, op) in [
        (0, TimeOp::Transfer),
        (1, TimeOp::Merge),
        (2, TimeOp::Query),
    ] {
        assert_eq!(TimeOp::try_from(id).map_err(CapError::code), Ok(op));
    }
    for id in [3, 255, u64::MAX] {
        assert_eq!(
            TimeOp::try_from(id).map_err(CapError::code),
            Err(CapError::InvalidOperation.code())
        );
    }
}

#[test]
fn transfer_encodes_destination_and_amount() {
    arm((0, 0, 0));
    source_key()
        .transfer(&other_key(), 1_000_000)
        .map_err(CapError::code)
        .unwrap();
    let (key, op, args, len) = take_request();
    assert_eq!(len, 2);
    assert_eq!(key, RawKey::new(KeySlot(17), 1).to_wire());
    assert_eq!(op, TimeOp::Transfer as u64);
    assert_eq!(
        args,
        [RawKey::new(KeySlot(9), 3).to_wire(), 1_000_000, 0, 0, 0, 0]
    );
}

#[test]
fn transfer_decodes_insufficient_budget_with_the_remaining_amount() {
    arm((36, 42, 0));
    let error = source_key().transfer(&other_key(), 100).unwrap_err();
    take_request();
    assert!(matches!(
        error,
        CapError::InsufficientBudget { remaining: 42 }
    ));
}

#[test]
fn merge_encodes_the_source_key() {
    arm((0, 0, 0));
    other_key()
        .merge(&source_key())
        .map_err(CapError::code)
        .unwrap();
    let (key, op, args, len) = take_request();
    assert_eq!(len, 1);
    assert_eq!(key, RawKey::new(KeySlot(9), 3).to_wire());
    assert_eq!(op, TimeOp::Merge as u64);
    assert_eq!(args, [RawKey::new(KeySlot(17), 1).to_wire(), 0, 0, 0, 0, 0]);
}

#[test]
fn query_returns_the_budget() {
    arm((0, u64::MAX, 0));
    let result = source_key().query().map_err(CapError::code).unwrap();
    let (_, op, args, len) = take_request();
    assert_eq!((op, args, len), (TimeOp::Query as u64, [0; 6], 0));
    assert_eq!(
        result,
        TimeQuery {
            budget_ns: u64::MAX
        }
    );
}

#[test]
fn query_rejects_a_nonzero_reserved_result_word() {
    arm((0, 5, 1));
    let error = source_key().query().unwrap_err();
    take_request();
    assert!(matches!(error, CapError::Unknown));
}

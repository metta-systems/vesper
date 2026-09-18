use {
    super::EventCountKey,
    std::cell::Cell,
    vesper_objects::{CapError, KeySlot, RawKey, event_count::EventCountOp},
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

fn event_count_key() -> EventCountKey {
    EventCountKey::from_key(RawKey::new(KeySlot(7), 1))
}

fn take_request() -> Request {
    REQUEST.with(|request| request.take().expect("missing syscall request"))
}

/// Issue `advance` and return the captured request plus the new value, or
/// the decoded kernel error.
pub(super) fn invoke_advance(response: Response) -> Result<(Request, u64), CapError> {
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
    let result = event_count_key().advance(0b101);
    let request = take_request();
    result.map(|value| (request, value))
}

/// Issue `await_ge` and return the captured request plus the observed
/// value, or the decoded kernel error.
pub(super) fn invoke_await(response: Response) -> Result<(Request, u64), CapError> {
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
    let result = event_count_key().await_ge(0x10, EventCountKey::WAIT_INFINITE);
    let request = take_request();
    result.map(|value| (request, value))
}

/// Issue `read` and return the captured request plus the observed value,
/// or the decoded kernel error.
pub(super) fn invoke_read(response: Response) -> Result<(Request, u64), CapError> {
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
    let result = event_count_key().read();
    let request = take_request();
    result.map(|value| (request, value))
}

#[test]
fn event_count_advance_encodes_the_delta_argument() {
    let ((key, op, args, len), value) = invoke_advance((0, 0b101, 0))
        .map_err(CapError::code)
        .unwrap();
    assert_eq!(len, 1);
    assert_eq!(key, RawKey::new(KeySlot(7), 1).to_wire());
    assert_eq!(op, EventCountOp::Advance as u64);
    assert_eq!(args, [0b101, 0, 0, 0, 0, 0]);
    assert_eq!(value, 0b101);
}

#[test]
fn event_count_await_encodes_target_and_timeout_arguments() {
    let ((key, op, args, len), value) = invoke_await((0, 0x10, 0)).map_err(CapError::code).unwrap();
    assert_eq!(len, 2);
    assert_eq!(key, RawKey::new(KeySlot(7), 1).to_wire());
    assert_eq!(op, EventCountOp::Await as u64);
    assert_eq!(args, [0x10, EventCountKey::WAIT_INFINITE, 0, 0, 0, 0]);
    assert_eq!(value, 0x10);
}

#[test]
fn event_count_read_sends_no_arguments_and_decodes_value() {
    let ((key, op, args, len), value) = invoke_read((0, 0x1, 0)).map_err(CapError::code).unwrap();
    assert_eq!(len, 0);
    assert_eq!(key, RawKey::new(KeySlot(7), 1).to_wire());
    assert_eq!(op, EventCountOp::Read as u64);
    assert_eq!(args, [0, 0, 0, 0, 0, 0]);
    assert_eq!(value, 0x1);
}

#[test]
fn event_count_wrappers_preserve_kernel_errors() {
    assert!(matches!(
        invoke_advance((5, 0, 0)),
        Err(CapError::InsufficientRights)
    ));
    assert!(matches!(
        invoke_await((8, 0, 0)),
        Err(CapError::InvalidOperation)
    ));
    assert!(matches!(
        invoke_read((31, 0, 0)),
        Err(CapError::CounterOverflow)
    ));
    assert!(matches!(
        invoke_advance((u64::MAX, 42, 99)),
        Err(CapError::UnknownResponse { .. })
    ));
}

#[test]
fn event_count_wait_infinite_is_the_selected_encoding() {
    // The timeout model (selected 2026-09-16): u64::MAX means infinite;
    // zero and finite values are invalid/unsupported on the wire.
    assert_eq!(EventCountKey::WAIT_INFINITE, u64::MAX);
}

use {
    super::NotificationKey,
    std::cell::Cell,
    vesper_objects::{CapError, KeySlot, RawKey, notification::NotificationOp},
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

fn notification_key() -> NotificationKey {
    NotificationKey::from_key(RawKey::new(KeySlot(7), 1))
}

fn take_request() -> Request {
    REQUEST.with(|request| request.take().expect("missing syscall request"))
}

/// Issue `signal` and return the captured request plus success, or the
/// decoded kernel error.
pub(super) fn invoke_signal(response: Response) -> Result<Request, CapError> {
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
    let result = notification_key().signal(0b101);
    let request = take_request();
    result.map(|()| request)
}

/// Issue `wait` and return the captured request plus the consumed bitmap,
/// or the decoded kernel error.
pub(super) fn invoke_wait(response: Response) -> Result<(Request, u64), CapError> {
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
    let result = notification_key().wait(NotificationKey::WAIT_INFINITE);
    let request = take_request();
    result.map(|bits| (request, bits))
}

/// Issue `poll` and return the captured request plus the consumed bitmap,
/// or the decoded kernel error.
pub(super) fn invoke_poll(response: Response) -> Result<(Request, u64), CapError> {
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
    let result = notification_key().poll();
    let request = take_request();
    result.map(|bits| (request, bits))
}

#[test]
fn notification_signal_preserves_request_encoding_and_decodes_success() {
    let (key, op, args, len) = invoke_signal((0, 0, 0)).map_err(CapError::code).unwrap();
    assert_eq!(len, 1);
    assert_eq!(key, RawKey::new(KeySlot(7), 1).to_wire());
    assert_eq!(op, NotificationOp::Signal as u64);
    assert_eq!(args, [0b101, 0, 0, 0, 0, 0]);
}

#[test]
fn notification_wait_encodes_the_timeout_argument() {
    let ((key, op, args, len), bits) = invoke_wait((0, 0b11, 0)).map_err(CapError::code).unwrap();
    assert_eq!(len, 1);
    assert_eq!(key, RawKey::new(KeySlot(7), 1).to_wire());
    assert_eq!(op, NotificationOp::Wait as u64);
    assert_eq!(args, [NotificationKey::WAIT_INFINITE, 0, 0, 0, 0, 0]);
    assert_eq!(bits, 0b11);
}

#[test]
fn notification_poll_sends_no_arguments_and_decodes_bits() {
    let ((key, op, args, len), bits) = invoke_poll((0, 0b1, 0)).map_err(CapError::code).unwrap();
    assert_eq!(len, 0);
    assert_eq!(key, RawKey::new(KeySlot(7), 1).to_wire());
    assert_eq!(op, NotificationOp::Poll as u64);
    assert_eq!(args, [0, 0, 0, 0, 0, 0]);
    assert_eq!(bits, 0b1);
}

#[test]
fn notification_wrappers_preserve_kernel_errors() {
    assert!(matches!(
        invoke_signal((5, 0, 0)),
        Err(CapError::InsufficientRights)
    ));
    assert!(matches!(
        invoke_wait((8, 0, 0)),
        Err(CapError::InvalidOperation)
    ));
    assert!(matches!(
        invoke_poll((23, 0, 0)),
        Err(CapError::PoolExhausted)
    ));
    assert!(matches!(
        invoke_signal((u64::MAX, 42, 99)),
        Err(CapError::UnknownResponse { .. })
    ));
}

#[test]
fn notification_wait_infinite_is_the_selected_encoding() {
    // The timeout model (selected 2026-09-16): u64::MAX means infinite;
    // zero and finite values are invalid/unsupported on the wire.
    assert_eq!(NotificationKey::WAIT_INFINITE, u64::MAX);
}

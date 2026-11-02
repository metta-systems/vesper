use {
    super::WaitSetKey,
    std::cell::Cell,
    vesper_objects::{CapError, KeySlot, RawKey, wait_set::WaitSetOp},
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

// Mirror `libsyscall::protected_call1` to `protected_call3`.
pub(super) unsafe fn protected_call1(key: u64, op: u64, a0: u64) -> Response {
    respond((key, op, [a0, 0, 0, 0, 0, 0], 1))
}

pub(super) unsafe fn protected_call2(key: u64, op: u64, a0: u64, a1: u64) -> Response {
    respond((key, op, [a0, a1, 0, 0, 0, 0], 2))
}

pub(super) unsafe fn protected_call3(key: u64, op: u64, a0: u64, a1: u64, a2: u64) -> Response {
    respond((key, op, [a0, a1, a2, 0, 0, 0], 3))
}

fn set_key() -> WaitSetKey {
    WaitSetKey::from_key(RawKey::new(KeySlot(9), 1))
}

fn invoke(
    response: Response,
    call: impl FnOnce(&WaitSetKey) -> Result<u64, CapError>,
) -> (Request, Result<u64, CapError>) {
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
    let result = call(&set_key());
    let request = REQUEST.with(|request| request.take().expect("missing syscall request"));
    (request, result)
}

#[test]
fn add_member_encodes_key_slot_and_condition() {
    let member = RawKey::new(KeySlot(12), 3);
    let ((key, op, args, len), result) = invoke((0, 0, 0), |set| {
        set.add_member(member, 5, 0b110).map(|()| 0)
    });
    result.unwrap();
    assert_eq!(len, 3);
    assert_eq!(key, RawKey::new(KeySlot(9), 1).to_wire());
    assert_eq!(op, WaitSetOp::AddMember as u64);
    assert_eq!(args, [member.to_wire(), 5, 0b110, 0, 0, 0]);
}

#[test]
fn update_and_remove_encode_the_slot() {
    let ((_, op, args, len), result) =
        invoke((0, 0, 0), |set| set.update_member(2, 77).map(|()| 0));
    result.unwrap();
    assert_eq!((len, op), (2, WaitSetOp::UpdateMember as u64));
    assert_eq!(args, [2, 77, 0, 0, 0, 0]);

    let ((_, op, args, len), result) = invoke((0, 0, 0), |set| set.remove_member(7).map(|()| 0));
    result.unwrap();
    assert_eq!((len, op), (1, WaitSetOp::RemoveMember as u64));
    assert_eq!(args, [7, 0, 0, 0, 0, 0]);
}

#[test]
fn await_encodes_the_timeout_and_decodes_the_ready_mask() {
    let ((key, op, args, len), result) = invoke((0, 0b1010, 0), |set| {
        set.await_ready(WaitSetKey::WAIT_INFINITE)
    });
    assert_eq!(result.unwrap(), 0b1010);
    assert_eq!(len, 1);
    assert_eq!(key, RawKey::new(KeySlot(9), 1).to_wire());
    assert_eq!(op, WaitSetOp::Await as u64);
    assert_eq!(args, [u64::MAX, 0, 0, 0, 0, 0]);
}

#[test]
fn wrappers_preserve_kernel_errors() {
    for (response, expected) in [
        ((6, 0, 0), "InsufficientRights"),
        ((9, 0, 0), "InvalidOperation"),
        ((24, 0, 0), "PoolExhausted"),
    ] {
        let (_, result) = invoke(response, |set| set.await_ready(1));
        let name = match result {
            Err(CapError::InsufficientRights) => "InsufficientRights",
            Err(CapError::InvalidOperation) => "InvalidOperation",
            Err(CapError::PoolExhausted) => "PoolExhausted",
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(name, expected);
    }
    let (_, result) = invoke((u64::MAX, 42, 99), |set| set.remove_member(0).map(|()| 0));
    assert!(matches!(result, Err(CapError::UnknownResponse { .. })));
}

#[test]
fn infinite_timeout_and_member_limit_are_pinned() {
    assert_eq!(WaitSetKey::WAIT_INFINITE, u64::MAX);
    assert_eq!(WaitSetKey::MAX_MEMBERS, 8);
}

#[test]
fn operation_numbers_are_pinned_and_unassigned_ones_refused() {
    for (op, number) in [
        (WaitSetOp::AddMember, 0_u8),
        (WaitSetOp::UpdateMember, 1),
        (WaitSetOp::RemoveMember, 2),
        (WaitSetOp::Await, 3),
    ] {
        assert_eq!(op as u8, number);
        assert_eq!(WaitSetOp::try_from(u64::from(number)).unwrap(), op);
    }
    assert!(matches!(
        WaitSetOp::try_from(4_u64),
        Err(CapError::InvalidOperation)
    ));
    assert!(matches!(
        WaitSetOp::try_from(u64::MAX),
        Err(CapError::InvalidOperation)
    ));
}

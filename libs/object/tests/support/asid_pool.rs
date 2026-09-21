use {
    super::ASIDPoolKey,
    std::cell::Cell,
    vesper_objects::{CapError, KeySlot, RawKey},
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

// Mirrors `libsyscall::protected_call6`, including its lint exemption.
#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn protected_call6(
    key: u64,
    op: u64,
    a0: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
    a5: u64,
) -> Response {
    respond((key, op, [a0, a1, a2, a3, a4, a5], 6))
}

/// Issue `ASIDPoolKey::assign` with the recorded key and return the captured
/// request plus the decoded ASID, or the kernel error decoded from `response`.
pub(super) fn invoke_assign(response: Response) -> Result<(Request, u16), CapError> {
    let pool_key = RawKey::new(KeySlot(5), 1);
    let as_key = RawKey::new(KeySlot(1), 1);
    let pool = ASIDPoolKey::from_key(pool_key);
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
    let result = pool.assign(as_key);
    let request = REQUEST.with(|request| request.take().expect("missing syscall request"));
    result.map(|asid| (request, asid))
}

#[test]
fn asid_pool_assign_preserves_request_encoding_and_decodes_success() {
    let ((key, op, args, len), asid) = invoke_assign((0, 1, 0)).map_err(CapError::code).unwrap();
    assert_eq!(len, 6);
    assert_eq!(key, RawKey::new(KeySlot(5), 1).to_wire());
    assert_eq!(op, vesper_objects::ASIDPoolOp::Assign as u64);
    assert_eq!(args, [RawKey::new(KeySlot(1), 1).to_wire(), 0, 0, 0, 0, 0]);
    assert_eq!(asid, 1);
}

#[test]
fn asid_pool_assign_rejects_out_of_range_success_word() {
    // The wire contract promises a 16-bit ASID; a wider success word is a
    // contract violation reported as a defined error, not a truncation.
    assert!(matches!(
        invoke_assign((0, 0x1_0000, 0)),
        Err(CapError::InvalidOperation)
    ));
}

#[test]
fn asid_pool_wrappers_preserve_kernel_errors() {
    assert!(matches!(
        invoke_assign((9, 0, 0)),
        Err(CapError::ASIDPoolExhausted)
    ));
    assert!(matches!(invoke_assign((6, 0, 0)), Err(CapError::NotMapped)));
    assert!(matches!(
        invoke_assign((7, 0, 0)),
        Err(CapError::AlreadyMapped)
    ));
    assert!(matches!(
        invoke_assign((5, 0, 0)),
        Err(CapError::InsufficientRights)
    ));
    assert!(matches!(
        invoke_assign((u64::MAX, 42, 99)),
        Err(CapError::UnknownResponse { .. })
    ));
}

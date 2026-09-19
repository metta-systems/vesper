use {
    super::PageTableKey,
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

/// Issue `PageTableKey::map` with the recorded key and return the captured
/// request, or the kernel error decoded from `response`.
pub(super) fn invoke_map(response: Response) -> Result<Request, CapError> {
    let pt_key = RawKey::new(KeySlot(12), 3);
    let parent_key = RawKey::new(KeySlot(1), 1);
    let pt = PageTableKey::from_key(pt_key);
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
    let result = pt.map(parent_key, 0x0000_0040_0000);
    captured(result)
}

/// Issue `PageTableKey::unmap` with the recorded key and return the captured
/// request, or the kernel error decoded from `response`.
pub(super) fn invoke_unmap(response: Response) -> Result<Request, CapError> {
    let pt_key = RawKey::new(KeySlot(12), 3);
    let pt = PageTableKey::from_key(pt_key);
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
    let result = pt.unmap();
    captured(result)
}

fn captured(result: Result<(), CapError>) -> Result<Request, CapError> {
    // Drain the recorded request on both paths: the transport always records
    // one, including error responses.
    let request = REQUEST.with(|request| request.take().expect("missing syscall request"));
    result.map(|()| request)
}

#[test]
fn page_table_map_preserves_request_encoding_and_accepts_success() {
    let (key, op, args, len) = invoke_map((0, 0, 0)).map_err(CapError::code).unwrap();
    assert_eq!(len, 6);
    assert_eq!(key, RawKey::new(KeySlot(12), 3).to_wire());
    assert_eq!(op, vesper_objects::PageTableOp::Map as u64);
    assert_eq!(
        args,
        [
            RawKey::new(KeySlot(1), 1).to_wire(),
            0x0000_0040_0000,
            0,
            0,
            0,
            0
        ]
    );
}

#[test]
fn page_table_unmap_preserves_request_encoding_and_accepts_success() {
    let (key, op, args, len) = invoke_unmap((0, 0, 0)).map_err(CapError::code).unwrap();
    assert_eq!(len, 6);
    assert_eq!(key, RawKey::new(KeySlot(12), 3).to_wire());
    assert_eq!(op, vesper_objects::PageTableOp::Unmap as u64);
    assert_eq!(args, [0, 0, 0, 0, 0, 0]);
}

#[test]
fn page_table_wrappers_preserve_kernel_errors() {
    assert!(matches!(
        invoke_map((7, 0, 0)),
        Err(CapError::AlreadyMapped)
    ));
    assert!(matches!(invoke_unmap((6, 0, 0)), Err(CapError::NotMapped)));
    assert!(matches!(
        invoke_map((u64::MAX, 42, 99)),
        Err(CapError::UnknownResponse { .. })
    ));
}

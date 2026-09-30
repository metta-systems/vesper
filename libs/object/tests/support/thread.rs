use {
    super::{DomainId, ThreadKey},
    std::cell::Cell,
    vesper_objects::{
        CapError, CoreType, InconsistencyReason, InvalidKeyReason, Key, KeySlot, RawKey,
    },
};

type Response = (u64, u64, u64);
type Request = (u64, u64, Option<(u64, u64)>);

std::thread_local! {
    static RESPONSE: Cell<Option<Response>> = const { Cell::new(None) };
    static REQUEST: Cell<Option<Request>> = const { Cell::new(None) };
}

fn respond(request: Request) -> Response {
    REQUEST.with(|recorded| assert!(recorded.replace(Some(request)).is_none()));
    RESPONSE.with(|response| response.take().expect("unexpected syscall"))
}

pub(super) unsafe fn protected_call0(key: u64, op: u64) -> Response {
    respond((key, op, None))
}

pub(super) unsafe fn protected_call2(key: u64, op: u64, a0: u64, a1: u64) -> Response {
    respond((key, op, Some((a0, a1))))
}

fn invoke(op: u64, response: Response) -> Result<(), CapError> {
    let thread_key = RawKey::new(KeySlot(u32::MAX), 0x89ab_cdef);
    let source_key = RawKey::new(KeySlot(u32::MAX - 1), 0x7654_3210);
    // Exercise mutations only; this fixture does not establish a DCB mapping.
    let thread = ThreadKey {
        key: Key::new(thread_key),
        id: DomainId::INVALID,
    };
    let source = Key::<()>::new(source_key);
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
    let result = match op {
        1 => thread.grant(&source, KeySlot(u32::MAX - 2)),
        2 => thread.suspend(),
        3 => thread.resume(),
        4 => thread.retire(),
        _ => panic!("unexpected test operation"),
    };
    let args = (op == 1).then_some((0x7654_3210_ffff_fffe, 0x0000_0000_ffff_fffd));
    REQUEST.with(|request| assert_eq!(request.take(), Some((0x89ab_cdef_ffff_ffff, op, args))));
    RESPONSE.with(|pending| assert!(pending.get().is_none()));
    assert_eq!(thread.key.raw(), thread_key);
    assert_eq!(thread.id, DomainId::INVALID);
    assert_eq!(source.raw(), source_key);
    result
}

#[test]
fn from_key_preserves_key_and_id_without_validation() {
    let key = RawKey::new(KeySlot(1), 7);
    let thread = ThreadKey::from_key(key, DomainId(3));
    assert_eq!(thread.key.raw(), key);
    assert_eq!(thread.id, DomainId(3));
}

#[test]
fn all_thread_wrappers_preserve_request_encoding_and_accept_success() {
    for op in 1..=4 {
        assert_eq!(
            invoke(op, (0, u64::MAX, 1 << 63)).map_err(CapError::code),
            Ok(())
        );
    }
}

#[test]
fn all_thread_wrappers_report_unsupported_dispatch_and_lookup_errors() {
    for op in 1..=4 {
        assert!(matches!(
            invoke(op, (16, 3, 0)),
            Err(CapError::UnsupportedCoreType(CoreType::Thread))
        ));
        assert!(matches!(
            invoke(op, (3, 0, 0)),
            Err(CapError::InvalidDomain)
        ));
        assert!(matches!(
            invoke(op, (11, u64::from(u32::MAX), 0)),
            Err(CapError::InvalidSlot(KeySlot(u32::MAX)))
        ));
        assert!(matches!(
            invoke(op, (12, 42, 0)),
            Err(CapError::EmptySlot(KeySlot(42)))
        ));
        assert!(matches!(
            invoke(op, (5, 0, 0)),
            Err(CapError::InsufficientRights)
        ));
    }
}

#[test]
fn all_thread_wrappers_preserve_key_diagnostics() {
    let wire_key = 0x7654_3210_ffff_fffe;
    for op in 1..=4 {
        match invoke(op, (26, wire_key, 0x0202)) {
            Err(CapError::InvalidKey {
                key,
                reason,
                operand,
            }) => {
                assert_eq!(key.to_wire(), wire_key);
                assert_eq!(reason, InvalidKeyReason::SlotOutOfRange);
                assert_eq!(operand, 2);
            }
            _ => panic!("lost invalid-key diagnostic"),
        }
        match invoke(op, (27, wire_key, 0x0201)) {
            Err(CapError::InconsistentKey {
                key,
                reason,
                operand,
            }) => {
                assert_eq!(key.to_wire(), wire_key);
                assert_eq!(reason, InconsistencyReason::SlotIncarnationMismatch);
                assert_eq!(operand, 2);
            }
            _ => panic!("lost inconsistency diagnostic"),
        }
        assert_eq!(
            invoke(op, (28, 42, 0)).map_err(CapError::code),
            Err((28, 42, 0))
        );
    }
}

#[test]
fn all_thread_wrappers_preserve_unknown_statuses_and_malformed_details() {
    for op in 1..=4 {
        for wire in [
            (u64::MAX, 42, 99),
            (1 << 32, 1, 2),
            (16, 258, 0),
            (3, 0, 1),
            (26, 0x7654_3210_ffff_fffe, 0x0801),
            (27, 0x7654_3210_ffff_fffe, 0x0100_0001),
            (28, 1 << 32, 0),
        ] {
            match invoke(op, wire) {
                Err(error @ CapError::UnknownResponse { .. }) => assert_eq!(error.code(), wire),
                _ => panic!("lost error details for operation {op}"),
            }
        }
    }
}

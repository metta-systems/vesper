use {
    super::AddressSpaceKey,
    std::cell::Cell,
    vesper_objects::{ArchType, CapError, InconsistencyReason, InvalidKeyReason, RawKey},
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

fn invoke(op: u64, response: Response) -> Result<(), CapError> {
    let key = RawKey::new(vesper_objects::KeySlot(u32::MAX), 0x89ab_cdef);
    let address_space = AddressSpaceKey::from_key(key);
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
    let result = match op {
        0 => address_space.activate(),
        1 => address_space.retire(),
        _ => panic!("unexpected test operation"),
    };
    REQUEST.with(|request| assert_eq!(request.take(), Some((0x89ab_cdef_ffff_ffff, op, None))));
    RESPONSE.with(|pending| assert!(pending.get().is_none()));
    assert_eq!(address_space.to_wire(), key.to_wire());
    result
}

#[test]
fn from_key_preserves_key_without_validation() {
    let key = RawKey::new(vesper_objects::KeySlot(1), 7);
    let address_space = AddressSpaceKey::from_key(key);
    assert_eq!(address_space.to_wire(), key.to_wire());
}

#[test]
fn all_address_space_wrappers_preserve_request_encoding_and_accept_success() {
    for op in 0..=1 {
        assert_eq!(
            invoke(op, (0, u64::MAX, 1 << 63)).map_err(CapError::code),
            Ok(())
        );
    }
}

#[test]
fn all_address_space_wrappers_report_unsupported_dispatch_and_lookup_errors() {
    for op in 0..=1 {
        assert!(matches!(
            invoke(op, (19, 2, 0)),
            Err(CapError::UnsupportedArchType(ArchType::AddressSpace))
        ));
        assert!(matches!(
            invoke(op, (3, 0, 0)),
            Err(CapError::InvalidDomain)
        ));
        assert!(matches!(
            invoke(op, (5, 0, 0)),
            Err(CapError::InsufficientRights)
        ));
    }
}

#[test]
fn all_address_space_wrappers_preserve_key_diagnostics() {
    let wire_key = 0x7654_3210_ffff_fffe;
    for op in 0..=1 {
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
fn all_address_space_wrappers_preserve_unknown_statuses_and_malformed_details() {
    for op in 0..=1 {
        for wire in [
            (u64::MAX, 42, 99),
            (1 << 32, 1, 2),
            (19, 258, 0),
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

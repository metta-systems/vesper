use {
    super::{DomainId, DomainKey},
    std::cell::Cell,
    vesper_objects::{CapError, CoreType, Key, KeySlot},
};

type Response = (u64, u64, u64);
type Request = (u32, u32, Option<(u64, u64)>);

std::thread_local! {
    static RESPONSE: Cell<Option<Response>> = const { Cell::new(None) };
    static REQUEST: Cell<Option<Request>> = const { Cell::new(None) };
}

fn respond(request: Request) -> Response {
    REQUEST.with(|recorded| assert!(recorded.replace(Some(request)).is_none()));
    RESPONSE.with(|response| response.take().expect("unexpected syscall"))
}

pub(super) unsafe fn protected_call0(slot: u32, op: u32) -> Response {
    respond((slot, op, None))
}

pub(super) unsafe fn protected_call2(slot: u32, op: u32, a0: u64, a1: u64) -> Response {
    respond((slot, op, Some((a0, a1))))
}

fn invoke(op: u32, response: Response) -> Result<(), CapError> {
    let domain = DomainKey {
        key: Key::new(KeySlot(u32::MAX)),
        id: DomainId::INVALID,
    };
    let source = Key::<()>::new(KeySlot(u32::MAX - 1));
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
    let result = match op {
        0 => domain.activate(),
        1 => domain.grant(&source, KeySlot(u32::MAX - 2)),
        2 => domain.suspend(),
        3 => domain.resume(),
        _ => panic!("unexpected test operation"),
    };
    let args = (op == 1).then_some((u64::from(u32::MAX - 1), u64::from(u32::MAX - 2)));
    REQUEST.with(|request| assert_eq!(request.take(), Some((u32::MAX, op, args))));
    RESPONSE.with(|pending| assert!(pending.get().is_none()));
    assert_eq!(domain.key.slot(), u32::MAX);
    assert_eq!(domain.id, DomainId::INVALID);
    assert_eq!(source.slot(), u32::MAX - 1);
    result
}

#[test]
fn all_domain_wrappers_preserve_request_encoding_and_accept_success() {
    for op in 0..=3 {
        assert_eq!(
            invoke(op, (0, u64::MAX, 1 << 63)).map_err(CapError::code),
            Ok(())
        );
    }
}

#[test]
fn all_domain_wrappers_report_unsupported_dispatch_and_lookup_errors() {
    for op in 0..=3 {
        assert!(matches!(
            invoke(op, (16, 2, 0)),
            Err(CapError::UnsupportedCoreType(CoreType::Domain))
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
fn all_domain_wrappers_preserve_unknown_statuses_and_malformed_details() {
    for op in 0..=3 {
        for wire in [(u64::MAX, 42, 99), (1 << 32, 1, 2), (16, 258, 0), (3, 0, 1)] {
            match invoke(op, wire) {
                Err(error @ CapError::UnknownResponse { .. }) => assert_eq!(error.code(), wire),
                _ => panic!("lost error details for operation {op}"),
            }
        }
    }
}

use {
    super::KeyTableKey,
    std::cell::Cell,
    vesper_objects::{CapError, CoreType, Key, KeySlot, Rights},
};

type Response = (u64, u64, u64);
type Request = (u32, u32, [u64; 4], usize);

std::thread_local! {
    static RESPONSE: Cell<Option<Response>> = const { Cell::new(None) };
    static REQUEST: Cell<Option<Request>> = const { Cell::new(None) };
}

fn respond(request: Request) -> Response {
    REQUEST.with(|recorded| assert!(recorded.replace(Some(request)).is_none()));
    RESPONSE.with(|response| response.take().expect("unexpected syscall"))
}

pub(super) unsafe fn protected_call1(slot: u32, op: u32, a0: u64) -> Response {
    respond((slot, op, [a0, 0, 0, 0], 1))
}

pub(super) unsafe fn protected_call4(
    slot: u32,
    op: u32,
    a0: u64,
    a1: u64,
    a2: u64,
    a3: u64,
) -> Response {
    respond((slot, op, [a0, a1, a2, a3], 4))
}

#[derive(Clone, Copy, Debug)]
enum Method {
    CopyDerive,
    Delete,
    Revoke,
    GrantTo,
}

const METHODS: [Method; 4] = [
    Method::CopyDerive,
    Method::Delete,
    Method::Revoke,
    Method::GrantTo,
];

fn invoke(method: Method, response: Response) -> Result<(), CapError> {
    let mut table = KeyTableKey {
        key: Key::new(KeySlot(u32::MAX)),
    };
    let other = KeyTableKey {
        key: Key::new(KeySlot(u32::MAX - 1)),
    };
    let src = u32::MAX - 2;
    let dst = u32::MAX - 3;
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
    let (result, op, args, arity) = match method {
        Method::CopyDerive => (
            table.copy_derive(src, &other, dst, Rights(Rights::READ)),
            0,
            [u64::from(src), u64::from(u32::MAX - 1), u64::from(dst), 1],
            4,
        ),
        Method::Delete => (table.delete(src), 2, [u64::from(src), 0, 0, 0], 1),
        Method::Revoke => (table.revoke(&other, src), 4, [u64::from(src), 0, 0, 0], 1),
        Method::GrantTo => (
            table.grant_to(src, &other, dst),
            0,
            [u64::from(src), u64::from(u32::MAX - 1), u64::from(dst), 15],
            4,
        ),
    };
    REQUEST.with(|request| assert_eq!(request.take(), Some((u32::MAX, op, args, arity))));
    RESPONSE.with(|pending| assert!(pending.get().is_none()));
    assert_eq!(table.key.slot(), u32::MAX);
    assert_eq!(other.key.slot(), u32::MAX - 1);
    result
}

#[test]
fn all_key_table_wrappers_preserve_request_encoding_and_accept_success() {
    for method in METHODS {
        assert_eq!(
            invoke(method, (0, u64::MAX, 1 << 63)).map_err(CapError::code),
            Ok(())
        );
    }
}

#[test]
fn all_key_table_wrappers_report_unsupported_dispatch_and_authority_errors() {
    for method in METHODS {
        assert!(matches!(
            invoke(method, (16, 3, 0)),
            Err(CapError::UnsupportedCoreType(CoreType::KeyTable))
        ));
        assert!(matches!(
            invoke(method, (3, 0, 0)),
            Err(CapError::InvalidDomain)
        ));
        assert!(matches!(
            invoke(method, (11, u64::from(u32::MAX), 0)),
            Err(CapError::InvalidSlot(KeySlot(u32::MAX)))
        ));
        assert!(matches!(
            invoke(method, (12, 42, 0)),
            Err(CapError::EmptySlot(KeySlot(42)))
        ));
        assert!(matches!(
            invoke(method, (13, 43, 0)),
            Err(CapError::SlotOccupied(KeySlot(43)))
        ));
        assert!(matches!(
            invoke(method, (5, 0, 0)),
            Err(CapError::InsufficientRights)
        ));
    }
}

#[test]
fn all_key_table_wrappers_preserve_unknown_statuses_and_malformed_details() {
    for method in METHODS {
        for wire in [
            (u64::MAX, 42, 99),
            (1 << 32, 1, 2),
            (16, 259, 0),
            (11, 1 << 32, 0),
            (5, 0, 1),
        ] {
            match invoke(method, wire) {
                Err(error @ CapError::UnknownResponse { .. }) => assert_eq!(error.code(), wire),
                _ => panic!("lost error details for {method:?}"),
            }
        }
    }
}

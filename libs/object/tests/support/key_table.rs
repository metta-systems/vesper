use {
    super::KeyTableKey,
    std::cell::Cell,
    vesper_objects::{
        CapError, CoreType, InconsistencyReason, InvalidKeyReason, KeySlot, RawKey, Rights,
    },
};

type Response = (u64, u64, u64);
type Request = (u64, u64, [u64; 4], usize);

std::thread_local! {
    static RESPONSE: Cell<Option<Response>> = const { Cell::new(None) };
    static REQUEST: Cell<Option<Request>> = const { Cell::new(None) };
}

fn respond(request: Request) -> Response {
    REQUEST.with(|recorded| assert!(recorded.replace(Some(request)).is_none()));
    RESPONSE.with(|response| response.take().expect("unexpected syscall"))
}

pub(super) unsafe fn protected_call1(key: u64, op: u64, a0: u64) -> Response {
    respond((key, op, [a0, 0, 0, 0], 1))
}

pub(super) unsafe fn protected_call4(
    key: u64,
    op: u64,
    a0: u64,
    a1: u64,
    a2: u64,
    a3: u64,
) -> Response {
    respond((key, op, [a0, a1, a2, a3], 4))
}

#[derive(Clone, Copy, Debug)]
enum Method {
    CopyDerive,
    Move,
    Delete,
    Revoke,
    GrantTo,
}

const METHODS: [Method; 5] = [
    Method::CopyDerive,
    Method::Move,
    Method::Delete,
    Method::Revoke,
    Method::GrantTo,
];

fn invoke(method: Method, response: Response) -> Result<Option<RawKey>, CapError> {
    let table_key = RawKey::new(KeySlot(u32::MAX), 0x89ab_cdef);
    let other_key = RawKey::new(KeySlot(u32::MAX - 1), 0x7654_3210);
    let mut table = KeyTableKey::from_key(table_key);
    let other = KeyTableKey::from_key(other_key);
    let src = RawKey::new(KeySlot(u32::MAX - 2), 0x1357_9bdf);
    let dst = u32::MAX - 3;
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
    let (result, op, args, arity) = match method {
        Method::CopyDerive => (
            table
                .copy_derive(src, &other, dst, Rights(Rights::READ))
                .map(Some),
            0,
            [0x1357_9bdf_ffff_fffd, 0x7654_3210_ffff_fffe, 0xffff_fffc, 1],
            4,
        ),
        Method::Move => (
            table.transfer(src, &other, dst).map(Some),
            1,
            [0x1357_9bdf_ffff_fffd, 0x7654_3210_ffff_fffe, 0xffff_fffc, 0],
            4,
        ),
        Method::Delete => (
            table.delete(src).map(|()| None),
            2,
            [0x1357_9bdf_ffff_fffd, 0, 0, 0],
            1,
        ),
        Method::Revoke => (
            table.revoke(&other, src).map(|()| None),
            4,
            [0x1357_9bdf_ffff_fffd, 0, 0, 0],
            1,
        ),
        Method::GrantTo => (
            table.grant_to(src, &other, dst).map(Some),
            0,
            [
                0x1357_9bdf_ffff_fffd,
                0x7654_3210_ffff_fffe,
                0xffff_fffc,
                // The prototype `Rights::all()` request, including `EXECUTE`
                // and `RETIRE`.
                u64::from(Rights::all().bits()),
            ],
            4,
        ),
    };
    REQUEST.with(|request| {
        assert_eq!(
            request.take(),
            Some((0x89ab_cdef_ffff_ffff, op, args, arity))
        );
    });
    RESPONSE.with(|pending| assert!(pending.get().is_none()));
    assert_eq!(table.key.raw(), table_key);
    assert_eq!(other.key.raw(), other_key);
    assert_eq!(src.to_wire(), 0x1357_9bdf_ffff_fffd);
    result
}

#[test]
fn all_key_table_wrappers_preserve_request_encoding_and_accept_success() {
    for method in METHODS {
        let expected = match method {
            Method::CopyDerive | Method::Move | Method::GrantTo => Some(0xfedc_ba98_ffff_fffc),
            Method::Delete | Method::Revoke => None,
        };
        assert_eq!(
            invoke(method, (0, 0xfedc_ba98_ffff_fffc, 0))
                .map(|key| key.map(|key| key.to_wire()))
                .map_err(CapError::code),
            Ok(expected)
        );
    }
}

#[test]
fn copy_derive_move_and_grant_to_ignore_nonzero_second_success_word() {
    for method in [Method::CopyDerive, Method::Move, Method::GrantTo] {
        for second in [1, 1 << 63, u64::MAX] {
            assert_eq!(
                invoke(method, (0, 0xfedc_ba98_ffff_fffc, second))
                    .map(|key| key.map(|key| key.to_wire()))
                    .map_err(CapError::code),
                Ok(Some(0xfedc_ba98_ffff_fffc))
            );
        }
    }
}

#[test]
fn all_key_table_wrappers_report_unsupported_dispatch_and_authority_errors() {
    for method in METHODS {
        assert!(matches!(
            invoke(method, (17, 2, 0)),
            Err(CapError::UnsupportedCoreType(CoreType::KeyTable))
        ));
        assert!(matches!(
            invoke(method, (3, 0, 0)),
            Err(CapError::InvalidThread)
        ));
        assert!(matches!(
            invoke(method, (12, u64::from(u32::MAX), 0)),
            Err(CapError::InvalidSlot(KeySlot(u32::MAX)))
        ));
        assert!(matches!(
            invoke(method, (13, 42, 0)),
            Err(CapError::EmptySlot(KeySlot(42)))
        ));
        assert!(matches!(
            invoke(method, (14, 43, 0)),
            Err(CapError::SlotOccupied(KeySlot(43)))
        ));
        assert!(matches!(
            invoke(method, (6, 0, 0)),
            Err(CapError::InsufficientRights)
        ));
    }
}

#[test]
fn all_key_table_wrappers_preserve_key_diagnostics() {
    let wire_key = 0x1357_9bdf_ffff_fffd;
    for method in METHODS {
        match invoke(method, (27, wire_key, 0x0203)) {
            Err(CapError::InvalidKey {
                key,
                reason,
                operand,
            }) => {
                assert_eq!(key.to_wire(), wire_key);
                assert_eq!(reason, InvalidKeyReason::NeverIssued);
                assert_eq!(operand, 2);
            }
            _ => panic!("lost invalid-key diagnostic"),
        }
        match invoke(method, (28, wire_key, 0x0202)) {
            Err(CapError::InconsistentKey {
                key,
                reason,
                operand,
            }) => {
                assert_eq!(key.to_wire(), wire_key);
                assert_eq!(reason, InconsistencyReason::CapabilityInvalidated);
                assert_eq!(operand, 2);
            }
            _ => panic!("lost inconsistency diagnostic"),
        }
        assert_eq!(
            invoke(method, (29, 43, 0)).map_err(CapError::code),
            Err((29, 43, 0))
        );
    }
}

#[test]
fn all_key_table_wrappers_preserve_unknown_statuses_and_malformed_details() {
    for method in METHODS {
        for wire in [
            (u64::MAX, 42, 99),
            (1 << 32, 1, 2),
            (17, 259, 0),
            (12, 1 << 32, 0),
            (6, 0, 1),
            (27, 0x1357_9bdf_ffff_fffd, 0x0901),
            (28, 0x1357_9bdf_ffff_fffd, 0x0100_0001),
            (29, 1 << 32, 0),
        ] {
            match invoke(method, wire) {
                Err(error @ CapError::UnknownResponse { .. }) => assert_eq!(error.code(), wire),
                _ => panic!("lost error details for {method:?}"),
            }
        }
    }
}

std::thread_local! {
    static BADGED_REQUEST: Cell<Option<(u64, u64, [u64; 5])>> = const { Cell::new(None) };
}

// Mirrors `libsyscall::protected_call5`.
pub(super) unsafe fn protected_call5(
    key: u64,
    op: u64,
    a0: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
) -> Response {
    BADGED_REQUEST.with(|recorded| recorded.set(Some((key, op, [a0, a1, a2, a3, a4]))));
    RESPONSE.with(|response| response.take().expect("unexpected syscall"))
}

#[test]
fn copy_derive_badged_sends_the_badge_in_the_fifth_argument() {
    let table = KeyTableKey::from_key(RawKey::new(KeySlot(u32::MAX), 0x89ab_cdef));
    let other = KeyTableKey::from_key(RawKey::new(KeySlot(u32::MAX - 1), 0x7654_3210));
    let src = RawKey::new(KeySlot(u32::MAX - 2), 0x1357_9bdf);
    for badge in [1, 0x1234_5678_9abc_def0, u64::MAX] {
        RESPONSE.with(|pending| pending.set(Some((0, 0xfedc_ba98_ffff_fffc, 0))));
        let key = table
            .copy_derive_badged(
                src,
                &other,
                u32::MAX - 3,
                Rights(Rights::CALL | Rights::BADGE),
                badge,
            )
            .map_err(CapError::code)
            .unwrap();
        assert_eq!(key.to_wire(), 0xfedc_ba98_ffff_fffc);
        let (wire_key, op, args) = BADGED_REQUEST.with(|recorded| recorded.take().unwrap());
        assert_eq!(wire_key, 0x89ab_cdef_ffff_ffff);
        assert_eq!(op, 0);
        assert_eq!(
            args,
            [
                0x1357_9bdf_ffff_fffd,
                0x7654_3210_ffff_fffe,
                0xffff_fffc,
                u64::from(Rights::CALL | Rights::BADGE),
                badge
            ]
        );
    }
}

#[test]
fn copy_derive_badged_reports_already_badged_and_rights_errors() {
    let table = KeyTableKey::from_key(RawKey::new(KeySlot(u32::MAX), 0x89ab_cdef));
    let src = RawKey::new(KeySlot(u32::MAX - 2), 0x1357_9bdf);
    for (response, expected) in [
        ((46, 0, 0), "AlreadyBadged"),
        ((6, 0, 0), "InsufficientRights"),
    ] {
        RESPONSE.with(|pending| pending.set(Some(response)));
        let name = match table.copy_derive_badged(src, &table, 1, Rights(Rights::BADGE), 5) {
            Err(CapError::AlreadyBadged) => "AlreadyBadged",
            Err(CapError::InsufficientRights) => "InsufficientRights",
            _ => panic!("unexpected result"),
        };
        BADGED_REQUEST.with(Cell::take);
        assert_eq!(name, expected);
    }
}

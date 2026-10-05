use {
    super::AddressSpaceKey,
    std::cell::Cell,
    vesper_objects::{
        ArchType, CapError, InconsistencyReason, InvalidKeyReason, InvalidStackReason, KeySlot,
        KeyTableKey, RawKey,
    },
};

type Response = (u64, u64, u64);
type Request = (u64, u64, [u64; 6]);

std::thread_local! {
    static RESPONSE: Cell<Option<Response>> = const { Cell::new(None) };
    static REQUEST: Cell<Option<Request>> = const { Cell::new(None) };
}

fn respond(request: Request) -> Response {
    REQUEST.with(|recorded| assert!(recorded.replace(Some(request)).is_none()));
    RESPONSE.with(|response| response.take().expect("unexpected syscall"))
}

pub(super) unsafe fn protected_call0(key: u64, op: u64) -> Response {
    respond((key, op, [0; 6]))
}

// Mirrors `libsyscall::protected_call6`, including its lint exemption.
#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn protected_call6(
    key: u64,
    op: u64,
    arg0: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
) -> Response {
    respond((key, op, [arg0, arg1, arg2, arg3, arg4, arg5]))
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
    REQUEST.with(|request| assert_eq!(request.take(), Some((0x89ab_cdef_ffff_ffff, op, [0; 6]))));
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
fn address_space_operation_ids_are_stable() {
    use vesper_objects::address_space::AddressSpaceOp;

    assert_eq!(AddressSpaceOp::Activate as u8, 0);
    assert_eq!(AddressSpaceOp::Retire as u8, 1);
    assert_eq!(AddressSpaceOp::CreateInvocation as u8, 3);
    assert!(matches!(
        AddressSpaceOp::try_from(3),
        Ok(AddressSpaceOp::CreateInvocation)
    ));
    assert!(matches!(
        AddressSpaceOp::try_from(2),
        Err(CapError::InvalidOperation)
    ));
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
fn create_invocation_encodes_operands_and_returns_destination_key() {
    let address_space_key = RawKey::new(vesper_objects::KeySlot(1), 0x89ab_cdef);
    let destination_key = RawKey::new(vesper_objects::KeySlot(3), 0x1357_2468);
    let address_space = AddressSpaceKey::from_key(address_space_key);
    let destination = KeyTableKey::from_key(destination_key);
    let returned_key = RawKey::new(KeySlot(12), 0x2468_ace0);
    RESPONSE.with(|pending| {
        assert!(
            pending
                .replace(Some((0, returned_key.to_wire(), 0)))
                .is_none()
        );
    });

    let result = address_space.create_invocation(
        0xfeed_cafe_1234,
        &destination,
        KeySlot(12),
        0x2000_0100,
        0x2000_0190,
        0x30,
    );

    let Ok(actual_key) = result else {
        panic!("CreateInvocation wrapper returned an error");
    };
    assert_eq!(actual_key, returned_key);
    REQUEST.with(|request| {
        assert_eq!(
            request.take(),
            Some((
                address_space_key.to_wire(),
                3,
                [
                    0xfeed_cafe_1234,
                    destination_key.to_wire(),
                    12,
                    0x2000_0100,
                    0x2000_0190,
                    0x30,
                ],
            ))
        );
    });
}

#[test]
fn create_invocation_preserves_kernel_errors() {
    let address_space = AddressSpaceKey::from_key(RawKey::new(vesper_objects::KeySlot(1), 7));
    let destination = KeyTableKey::from_key(RawKey::new(vesper_objects::KeySlot(3), 9));
    RESPONSE.with(|pending| {
        assert!(
            pending
                .replace(Some((
                    vesper_objects::syscall_status::INSUFFICIENT_RIGHTS,
                    0,
                    0
                )))
                .is_none()
        );
    });

    assert!(matches!(
        address_space.create_invocation(0x1000, &destination, KeySlot(5), 0x2000, 0x3000, 0x80),
        Err(CapError::InsufficientRights)
    ));
    REQUEST.with(|request| assert!(request.take().is_some()));
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

fn invoke_create_invocation(args: [u64; 6], response: Response) -> Result<RawKey, CapError> {
    let address_space_key = RawKey::new(KeySlot(u32::MAX), 0x89ab_cdef);
    let destination_key = RawKey::from_wire(args[1]);
    let address_space = AddressSpaceKey::from_key(address_space_key);
    let destination = KeyTableKey::from_key(destination_key);
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));

    let result = address_space.create_invocation(
        args[0],
        &destination,
        KeySlot(u32::try_from(args[2]).unwrap()),
        args[3],
        args[4],
        args[5],
    );

    REQUEST.with(|request| {
        assert_eq!(request.take(), Some((0x89ab_cdef_ffff_ffff, 3, args)));
    });
    RESPONSE.with(|pending| assert!(pending.get().is_none()));
    assert_eq!(address_space.to_wire(), address_space_key.to_wire());
    assert_eq!(destination.to_wire(), destination_key.to_wire());
    result
}

const CREATE_INVOCATION_ARGS: [u64; 6] = [
    0xfedc_ba98_7654_3210,
    0x1357_2468_89ab_cdef,
    0xffff_ffff,
    0x2000_0100,
    0x2000_0190,
    0x30,
];

#[test]
fn create_invocation_preserves_full_width_operands_and_success_key() {
    for returned_key in [0, 1, 0x2468_ace0_1357_9bdf, 1 << 63, u64::MAX] {
        for second in [0, 1, 1 << 63, u64::MAX] {
            assert_eq!(
                invoke_create_invocation(CREATE_INVOCATION_ARGS, (0, returned_key, second))
                    .map(|key| key.to_wire())
                    .map_err(CapError::code),
                Ok(returned_key)
            );
        }
    }
}

#[test]
fn create_invocation_forwards_malformed_values_without_client_validation() {
    // The kernel owns stack validation and admission-stage precedence.
    for (function, base, end, minimum) in [
        (0, 0, 0, 0),
        (1, 0x1001, 0x1001, 0),
        (u64::MAX, u64::MAX, 1 << 63, u64::MAX),
        (0x1000, 0x2000, 0x3000, 0x1001),
        (1 << 63, 0x2000_0100, 0x2000_0190, 0x90),
    ] {
        let args = [
            function,
            CREATE_INVOCATION_ARGS[1],
            CREATE_INVOCATION_ARGS[2],
            base,
            end,
            minimum,
        ];
        assert_eq!(
            invoke_create_invocation(args, (5, 0, 0)).map_err(CapError::code),
            Err((5, 0, 0))
        );
    }
}

#[test]
fn create_invocation_decodes_all_typed_stack_reasons_without_losing_values() {
    for (id, reason) in [
        (1, InvalidStackReason::ExtentEmpty),
        (2, InvalidStackReason::ExtentInverted),
        (3, InvalidStackReason::BaseOutsideUserRange),
        (4, InvalidStackReason::EndOutsideUserRange),
        (5, InvalidStackReason::BaseMisaligned),
        (6, InvalidStackReason::EndMisaligned),
        (7, InvalidStackReason::MinimumHeadroomZero),
        (8, InvalidStackReason::MinimumHeadroomMisaligned),
        (9, InvalidStackReason::MinimumHeadroomTooLarge),
        (10, InvalidStackReason::SpMisaligned),
        (11, InvalidStackReason::SpOutOfRange),
        (12, InvalidStackReason::SpInsufficientHeadroom),
    ] {
        for value in [0, 0x1234_5678_9abc_def0, u64::MAX] {
            match invoke_create_invocation(CREATE_INVOCATION_ARGS, (32, value, id)) {
                Err(CapError::InvalidStack {
                    value: actual_value,
                    reason: actual_reason,
                }) => {
                    assert_eq!(actual_value, value);
                    assert_eq!(actual_reason, reason);
                }
                _ => panic!("lost InvalidStack diagnostic"),
            }
        }
    }
}

#[test]
fn create_invocation_preserves_existing_admission_errors_and_key_diagnostics() {
    for wire in [
        (3, 0, 0),
        (4, 0, 0),
        (5, 0, 0),
        (11, u64::from(u32::MAX), 0),
        (13, 42, 0),
        (19, 2, 0),
        (26, 0x7654_3210_ffff_fffe, 0x0304),
        (27, 0x7654_3210_ffff_fffe, 0x0301),
        (28, 42, 0),
    ] {
        match invoke_create_invocation(CREATE_INVOCATION_ARGS, wire) {
            Err(CapError::UnknownResponse { .. }) | Ok(_) => panic!("lost known error {wire:?}"),
            Err(error) => assert_eq!(error.code(), wire),
        }
    }
}

#[test]
fn create_invocation_preserves_unknown_statuses_and_extended_stack_reasons() {
    for wire in [
        (u64::MAX, u64::MAX, 0x1234_5678_9abc_def0),
        ((1 << 32) | 32, u64::MAX, 1),
        (32, u64::MAX, 0),
        (32, u64::MAX, 13),
        (32, 0x1234_5678_9abc_def0, 0x0101),
        (32, u64::MAX, (1 << 32) | 12),
        (32, u64::MAX, (1 << 63) | 1),
        (32, u64::MAX, u64::MAX),
        (34, u64::MAX, u64::MAX),
        (5, 0, 1),
        (26, 0x7654_3210_ffff_fffe, 0x0801),
    ] {
        match invoke_create_invocation(CREATE_INVOCATION_ARGS, wire) {
            Err(error @ CapError::UnknownResponse { .. }) => assert_eq!(error.code(), wire),
            _ => panic!("lost error details for CreateInvocation {wire:?}"),
        }
    }
}

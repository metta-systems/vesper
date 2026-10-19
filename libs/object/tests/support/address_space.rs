use {
    super::AddressSpaceKey,
    std::cell::Cell,
    vesper_objects::{
        ArchType, CapError, InconsistencyReason, InvalidKeyReason, InvalidStackReason, KeySlot,
        KeyTableKey, RawKey, StackPoolHandle,
    },
};

type Response = (u64, u64, u64);
type Request = (u64, u64, [u64; 6]);

std::thread_local! {
    static RESPONSE: Cell<Option<Response>> = const { Cell::new(None) };
    static REQUEST: Cell<Option<Request>> = const { Cell::new(None) };
    static X8: Cell<Option<u64>> = const { Cell::new(None) };
}

fn respond(request: Request) -> Response {
    REQUEST.with(|recorded| assert!(recorded.replace(Some(request)).is_none()));
    RESPONSE.with(|response| response.take().expect("unexpected syscall"))
}

pub(super) unsafe fn protected_call1(key: u64, op: u64, arg0: u64) -> Response {
    respond((key, op, [arg0, 0, 0, 0, 0, 0]))
}

pub(super) unsafe fn protected_call3(key: u64, op: u64, a0: u64, a1: u64, a2: u64) -> Response {
    respond((key, op, [a0, a1, a2, 0, 0, 0]))
}

pub(super) unsafe fn protected_call0(key: u64, op: u64) -> Response {
    respond((key, op, [0; 6]))
}

// Mirrors `libsyscall::protected_call6_with_x8`, including its lint exemption;
// the `x8` word is recorded beside the request.
#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn protected_call6_with_x8(
    key: u64,
    op: u64,
    arg0: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    x8: u64,
) -> Response {
    X8.with(|recorded| recorded.set(Some(x8)));
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
    assert_eq!(AddressSpaceOp::Bind as u8, 2);
    assert_eq!(AddressSpaceOp::CreateInvocation as u8, 3);
    assert_eq!(AddressSpaceOp::CreateStackPool as u8, 4);
    assert_eq!(AddressSpaceOp::DestroyStackPool as u8, 5);
    assert!(matches!(
        AddressSpaceOp::try_from(3),
        Ok(AddressSpaceOp::CreateInvocation)
    ));
    assert!(matches!(
        AddressSpaceOp::try_from(4),
        Ok(AddressSpaceOp::CreateStackPool)
    ));
    assert!(matches!(
        AddressSpaceOp::try_from(5),
        Ok(AddressSpaceOp::DestroyStackPool)
    ));
    assert!(matches!(
        AddressSpaceOp::try_from(6),
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

    let pool = StackPoolHandle::new(5, 0x0102_0304);
    let result = address_space.create_invocation(0xfeed_cafe_1234, &destination, KeySlot(12), pool);

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
                    0x01_0203_0405,
                    0,
                    0,
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
        address_space.create_invocation(
            0x1000,
            &destination,
            KeySlot(5),
            StackPoolHandle::new(0, 1)
        ),
        Err(CapError::InsufficientRights)
    ));
    REQUEST.with(|request| assert!(request.take().is_some()));
}

#[test]
fn stack_pool_handles_round_trip_and_reject_reserved_bits() {
    for (slot, generation) in [(0, 0), (1, 1), (15, 0x7fff_ffff), (255, u32::MAX)] {
        let handle = StackPoolHandle::new(slot, generation);
        assert_eq!(StackPoolHandle::from_wire(handle.to_wire()), Some(handle));
        assert_eq!((handle.slot(), handle.generation()), (slot, generation));
        assert_eq!(handle.to_wire() >> 40, 0);
    }
    assert_eq!(StackPoolHandle::new(3, 7).to_wire(), 3 | (7 << 8));
    for word in [1 << 40, u64::MAX, 1 << 63] {
        assert_eq!(StackPoolHandle::from_wire(word), None);
    }
}

#[test]
fn create_stack_pool_encodes_extent_and_returns_the_handle() {
    let address_space_key = RawKey::new(vesper_objects::KeySlot(1), 0x89ab_cdef);
    let address_space = AddressSpaceKey::from_key(address_space_key);
    let returned = StackPoolHandle::new(2, 9);
    RESPONSE.with(|pending| assert!(pending.replace(Some((0, returned.to_wire(), 0))).is_none()));

    let result = address_space.create_stack_pool(0x2000_0000, 0x3000, 16);

    assert_eq!(result.map_err(CapError::code), Ok(returned));
    REQUEST.with(|request| {
        assert_eq!(
            request.take(),
            Some((
                address_space_key.to_wire(),
                4,
                [0x2000_0000, 0x3000, 16, 0, 0, 0]
            ))
        );
    });
}

#[test]
fn create_stack_pool_decodes_every_typed_reason_without_losing_values() {
    let address_space = AddressSpaceKey::from_key(RawKey::new(KeySlot(1), 7));
    for (id, reason) in [
        (1, InvalidStackReason::BaseOutsideUserRange),
        (2, InvalidStackReason::EndOutsideUserRange),
        (3, InvalidStackReason::BaseMisaligned),
        (4, InvalidStackReason::StackBytesMisaligned),
        (5, InvalidStackReason::StackBytesTooSmall),
        (6, InvalidStackReason::StackCountOutOfRange),
    ] {
        for value in [0, 0x1234_5678_9abc_def0, u64::MAX] {
            RESPONSE.with(|pending| assert!(pending.replace(Some((33, value, id))).is_none()));
            let result = address_space.create_stack_pool(0x2000, 0x1000, 4);
            REQUEST.with(|request| assert!(request.take().is_some()));
            match result {
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
fn create_stack_pool_reports_a_full_table() {
    let address_space = AddressSpaceKey::from_key(RawKey::new(KeySlot(1), 7));
    RESPONSE.with(|pending| {
        assert!(
            pending
                .replace(Some((vesper_objects::syscall_status::POOL_EXHAUSTED, 0, 0)))
                .is_none()
        );
    });
    assert!(matches!(
        address_space.create_stack_pool(0x2000, 0x1000, 4),
        Err(CapError::PoolExhausted)
    ));
    REQUEST.with(|request| assert!(request.take().is_some()));
}

#[test]
fn destroy_stack_pool_encodes_the_handle_and_decodes_statuses() {
    let address_space_key = RawKey::new(vesper_objects::KeySlot(1), 0x89ab_cdef);
    let address_space = AddressSpaceKey::from_key(address_space_key);
    let handle = StackPoolHandle::new(4, 0x55);
    for (response, expected) in [
        ((0, 0, 0), Ok(())),
        ((9, 0, 0), Err(CapError::InvalidOperation.code())),
        (
            (42, 7, 0),
            Err(CapError::StaleGeneration { current: 7 }.code()),
        ),
        ((6, 0, 0), Err(CapError::InsufficientRights.code())),
    ] {
        RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
        assert_eq!(
            address_space
                .destroy_stack_pool(handle)
                .map_err(CapError::code),
            expected
        );
        REQUEST.with(|request| {
            assert_eq!(
                request.take(),
                Some((
                    address_space_key.to_wire(),
                    5,
                    [handle.to_wire(), 0, 0, 0, 0, 0]
                ))
            );
        });
    }
}

#[test]
fn all_address_space_wrappers_report_unsupported_dispatch_and_lookup_errors() {
    for op in 0..=1 {
        assert!(matches!(
            invoke(op, (20, 2, 0)),
            Err(CapError::UnsupportedArchType(ArchType::AddressSpace))
        ));
        assert!(matches!(
            invoke(op, (4, 0, 0)),
            Err(CapError::InvalidAddressSpace)
        ));
        assert!(matches!(
            invoke(op, (6, 0, 0)),
            Err(CapError::InsufficientRights)
        ));
    }
}

#[test]
fn all_address_space_wrappers_preserve_key_diagnostics() {
    let wire_key = 0x7654_3210_ffff_fffe;
    for op in 0..=1 {
        match invoke(op, (27, wire_key, 0x0202)) {
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
        match invoke(op, (28, wire_key, 0x0201)) {
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
            invoke(op, (29, 42, 0)).map_err(CapError::code),
            Err((29, 42, 0))
        );
    }
}

#[test]
fn all_address_space_wrappers_preserve_unknown_statuses_and_malformed_details() {
    for op in 0..=1 {
        for wire in [
            (u64::MAX, 42, 99),
            (1 << 32, 1, 2),
            (20, 258, 0),
            (3, 0, 1),
            (27, 0x7654_3210_ffff_fffe, 0x0901),
            (28, 0x7654_3210_ffff_fffe, 0x0100_0001),
            (29, 1 << 32, 0),
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
        StackPoolHandle::from_wire(args[3]).unwrap(),
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
    0x00_0102_0304,
    0,
    0,
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
fn create_invocation_forwards_function_and_pool_without_client_validation() {
    // The kernel owns admission: a zero function address and a stale handle
    // are submitted as given.
    for (function, pool) in [
        (0, 0),
        (1, 0xff),
        (u64::MAX, 0xff_ffff_ffff),
        (0x1000, 0x1_0000_0001),
        (1 << 63, 0x01_0203_0405),
    ] {
        let args = [
            function,
            CREATE_INVOCATION_ARGS[1],
            CREATE_INVOCATION_ARGS[2],
            pool,
            0,
            0,
        ];
        assert_eq!(
            invoke_create_invocation(args, (6, 0, 0)).map_err(CapError::code),
            Err((6, 0, 0))
        );
    }
}

#[test]
fn create_invocation_preserves_existing_admission_errors_and_key_diagnostics() {
    for wire in [
        (3, 0, 0),
        (4, 0, 0),
        (5, 0, 0),
        (6, 0, 0),
        (12, u64::from(u32::MAX), 0),
        (14, 42, 0),
        (20, 2, 0),
        (27, 0x7654_3210_ffff_fffe, 0x0304),
        (28, 0x7654_3210_ffff_fffe, 0x0301),
        (29, 42, 0),
        (42, 7, 0),
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
        ((1 << 32) | 33, u64::MAX, 1),
        (33, u64::MAX, 0),
        (33, u64::MAX, 7),
        (33, 0x1234_5678_9abc_def0, 0x0101),
        (33, u64::MAX, (1 << 32) | 6),
        (33, u64::MAX, (1 << 63) | 1),
        (33, u64::MAX, u64::MAX),
        (36, u64::MAX, u64::MAX),
        (6, 0, 1),
        (27, 0x7654_3210_ffff_fffe, 0x0901),
    ] {
        match invoke_create_invocation(CREATE_INVOCATION_ARGS, wire) {
            Err(error @ CapError::UnknownResponse { .. }) => assert_eq!(error.code(), wire),
            _ => panic!("lost error details for CreateInvocation {wire:?}"),
        }
    }
}

fn submitted_x8(window: Option<vesper_objects::PlacementWindow>) -> u64 {
    let address_space = AddressSpaceKey::from_key(RawKey::new(KeySlot(1), 0x89ab_cdef));
    let destination = KeyTableKey::from_key(RawKey::new(KeySlot(3), 0x1357_2468));
    X8.with(|recorded| recorded.set(None));
    RESPONSE.with(|pending| {
        assert!(
            pending
                .replace(Some((0, RawKey::new(KeySlot(12), 1).to_wire(), 0)))
                .is_none()
        );
    });
    let pool = StackPoolHandle::new(0, 1);
    let result = match window {
        None => address_space.create_invocation(0x1000, &destination, KeySlot(12), pool),
        Some(window) => address_space.create_invocation_with_window(
            0x1000,
            &destination,
            KeySlot(12),
            pool,
            window,
        ),
    };
    assert!(result.is_ok());
    REQUEST.with(|request| assert!(request.take().is_some()));
    X8.with(|recorded| recorded.take().expect("x8 was not submitted"))
}

#[test]
fn create_invocation_without_a_window_submits_zero_in_x8() {
    assert_eq!(submitted_x8(None), 0);
}

#[test]
fn create_invocation_with_window_submits_first_and_count_in_x8() {
    use vesper_objects::PlacementWindow;

    assert_eq!(
        submitted_x8(Some(PlacementWindow::new(40, 3))),
        40 | (3 << 32)
    );
    assert_eq!(
        submitted_x8(Some(PlacementWindow::new(u32::MAX, u32::MAX))),
        u64::MAX
    );
    // An empty window is the zero word whatever its first slot.
    assert_eq!(submitted_x8(Some(PlacementWindow::new(40, 0))), 0);
}

#[test]
fn placement_window_wire_form_round_trips() {
    use vesper_objects::PlacementWindow;

    for (first, count) in [(0, 1), (7, 2), (u32::MAX, 1), (1, u32::MAX)] {
        let window = PlacementWindow::new(first, count);
        assert_eq!(PlacementWindow::from_wire(window.to_wire()), window);
        assert_eq!((window.first(), window.count()), (first, count));
        assert!(window.accepts_keys());
    }
    // A zero count normalizes to the empty window, whatever the first word.
    for word in [0_u64, 0x0000_0000_dead_beef, 0xffff_ffff] {
        let window = PlacementWindow::from_wire(word);
        assert_eq!(window, PlacementWindow::NONE);
        assert!(!window.accepts_keys());
        assert_eq!(window.to_wire(), 0);
    }
    assert_eq!(PlacementWindow::new(5, 3).end(), Some(8));
    assert_eq!(PlacementWindow::new(u32::MAX, 2).end(), None);
}

#[test]
fn bind_encodes_the_table_key_and_decodes_its_statuses() {
    let address_space_key = RawKey::new(KeySlot(2), 0x89ab_cdef);
    let table_key = RawKey::new(KeySlot(4), 0x1357_2468);
    let address_space = AddressSpaceKey::from_key(address_space_key);
    let table = KeyTableKey::from_key(table_key);
    for (response, expected) in [
        ((0, 0, 0), Ok(())),
        ((40, 0, 0), Err(CapError::NotBound.code())),
        ((8, 0, 0), Err(CapError::AlreadyMapped.code())),
    ] {
        RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
        assert_eq!(address_space.bind(&table).map_err(CapError::code), expected);
        REQUEST.with(|request| {
            assert_eq!(
                request.take(),
                Some((
                    address_space_key.to_wire(),
                    2,
                    [table_key.to_wire(), 0, 0, 0, 0, 0]
                ))
            );
        });
    }
}

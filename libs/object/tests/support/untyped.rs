use {
    super::UntypedKey,
    std::cell::Cell,
    vesper_objects::{CapError, KeySlot, ObjectType, RawKey, Rights},
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

fn invoke_with(
    kind: ObjectType,
    size_bits: u8,
    guard: u32,
    response: Response,
) -> Result<RawKey, CapError> {
    let untyped_key = RawKey::new(KeySlot(4), 0x89ab_cdef);
    let table_key = RawKey::new(KeySlot(u32::MAX), 0x1357_9bdf);
    let untyped = UntypedKey::from_key(untyped_key);
    let table = vesper_objects::KeyTableKey::from_key(table_key);
    let dst = u32::MAX - 1;
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
    let result = untyped.retype(
        kind,
        size_bits,
        guard,
        2,
        &table,
        dst,
        Rights(Rights::DERIVE | Rights::REMOVE | Rights::INSTALL),
    );
    REQUEST.with(|request| {
        assert_eq!(
            request.take(),
            Some((
                0x89ab_cdef_0000_0004,
                0,
                [
                    u64::from(kind.as_u8()),
                    (u64::from(guard) << 8) | u64::from(size_bits),
                    2,
                    0x1357_9bdf_ffff_ffff,
                    0xffff_fffe,
                    7
                ],
                6,
            ))
        );
    });
    RESPONSE.with(|pending| assert!(pending.get().is_none()));
    assert_eq!(untyped_key.to_wire(), 0x89ab_cdef_0000_0004);
    assert_eq!(table_key.to_wire(), 0x1357_9bdf_ffff_ffff);
    result
}

fn invoke(response: Response) -> Result<RawKey, CapError> {
    invoke_with(ObjectType::KEY_TABLE, 8, 0, response)
}

#[test]
fn retype_preserves_request_encoding_and_accepts_success() {
    assert_eq!(
        invoke((0, 0xfedc_ba98_ffff_fffc, 0))
            .map(|key| key.to_wire())
            .map_err(CapError::code),
        Ok(0xfedc_ba98_ffff_fffc)
    );
}

#[test]
fn retype_ignores_nonzero_second_success_word() {
    for second in [1, 1 << 63, u64::MAX] {
        assert_eq!(
            invoke((0, 0xfedc_ba98_ffff_fffc, second))
                .map(|key| key.to_wire())
                .map_err(CapError::code),
            Ok(0xfedc_ba98_ffff_fffc)
        );
    }
}

#[test]
fn retype_reports_unsupported_dispatch_and_authority_errors() {
    assert!(matches!(
        invoke((16, 1, 0)),
        Err(CapError::UnsupportedCoreType(
            vesper_objects::CoreType::Untyped
        ))
    ));
    assert!(matches!(invoke((3, 0, 0)), Err(CapError::InvalidDomain)));
    assert!(matches!(
        invoke((5, 0, 0)),
        Err(CapError::InsufficientRights)
    ));
    assert!(matches!(
        invoke((22, 0, 0)),
        Err(CapError::InsufficientMemory)
    ));
    assert!(matches!(invoke((24, 0, 0)), Err(CapError::InvalidSize(0))));
}

#[test]
fn retype_preserves_key_diagnostics() {
    let wire_key = 0x1357_9bdf_ffff_fffd;
    match invoke((26, wire_key, 0x0203)) {
        Err(CapError::InvalidKey {
            key,
            reason,
            operand,
        }) => {
            assert_eq!(key.to_wire(), wire_key);
            assert_eq!(reason, vesper_objects::InvalidKeyReason::NeverIssued);
            assert_eq!(operand, 2);
        }
        _ => panic!("lost invalid-key diagnostic"),
    }
    match invoke((27, wire_key, 0x0202)) {
        Err(CapError::InconsistentKey {
            key,
            reason,
            operand,
        }) => {
            assert_eq!(key.to_wire(), wire_key);
            assert_eq!(
                reason,
                vesper_objects::InconsistencyReason::CapabilityInvalidated
            );
            assert_eq!(operand, 2);
        }
        _ => panic!("lost inconsistency diagnostic"),
    }
}

#[test]
fn retype_preserves_unknown_statuses_and_malformed_details() {
    for wire in [
        (u64::MAX, 42, 99),
        (1 << 32, 1, 2),
        (16, 259, 0),
        (11, 1 << 32, 0),
        (5, 0, 1),
        (26, 0x1357_9bdf_ffff_fffd, 0x0801),
        (27, 0x1357_9bdf_ffff_fffd, 0x0100_0001),
        (28, 1 << 32, 0),
    ] {
        match invoke(wire) {
            Err(error @ CapError::UnknownResponse { .. }) => assert_eq!(error.code(), wire),
            _ => panic!("lost error details for {wire:?}"),
        }
    }
}

#[test]
fn retype_encodes_frame_kind_and_granule_size_bits() {
    // Frame is an architecture kind: wire kind 0x80 (arch bit | Frame 0)
    // with size_bits 12, the AArch64 4 KiB granule baseline.
    assert_eq!(
        invoke_with(ObjectType::FRAME, 12, 0, (0, 0xfedc_ba98_ffff_fffc, 0))
            .map(|key| key.to_wire())
            .map_err(CapError::code),
        Ok(0xfedc_ba98_ffff_fffc)
    );
}

#[test]
fn retype_encodes_untyped_split_kind_and_size_bits() {
    // The Untyped split: wire kind 0x01 (core Untyped) with the child
    // region size exponent (here 8 = a 256-byte child).
    assert_eq!(
        invoke_with(ObjectType::UNTYPED, 8, 0, (0, 0xfedc_ba98_ffff_fffc, 0))
            .map(|key| key.to_wire())
            .map_err(CapError::code),
        Ok(0xfedc_ba98_ffff_fffc)
    );
}

#[test]
fn retype_encodes_the_keytable_guard_into_the_size_word() {
    // The KeyTable guard rides bits 39:8 of `x3` above the capacity
    // exponent (selected 2026-09-23); every other kind passes zero.
    assert_eq!(
        invoke_with(
            ObjectType::KEY_TABLE,
            8,
            0xC0F_FEE,
            (0, 0xfedc_ba98_ffff_fffc, 0)
        )
        .map(|key| key.to_wire())
        .map_err(CapError::code),
        Ok(0xfedc_ba98_ffff_fffc)
    );
}

#[test]
fn retype_preserves_invalid_frame_size_errors() {
    // Non-granular frame sizes are rejected with the requested size, from
    // size_bits 0 (no 1-byte frames) up.
    assert!(matches!(
        invoke_with(ObjectType::FRAME, 0, 0, (25, 0, 0)),
        Err(CapError::InvalidFrameSize(0))
    ));
    assert!(matches!(
        invoke_with(ObjectType::FRAME, 13, 0, (25, 13, 0)),
        Err(CapError::InvalidFrameSize(13))
    ));
}

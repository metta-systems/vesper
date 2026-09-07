use vesper_objects::{
    CapError, InconsistencyReason, InvalidKeyReason, Key, KeySlot, RawKey, decode_syscall_result,
    syscall_status,
};

fn assert_unknown_response(wire: (u64, u64, u64)) {
    match decode_syscall_result(wire) {
        Err(CapError::UnknownResponse {
            status,
            detail1,
            detail2,
        }) => {
            assert_eq!((status.get(), detail1, detail2), wire);
            assert_eq!(
                CapError::UnknownResponse {
                    status,
                    detail1,
                    detail2,
                }
                .code(),
                wire
            );
        }
        _ => panic!("expected lossless fallback for {wire:?}"),
    }
}

#[test]
fn raw_key_const_api_pins_literal_wire_halves() {
    const RAW: RawKey = RawKey::new(KeySlot(0x89ab_cdef), 0x0123_4567);
    const DECODED: RawKey = RawKey::from_wire(0x0123_4567_89ab_cdef);
    const WIRE: u64 = RAW.to_wire();
    const SLOT: KeySlot = RAW.slot();
    const INCARNATION: u32 = RAW.incarnation();

    assert_eq!(RAW, DECODED);
    assert_eq!(WIRE, 0x0123_4567_89ab_cdef);
    assert_eq!(SLOT, KeySlot(0x89ab_cdef));
    assert_eq!(INCARNATION, 0x0123_4567);

    // Decoding is representation-only, even for zero incarnation or huge slots.
    for (wire, slot, incarnation) in [
        (0x0000_0000_0000_0000, 0, 0),
        (0x0000_0000_ffff_ffff, u32::MAX, 0),
        (0x0000_0001_0000_0000, 0, 1),
        (0x0000_0001_ffff_ffff, u32::MAX, 1),
        (0x7fff_ffff_8000_0000, 0x8000_0000, 0x7fff_ffff),
        (0x8000_0000_7fff_ffff, 0x7fff_ffff, 0x8000_0000),
        (0xffff_ffff_0000_0000, 0, u32::MAX),
        (0xffff_ffff_ffff_ffff, u32::MAX, u32::MAX),
    ] {
        let raw = RawKey::from_wire(wire);
        assert_eq!(raw.slot(), KeySlot(slot));
        assert_eq!(raw.incarnation(), incarnation);
        assert_eq!(raw.to_wire(), wire);
        assert_eq!(RawKey::new(KeySlot(slot), incarnation).to_wire(), wire);
    }
}

#[test]
fn raw_key_preserves_every_bit_and_field_boundaries() {
    for bit in 0..64 {
        let wire = 1_u64 << bit;
        assert_eq!(RawKey::from_wire(wire).to_wire(), wire);
        assert_eq!(RawKey::from_wire(!wire).to_wire(), !wire);
    }
    let boundaries = [0, 1, 255, 256, 0x7fff_ffff, 0x8000_0000, u32::MAX];
    for slot in boundaries {
        for incarnation in boundaries {
            let raw = RawKey::new(KeySlot(slot), incarnation);
            assert_eq!(RawKey::from_wire(raw.to_wire()), raw);
            assert_eq!(raw.to_wire() & 0xffff_ffff, u64::from(slot));
            assert_eq!(raw.to_wire() >> 32, u64::from(incarnation));
        }
    }
}

#[test]
fn typed_key_is_const_nonowning_and_has_no_phantom_trait_bounds() {
    struct NonCopyObject;

    fn copy_handle<T: Copy>(key: T) -> (T, T) {
        (key, key)
    }
    fn clone_handle<T: Clone>(key: &T) -> T {
        key.clone()
    }
    fn require_eq<T: Eq>() {}

    const KEY: Key<NonCopyObject> = Key::new(RawKey::new(KeySlot(0x89ab_cdef), 7));
    const RAW: RawKey = KEY.raw();
    const WIRE: u64 = KEY.to_wire();
    const SLOT: u32 = KEY.slot();

    let key = KEY;
    let (first, second) = copy_handle(key);
    let cloned = clone_handle(&key);
    require_eq::<Key<NonCopyObject>>();
    assert_eq!(key, first);
    assert_eq!(first, second);
    assert_eq!(second, cloned);
    assert_eq!(RAW, RawKey::new(KeySlot(0x89ab_cdef), 7));
    assert_eq!(WIRE, 0x0000_0007_89ab_cdef);
    assert_eq!(SLOT, 0x89ab_cdef);
    assert_ne!(key, Key::new(RawKey::new(KeySlot(SLOT), 8)));
    assert_ne!(key, Key::new(RawKey::new(KeySlot(0), 7)));
    assert!(!core::mem::needs_drop::<Key<String>>());
}

#[test]
fn key_statuses_and_all_reason_bytes_are_pinned() {
    assert_eq!(syscall_status::INVALID_KEY, 26);
    assert_eq!(syscall_status::INCONSISTENT_KEY, 27);
    assert_eq!(syscall_status::KEY_SLOT_EXHAUSTED, 28);

    for byte in 0..=u8::MAX {
        let invalid = match byte {
            1 => Ok(InvalidKeyReason::ZeroIncarnation),
            2 => Ok(InvalidKeyReason::SlotOutOfRange),
            3 => Ok(InvalidKeyReason::NeverIssued),
            _ => Err(()),
        };
        let inconsistent = match byte {
            1 => Ok(InconsistencyReason::SlotIncarnationMismatch),
            2 => Ok(InconsistencyReason::CapabilityInvalidated),
            3 => Ok(InconsistencyReason::ObjectRetired),
            _ => Err(()),
        };
        assert_eq!(InvalidKeyReason::try_from(byte), invalid);
        assert_eq!(InconsistencyReason::try_from(byte), inconsistent);
        if let Ok(reason) = invalid {
            assert_eq!(reason as u8, byte);
        }
        if let Ok(reason) = inconsistent {
            assert_eq!(reason as u8, byte);
        }
    }
}

#[test]
fn key_errors_pin_all_reasons_operands_and_submitted_key_bits() {
    for submitted in [0, 0x0000_0000_ffff_ffff, 0x8000_0001_89ab_cdef, u64::MAX] {
        for operand in 0..=7 {
            for (reason, literal) in [
                (InvalidKeyReason::ZeroIncarnation, 0x01),
                (InvalidKeyReason::SlotOutOfRange, 0x02),
                (InvalidKeyReason::NeverIssued, 0x03),
            ] {
                let wire = (26, submitted, literal | (u64::from(operand) << 8));
                let error = CapError::InvalidKey {
                    key: RawKey::from_wire(submitted),
                    reason,
                    operand,
                };
                assert_eq!(error.code(), wire);
                match decode_syscall_result(wire) {
                    Err(CapError::InvalidKey {
                        key,
                        reason: decoded_reason,
                        operand: decoded_operand,
                    }) => {
                        assert_eq!(key.to_wire(), submitted);
                        assert_eq!(decoded_reason, reason);
                        assert_eq!(decoded_operand, operand);
                    }
                    _ => panic!("expected InvalidKey for {wire:?}"),
                }
            }
            for (reason, literal) in [
                (InconsistencyReason::SlotIncarnationMismatch, 0x01),
                (InconsistencyReason::CapabilityInvalidated, 0x02),
                (InconsistencyReason::ObjectRetired, 0x03),
            ] {
                let wire = (27, submitted, literal | (u64::from(operand) << 8));
                let error = CapError::InconsistentKey {
                    key: RawKey::from_wire(submitted),
                    reason,
                    operand,
                };
                assert_eq!(error.code(), wire);
                match decode_syscall_result(wire) {
                    Err(CapError::InconsistentKey {
                        key,
                        reason: decoded_reason,
                        operand: decoded_operand,
                    }) => {
                        assert_eq!(key.to_wire(), submitted);
                        assert_eq!(decoded_reason, reason);
                        assert_eq!(decoded_operand, operand);
                    }
                    _ => panic!("expected InconsistentKey for {wire:?}"),
                }
            }
        }
    }
}

#[test]
fn key_error_unknown_reasons_operands_and_extension_bits_are_lossless() {
    for status in [26, 27] {
        for reason in 0..=u8::MAX {
            if !(1..=3).contains(&reason) {
                for operand in 0..=7_u64 {
                    assert_unknown_response((status, u64::MAX, u64::from(reason) | (operand << 8)));
                }
            }
        }
        for operand in 8..=u8::MAX {
            for reason in 1..=3 {
                assert_unknown_response((status, u64::MAX, reason | (u64::from(operand) << 8)));
            }
        }
        for bit in 16..64 {
            assert_unknown_response((status, 0x8000_0001_89ab_cdef, 0x0703 | (1_u64 << bit)));
        }
        assert_unknown_response((status, u64::MAX, u64::MAX));
    }
}

#[test]
fn slot_exhaustion_checks_slot_width_and_unused_detail_word() {
    for slot in [0, 1, 255, 256, 0x8000_0000, u32::MAX] {
        let wire = (28, u64::from(slot), 0);
        assert_eq!(CapError::KeySlotExhausted(KeySlot(slot)).code(), wire);
        assert!(matches!(
            decode_syscall_result(wire),
            Err(CapError::KeySlotExhausted(KeySlot(decoded))) if decoded == slot
        ));
    }
    for bit in 32..64 {
        assert_unknown_response((28, (1_u64 << bit) | 42, 0));
    }
    for bit in 0..64 {
        assert_unknown_response((28, 42, 1_u64 << bit));
    }
    assert_unknown_response((28, u64::MAX, 0));
    assert_unknown_response((28, 0, u64::MAX));
}

#[test]
fn recontextualization_changes_only_key_error_operand() {
    let key = RawKey::from_wire(0x8000_0001_89ab_cdef);
    assert_eq!(
        CapError::InvalidKey {
            key,
            reason: InvalidKeyReason::NeverIssued,
            operand: 0,
        }
        .with_key_operand(7)
        .code(),
        (26, 0x8000_0001_89ab_cdef, 0x0703)
    );
    assert_eq!(
        CapError::InconsistentKey {
            key,
            reason: InconsistencyReason::CapabilityInvalidated,
            operand: 7,
        }
        .with_key_operand(2)
        .code(),
        (27, 0x8000_0001_89ab_cdef, 0x0202)
    );
    assert_eq!(
        CapError::KeySlotExhausted(KeySlot(42))
            .with_key_operand(7)
            .code(),
        (28, 42, 0)
    );
    assert_eq!(
        CapError::InsufficientRights.with_key_operand(7).code(),
        (5, 0, 0)
    );
    let wire = (26, key.to_wire(), 0xff03);
    assert_eq!(
        CapError::InvalidKey {
            key,
            reason: InvalidKeyReason::NeverIssued,
            operand: 0,
        }
        .with_key_operand(255)
        .code(),
        wire
    );
    assert_unknown_response(wire);
    match decode_syscall_result(wire) {
        Err(error) => assert_eq!(error.with_key_operand(2).code(), wire),
        Ok(_) => panic!("invalid operand must not decode as success"),
    }
}

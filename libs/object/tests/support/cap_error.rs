use vesper_objects::{ArchType, CapError, CoreType, KeySlot, ObjectType, decode_syscall_result};

// Check literal wire values and the actual decoded variant independently.
macro_rules! assert_error {
    ($wire:expr, $error:expr, $pattern:pat $(if $guard:expr)?) => {
        assert_eq!($error.code(), $wire);
        assert!(matches!(decode_syscall_result($wire), Err($pattern) $(if $guard)?));
    };
}

fn assert_unknown_response(wire: (u64, u64, u64)) {
    match decode_syscall_result(wire) {
        Err(error @ CapError::UnknownResponse { .. }) => {
            if let CapError::UnknownResponse {
                status,
                detail1,
                detail2,
            } = &error
            {
                assert_ne!(status.get(), 0);
                assert_eq!((status.get(), *detail1, *detail2), wire);
            }
            assert_eq!(error.code(), wire);
        }
        _ => panic!("expected lossless fallback for {wire:?}"),
    }
}

#[test]
fn shared_status_constants_match_wire_baseline() {
    use vesper_objects::syscall_status::{
        ALREADY_MAPPED, ASID_POOL_EXHAUSTED, EMPTY_SLOT, INSUFFICIENT_MEMORY, INSUFFICIENT_RIGHTS,
        INVALID_DOMAIN, INVALID_FRAME_SIZE, INVALID_OBJECT_TYPE, INVALID_OPERATION,
        INVALID_POINTER, INVALID_SIZE, INVALID_SLOT, NO_ASID_ASSIGNED, NOT_ARCH_TYPE,
        NOT_CORE_TYPE, NOT_MAPPED, NULL_CAPABILITY, POOL_EXHAUSTED, SLOT_OCCUPIED, SUCCESS,
        TYPE_MISMATCH, UNKNOWN, UNKNOWN_ARCH_TYPE, UNKNOWN_CORE_TYPE, UNSUPPORTED_ARCH_TYPE,
        UNSUPPORTED_CORE_TYPE,
    };

    assert_eq!(
        [
            SUCCESS,
            UNKNOWN,
            NULL_CAPABILITY,
            INVALID_DOMAIN,
            INVALID_POINTER,
            INSUFFICIENT_RIGHTS,
            NOT_MAPPED,
            ALREADY_MAPPED,
            INVALID_OPERATION,
            ASID_POOL_EXHAUSTED,
            NO_ASID_ASSIGNED,
            INVALID_SLOT,
            EMPTY_SLOT,
            SLOT_OCCUPIED,
            NOT_CORE_TYPE,
            UNKNOWN_CORE_TYPE,
            UNSUPPORTED_CORE_TYPE,
            NOT_ARCH_TYPE,
            UNKNOWN_ARCH_TYPE,
            UNSUPPORTED_ARCH_TYPE,
            INVALID_OBJECT_TYPE,
            TYPE_MISMATCH,
            INSUFFICIENT_MEMORY,
            POOL_EXHAUSTED,
            INVALID_SIZE,
            INVALID_FRAME_SIZE,
        ],
        [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25
        ]
    );
}

#[test]
fn literal_error_baseline_encodes_and_decodes_real_variants() {
    assert_error!((1, 0, 0), CapError::Unknown, CapError::Unknown);
    assert_error!(
        (2, 0, 0),
        CapError::NullCapability,
        CapError::NullCapability
    );
    assert_error!((3, 0, 0), CapError::InvalidDomain, CapError::InvalidDomain);
    assert_error!(
        (4, 0, 0),
        CapError::InvalidPointer,
        CapError::InvalidPointer
    );
    assert_error!(
        (5, 0, 0),
        CapError::InsufficientRights,
        CapError::InsufficientRights
    );
    assert_error!((6, 0, 0), CapError::NotMapped, CapError::NotMapped);
    assert_error!((7, 0, 0), CapError::AlreadyMapped, CapError::AlreadyMapped);
    assert_error!(
        (8, 0, 0),
        CapError::InvalidOperation,
        CapError::InvalidOperation
    );
    assert_error!(
        (9, 0, 0),
        CapError::ASIDPoolExhausted,
        CapError::ASIDPoolExhausted
    );
    assert_error!(
        (10, 0, 0),
        CapError::NoASIDAssigned,
        CapError::NoASIDAssigned
    );
    assert_error!(
        (11, 42, 0),
        CapError::InvalidSlot(KeySlot(42)),
        CapError::InvalidSlot(KeySlot(42))
    );
    assert_error!(
        (12, 43, 0),
        CapError::EmptySlot(KeySlot(43)),
        CapError::EmptySlot(KeySlot(43))
    );
    assert_error!(
        (13, 44, 0),
        CapError::SlotOccupied(KeySlot(44)),
        CapError::SlotOccupied(KeySlot(44))
    );
    assert_error!((14, 0x82, 0), CapError::NotCoreType(ObjectType::VSPACE), CapError::NotCoreType(t) if t == ObjectType::VSPACE);
    assert_error!(
        (15, 126, 0),
        CapError::UnknownCoreType(126),
        CapError::UnknownCoreType(126)
    );
    assert_error!(
        (16, 7, 0),
        CapError::UnsupportedCoreType(CoreType::EventCount),
        CapError::UnsupportedCoreType(CoreType::EventCount)
    );
    assert_error!((17, 4, 0), CapError::NotArchType(ObjectType::TIME), CapError::NotArchType(t) if t == ObjectType::TIME);
    assert_error!(
        (18, 9, 0),
        CapError::UnknownArchType(9),
        CapError::UnknownArchType(9)
    );
    assert_error!(
        (19, 2, 0),
        CapError::UnsupportedArchType(ArchType::VSpace),
        CapError::UnsupportedArchType(ArchType::VSpace)
    );
    assert_error!((20, 0xff, 0), CapError::InvalidObjectType(ObjectType::from(0xff)), CapError::InvalidObjectType(t) if t.as_u8() == 0xff);
    assert_error!(
        (21, 7, 0x88),
        CapError::TypeMismatch {
            expected: ObjectType::EVENT_COUNT,
            found: ObjectType::IRQ_CONTROL,
        },
        CapError::TypeMismatch { expected, found }
            if expected == ObjectType::EVENT_COUNT && found == ObjectType::IRQ_CONTROL
    );
    assert_error!(
        (22, 0, 0),
        CapError::InsufficientMemory,
        CapError::InsufficientMemory
    );
    assert_error!((23, 0, 0), CapError::PoolExhausted, CapError::PoolExhausted);
    assert_error!(
        (24, 4096, 0),
        CapError::InvalidSize(4096),
        CapError::InvalidSize(4096)
    );
    assert_error!(
        (25, 8192, 0),
        CapError::InvalidFrameSize(8192),
        CapError::InvalidFrameSize(8192)
    );
}

#[test]
fn success_preserves_both_full_width_words() {
    for first in [0, 1, 1 << 63, u64::MAX] {
        for second in [0, 1, 1 << 63, u64::MAX] {
            assert_eq!(
                decode_syscall_result((0, first, second)).map_err(CapError::code),
                Ok((first, second))
            );
        }
    }
}

#[test]
fn unknown_statuses_and_high_bit_aliases_are_lossless() {
    for status in [26, 255, 256, 1 << 32, 1 << 63, u64::MAX] {
        assert_unknown_response((status, 0, 0));
        assert_unknown_response((status, u64::MAX, 0x1234_5678_9abc_def0));
    }
    for low in 0..=25 {
        for high in [1 << 8, 1 << 16, 1 << 32, 1 << 63] {
            assert_unknown_response((high | low, 0, 0));
        }
    }
}

#[test]
fn slot_details_are_checked_before_narrowing() {
    for raw in [0, 1, u32::MAX] {
        let detail = u64::from(raw);
        assert_error!((11, detail, 0), CapError::InvalidSlot(KeySlot(raw)), CapError::InvalidSlot(KeySlot(s)) if s == raw);
        assert_error!((12, detail, 0), CapError::EmptySlot(KeySlot(raw)), CapError::EmptySlot(KeySlot(s)) if s == raw);
        assert_error!((13, detail, 0), CapError::SlotOccupied(KeySlot(raw)), CapError::SlotOccupied(KeySlot(s)) if s == raw);
    }
    for status in 11..=13 {
        for detail in [1 << 32, (1 << 32) | 42, 1 << 63, u64::MAX] {
            assert_unknown_response((status, detail, 0));
        }
    }
}

#[test]
fn raw_type_details_preserve_every_byte_including_unknown_indices() {
    for raw in u8::MIN..=u8::MAX {
        let detail = u64::from(raw);
        let object = ObjectType::from(raw);
        assert_error!((14, detail, 0), CapError::NotCoreType(object), CapError::NotCoreType(t) if t == object);
        assert_error!((15, detail, 0), CapError::UnknownCoreType(raw), CapError::UnknownCoreType(t) if t == raw);
        assert_error!((17, detail, 0), CapError::NotArchType(object), CapError::NotArchType(t) if t == object);
        assert_error!((18, detail, 0), CapError::UnknownArchType(raw), CapError::UnknownArchType(t) if t == raw);
        assert_error!((20, detail, 0), CapError::InvalidObjectType(object), CapError::InvalidObjectType(t) if t == object);
        assert_error!(
            (21, detail, 0x82),
            CapError::TypeMismatch { expected: object, found: ObjectType::VSPACE },
            CapError::TypeMismatch { expected, found }
                if expected == object && found == ObjectType::VSPACE
        );
        assert_error!(
            (21, 7, detail),
            CapError::TypeMismatch { expected: ObjectType::EVENT_COUNT, found: object },
            CapError::TypeMismatch { expected, found }
                if expected == ObjectType::EVENT_COUNT && found == object
        );
    }
}

#[test]
fn unsupported_types_decode_only_known_local_indices() {
    let core_types = [
        (0, CoreType::Null),
        (1, CoreType::Untyped),
        (2, CoreType::Domain),
        (3, CoreType::KeyTable),
        (4, CoreType::Time),
        (5, CoreType::Endpoint),
        (6, CoreType::Notification),
        (7, CoreType::EventCount),
        (8, CoreType::Buffer),
        (9, CoreType::Reply),
        (127, CoreType::DebugConsole),
    ];
    let arch_types = [
        (0, ArchType::Frame),
        (1, ArchType::PageTable),
        (2, ArchType::VSpace),
        (3, ArchType::ASIDPool),
        (4, ArchType::ASID),
        (5, ArchType::IOSpace),
        (6, ArchType::IOPort),
        (7, ArchType::IRQHandler),
        (8, ArchType::IRQControl),
    ];
    for detail in 0..=255 {
        if let Some((_, kind)) = core_types.iter().find(|entry| entry.0 == detail) {
            assert_error!((16, detail, 0), CapError::UnsupportedCoreType(*kind), CapError::UnsupportedCoreType(t) if t == *kind);
        } else {
            assert_unknown_response((16, detail, 0));
        }
        if let Some((_, kind)) = arch_types.iter().find(|entry| entry.0 == detail) {
            assert_error!((19, detail, 0), CapError::UnsupportedArchType(*kind), CapError::UnsupportedArchType(t) if t == *kind);
        } else {
            // Full architecture wire IDs (0x80..=0x88) are not local indices.
            assert_unknown_response((19, detail, 0));
        }
    }
}

#[test]
fn oversized_type_details_preserve_both_words() {
    for detail in [256, 257, 1 << 32, 1 << 63, u64::MAX] {
        for status in 14..=20 {
            assert_unknown_response((status, detail, 0));
        }
        assert_unknown_response((21, detail, 0x82));
        assert_unknown_response((21, 7, detail));
        assert_unknown_response((21, detail, detail));
    }
}

#[test]
fn nonzero_unused_details_are_not_silently_discarded() {
    for extra in [1, 1 << 63, u64::MAX] {
        for status in 1..=25 {
            if status != 21 {
                assert_unknown_response((status, 0, extra));
            }
        }
        for status in [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 22, 23] {
            assert_unknown_response((status, extra, 0));
            assert_unknown_response((status, extra, extra));
        }
    }
}

#[test]
fn size_details_use_checked_native_width() {
    for detail in [0, 1, u64::from(u32::MAX), 1 << 32, 1 << 63, u64::MAX] {
        if let Ok(size) = usize::try_from(detail) {
            assert_error!((24, detail, 0), CapError::InvalidSize(size), CapError::InvalidSize(s) if s == size);
            assert_error!((25, detail, 0), CapError::InvalidFrameSize(size), CapError::InvalidFrameSize(s) if s == size);
        } else {
            assert_unknown_response((24, detail, 0));
            assert_unknown_response((25, detail, 0));
        }
    }
}

use vesper_objects::{
    ArchType, CapError, CoreType, InvalidStackReason, KeySlot, ObjectType, RawKey,
    decode_syscall_result,
};

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
        INVALID_ADDRESS_SPACE, INVALID_FRAME_SIZE, INVALID_OBJECT_TYPE, INVALID_OPERATION,
        INVALID_POINTER, INVALID_SIZE, INVALID_SLOT, INVALID_THREAD, NO_ASID_ASSIGNED,
        NOT_ARCH_TYPE, NOT_CORE_TYPE, NOT_MAPPED, NULL_CAPABILITY, POOL_EXHAUSTED, SLOT_OCCUPIED,
        SUCCESS, TYPE_MISMATCH, UNKNOWN, UNKNOWN_ARCH_TYPE, UNKNOWN_CORE_TYPE,
        UNSUPPORTED_ARCH_TYPE, UNSUPPORTED_CORE_TYPE,
    };

    assert_eq!(
        [
            SUCCESS,
            UNKNOWN,
            NULL_CAPABILITY,
            INVALID_THREAD,
            INVALID_ADDRESS_SPACE,
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
            24, 25, 26
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
    assert_error!((3, 0, 0), CapError::InvalidThread, CapError::InvalidThread);
    assert_error!(
        (4, 0, 0),
        CapError::InvalidAddressSpace,
        CapError::InvalidAddressSpace
    );
    assert_error!(
        (5, 0, 0),
        CapError::InvalidPointer,
        CapError::InvalidPointer
    );
    assert_error!(
        (6, 0, 0),
        CapError::InsufficientRights,
        CapError::InsufficientRights
    );
    assert_error!((7, 0, 0), CapError::NotMapped, CapError::NotMapped);
    assert_error!((8, 0, 0), CapError::AlreadyMapped, CapError::AlreadyMapped);
    assert_error!(
        (9, 0, 0),
        CapError::InvalidOperation,
        CapError::InvalidOperation
    );
    assert_error!(
        (10, 0, 0),
        CapError::ASIDPoolExhausted,
        CapError::ASIDPoolExhausted
    );
    assert_error!(
        (11, 0, 0),
        CapError::NoASIDAssigned,
        CapError::NoASIDAssigned
    );
    assert_error!(
        (12, 42, 0),
        CapError::InvalidSlot(KeySlot(42)),
        CapError::InvalidSlot(KeySlot(42))
    );
    assert_error!(
        (13, 43, 0),
        CapError::EmptySlot(KeySlot(43)),
        CapError::EmptySlot(KeySlot(43))
    );
    assert_error!(
        (14, 44, 0),
        CapError::SlotOccupied(KeySlot(44)),
        CapError::SlotOccupied(KeySlot(44))
    );
    assert_error!((15, 0x82, 0), CapError::NotCoreType(ObjectType::ADDRESS_SPACE), CapError::NotCoreType(t) if t == ObjectType::ADDRESS_SPACE);
    assert_error!(
        (16, 126, 0),
        CapError::UnknownCoreType(126),
        CapError::UnknownCoreType(126)
    );
    assert_error!(
        (17, 8, 0),
        CapError::UnsupportedCoreType(CoreType::EventCount),
        CapError::UnsupportedCoreType(CoreType::EventCount)
    );
    assert_error!((18, 4, 0), CapError::NotArchType(ObjectType::TIME), CapError::NotArchType(t) if t == ObjectType::TIME);
    assert_error!(
        (19, 7, 0),
        CapError::UnknownArchType(7),
        CapError::UnknownArchType(7)
    );
    assert_error!(
        (20, 2, 0),
        CapError::UnsupportedArchType(ArchType::AddressSpace),
        CapError::UnsupportedArchType(ArchType::AddressSpace)
    );
    assert_error!((21, 0xff, 0), CapError::InvalidObjectType(ObjectType::from(0xff)), CapError::InvalidObjectType(t) if t.as_u8() == 0xff);
    assert_error!(
        (22, 8, 0x86),
        CapError::TypeMismatch {
            expected: ObjectType::EVENT_COUNT,
            found: ObjectType::IO_PORT,
        },
        CapError::TypeMismatch { expected, found }
            if expected == ObjectType::EVENT_COUNT && found == ObjectType::IO_PORT
    );
    assert_error!(
        (23, 0, 0),
        CapError::InsufficientMemory,
        CapError::InsufficientMemory
    );
    assert_error!((24, 0, 0), CapError::PoolExhausted, CapError::PoolExhausted);
    assert_error!(
        (25, 4096, 0),
        CapError::InvalidSize(4096),
        CapError::InvalidSize(4096)
    );
    assert_error!(
        (26, 8192, 0),
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
    for status in [27, 255, 256, 1 << 32, 1 << 63, u64::MAX] {
        assert_unknown_response((status, 0, 0));
        assert_unknown_response((status, u64::MAX, 0x1234_5678_9abc_def0));
    }
    for low in 0..=26 {
        for high in [1 << 8, 1 << 16, 1 << 32, 1 << 63] {
            assert_unknown_response((high | low, 0, 0));
        }
    }
}

#[test]
fn slot_details_are_checked_before_narrowing() {
    for raw in [0, 1, u32::MAX] {
        let detail = u64::from(raw);
        assert_error!((12, detail, 0), CapError::InvalidSlot(KeySlot(raw)), CapError::InvalidSlot(KeySlot(s)) if s == raw);
        assert_error!((13, detail, 0), CapError::EmptySlot(KeySlot(raw)), CapError::EmptySlot(KeySlot(s)) if s == raw);
        assert_error!((14, detail, 0), CapError::SlotOccupied(KeySlot(raw)), CapError::SlotOccupied(KeySlot(s)) if s == raw);
    }
    for status in 12..=14 {
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
        assert_error!((15, detail, 0), CapError::NotCoreType(object), CapError::NotCoreType(t) if t == object);
        assert_error!((16, detail, 0), CapError::UnknownCoreType(raw), CapError::UnknownCoreType(t) if t == raw);
        assert_error!((18, detail, 0), CapError::NotArchType(object), CapError::NotArchType(t) if t == object);
        assert_error!((19, detail, 0), CapError::UnknownArchType(raw), CapError::UnknownArchType(t) if t == raw);
        assert_error!((21, detail, 0), CapError::InvalidObjectType(object), CapError::InvalidObjectType(t) if t == object);
        assert_error!(
            (22, detail, 0x82),
            CapError::TypeMismatch { expected: object, found: ObjectType::ADDRESS_SPACE },
            CapError::TypeMismatch { expected, found }
                if expected == object && found == ObjectType::ADDRESS_SPACE
        );
        assert_error!(
            (22, 8, detail),
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
        (2, CoreType::KeyTable),
        (3, CoreType::Thread),
        (4, CoreType::Time),
        (5, CoreType::Scheduler),
        (6, CoreType::Invocation),
        (7, CoreType::Notification),
        (8, CoreType::EventCount),
        (9, CoreType::WaitSet),
        (10, CoreType::IRQControl),
        (11, CoreType::IRQHandler),
        (127, CoreType::DebugConsole),
    ];
    let arch_types = [
        (0, ArchType::Frame),
        (1, ArchType::PageTable),
        (2, ArchType::AddressSpace),
        (3, ArchType::ASIDPool),
        (4, ArchType::ASIDControl),
        (5, ArchType::IOSpace),
        (6, ArchType::IOPort),
    ];
    for detail in 0..=255 {
        if let Some((_, kind)) = core_types.iter().find(|entry| entry.0 == detail) {
            assert_error!((17, detail, 0), CapError::UnsupportedCoreType(*kind), CapError::UnsupportedCoreType(t) if t == *kind);
        } else {
            assert_unknown_response((17, detail, 0));
        }
        if let Some((_, kind)) = arch_types.iter().find(|entry| entry.0 == detail) {
            assert_error!((20, detail, 0), CapError::UnsupportedArchType(*kind), CapError::UnsupportedArchType(t) if t == *kind);
        } else {
            // Full architecture wire IDs (0x80..=0x86) are not local indices.
            assert_unknown_response((20, detail, 0));
        }
    }
}

#[test]
fn oversized_type_details_preserve_both_words() {
    for detail in [256, 257, 1 << 32, 1 << 63, u64::MAX] {
        for status in 15..=21 {
            assert_unknown_response((status, detail, 0));
        }
        assert_unknown_response((22, detail, 0x82));
        assert_unknown_response((22, 9, detail));
        assert_unknown_response((22, detail, detail));
    }
}

#[test]
fn nonzero_unused_details_are_not_silently_discarded() {
    for extra in [1, 1 << 63, u64::MAX] {
        for status in 1..=26 {
            if status != 22 {
                assert_unknown_response((status, 0, extra));
            }
        }
        for status in [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 23, 24] {
            assert_unknown_response((status, extra, 0));
            assert_unknown_response((status, extra, extra));
        }
    }
}

#[test]
fn size_details_use_checked_native_width() {
    for detail in [0, 1, u64::from(u32::MAX), 1 << 32, 1 << 63, u64::MAX] {
        if let Ok(size) = usize::try_from(detail) {
            assert_error!((25, detail, 0), CapError::InvalidSize(size), CapError::InvalidSize(s) if s == size);
            assert_error!((26, detail, 0), CapError::InvalidFrameSize(size), CapError::InvalidFrameSize(s) if s == size);
        } else {
            assert_unknown_response((25, detail, 0));
            assert_unknown_response((26, detail, 0));
        }
    }
}

#[test]
fn missing_intermediate_pins_status_vaddr_and_lossless_fallback() {
    use vesper_objects::syscall_status::MISSING_INTERMEDIATE;

    assert_eq!(MISSING_INTERMEDIATE, 30);
    for vaddr in [0, 1, 0x1000_0000, 1 << 47, 1 << 63, u64::MAX] {
        let wire = (30, vaddr, 0);
        assert_error!(
            wire,
            CapError::MissingIntermediate { vaddr },
            CapError::MissingIntermediate { vaddr: v } if v == vaddr
        );
    }
    // A nonzero second detail word is not silently discarded.
    for extra in [1, 1 << 63, u64::MAX] {
        assert_unknown_response((30, 0, extra));
        assert_unknown_response((30, extra, extra));
    }
}

#[test]
fn physical_alias_pins_status_paddr_and_lossless_fallback() {
    use vesper_objects::syscall_status::PHYSICAL_ALIAS;

    assert_eq!(PHYSICAL_ALIAS, 31);
    for paddr in [0, 0x1000, 0x1000_0000, 1 << 47, 1 << 63, u64::MAX] {
        let wire = (31, paddr, 0);
        assert_error!(
            wire,
            CapError::PhysicalAlias { paddr },
            CapError::PhysicalAlias { paddr: p } if p == paddr
        );
    }
    // A nonzero second detail word is not silently discarded.
    for extra in [1, 1 << 63, u64::MAX] {
        assert_unknown_response((31, 0, extra));
        assert_unknown_response((31, extra, extra));
    }
}

#[test]
fn counter_overflow_pins_status_and_lossless_fallback() {
    use vesper_objects::syscall_status::COUNTER_OVERFLOW;

    assert_eq!(COUNTER_OVERFLOW, 32);
    // The overflow error carries zero details.
    assert_error!(
        (32, 0, 0),
        CapError::CounterOverflow,
        CapError::CounterOverflow
    );
    // Nonzero detail words are not silently discarded.
    for extra in [1, 1 << 63, u64::MAX] {
        assert_unknown_response((32, extra, 0));
        assert_unknown_response((32, 0, extra));
        assert_unknown_response((32, extra, extra));
    }
}

const INVALID_STACK_REASONS: [(u64, InvalidStackReason); 12] = [
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
];

#[test]
fn invalid_stack_status_and_reason_ids_match_literal_wire_contract() {
    assert_eq!(vesper_objects::syscall_status::INVALID_STACK, 33);
    for (id, reason) in INVALID_STACK_REASONS {
        assert_eq!(u64::from(reason as u8), id);
        assert_eq!(InvalidStackReason::try_from(id), Ok(reason));
    }
}

#[test]
fn invalid_stack_encodes_and_decodes_every_reason_and_full_width_value() {
    for (id, reason) in INVALID_STACK_REASONS {
        for value in [0, 1, 0x1234_5678_9abc_def0, 1 << 63, u64::MAX] {
            let wire = (33, value, id);
            assert_error!(
                wire,
                CapError::InvalidStack { value, reason },
                CapError::InvalidStack { value: actual_value, reason: actual_reason }
                    if actual_value == value && actual_reason == reason
            );
            assert_eq!(decode_syscall_result(wire).err().unwrap().code(), wire);
        }
    }
}

#[test]
fn invalid_stack_rejects_zero_and_every_unassigned_reason_byte_losslessly() {
    for id in 0..=u64::from(u8::MAX) {
        if !(1..=12).contains(&id) {
            assert_eq!(InvalidStackReason::try_from(id), Err(()));
            for value in [0, 0x1234_5678_9abc_def0, u64::MAX] {
                assert_unknown_response((33, value, id));
            }
        }
    }
}

#[test]
fn invalid_stack_reasons_reject_full_width_extensions_without_operand_packing() {
    for bit in 4..64 {
        for low in 0..=12 {
            let reason = (1_u64 << bit) | low;
            assert_eq!(InvalidStackReason::try_from(reason), Err(()));
            assert_unknown_response((33, 0xfedc_ba98_7654_3210, reason));
        }
    }
    for reason in [256, 257, 0x0801, 0x0100_0001, u64::MAX] {
        assert_eq!(InvalidStackReason::try_from(reason), Err(()));
        assert_unknown_response((33, u64::MAX, reason));
    }
}

#[test]
fn nesting_depth_status_is_lossless_and_rejects_nonzero_detail_two() {
    use vesper_objects::syscall_status::NESTING_DEPTH;

    assert_eq!(NESTING_DEPTH, 34);
    for count in [0, 1, 15, 16, u64::MAX] {
        let wire = (34, count, 0);
        assert_error!(
            wire,
            CapError::NestingDepth { count },
            CapError::NestingDepth { count: actual } if actual == count
        );
    }
    for detail in [1, 16, u64::MAX] {
        assert_unknown_response((34, 16, detail));
    }
}

#[test]
fn invalid_stack_status_extensions_and_unassigned_statuses_remain_unknown() {
    for bit in 6..64 {
        assert_unknown_response(((1_u64 << bit) | 33, u64::MAX, 1));
    }
    for wire in [(46, 0, 0), (46, 0x1234_5678_9abc_def0, u64::MAX)] {
        assert_unknown_response(wire);
    }
}

#[test]
fn unexpected_return_status_is_lossless_for_arbitrary_words() {
    use vesper_objects::syscall_status::UNEXPECTED_RETURN;

    assert_eq!(UNEXPECTED_RETURN, 35);
    for (word1, word2) in [
        (0, 0),
        (1, u64::MAX),
        (0x1234_5678_9abc_def0, 0x0fed_cba9_8765_4321),
        (u64::MAX, u64::MAX),
    ] {
        assert_error!(
            (35, word1, word2),
            CapError::UnexpectedReturn { word1, word2 },
            CapError::UnexpectedReturn { word1: first, word2: second }
                if first == word1 && second == word2
        );
    }
    // High-bit aliases of 35 are not this status.
    for bit in 6..64 {
        assert_unknown_response(((1_u64 << bit) | 35, 1, 2));
    }
}

#[test]
fn time_budget_statuses_round_trip_and_reject_unused_details() {
    use vesper_objects::syscall_status::INSUFFICIENT_BUDGET;

    assert_eq!(INSUFFICIENT_BUDGET, 36);
    for remaining in [0, 1, 0x1234_5678_9abc_def0, u64::MAX - 1] {
        assert_error!(
            (36, remaining, 0),
            CapError::InsufficientBudget { remaining },
            CapError::InsufficientBudget { remaining: actual } if actual == remaining
        );
        assert_unknown_response((36, remaining, 1));
    }
}

#[test]
fn irq_line_active_round_trips_the_line() {
    use vesper_objects::syscall_status::IRQ_LINE_ACTIVE;

    assert_eq!(IRQ_LINE_ACTIVE, 37);
    for line in [0, 1, 30, 1019, u32::MAX] {
        assert_error!(
            (37, u64::from(line), 0),
            CapError::IrqLineActive { line },
            CapError::IrqLineActive { line: actual } if actual == line
        );
        assert_unknown_response((37, u64::from(line), 1));
    }
    // A line wider than 32 bits is not this status.
    assert_unknown_response((37, 1 << 32, 0));
}

#[test]
fn call_with_key_statuses_round_trip_with_zero_details() {
    use vesper_objects::syscall_status::{KEY_NOT_ACCEPTED, NO_PLACEMENT_SLOT};

    assert_eq!(KEY_NOT_ACCEPTED, 38);
    assert_eq!(NO_PLACEMENT_SLOT, 39);
    assert_error!(
        (38, 0, 0),
        CapError::KeyNotAccepted,
        CapError::KeyNotAccepted
    );
    assert_error!(
        (39, 0, 0),
        CapError::NoPlacementSlot,
        CapError::NoPlacementSlot
    );
    for status in [38, 39] {
        for (detail1, detail2) in [(1, 0), (0, 1), (u64::MAX, u64::MAX)] {
            assert_unknown_response((status, detail1, detail2));
        }
    }
}

#[test]
fn not_bound_status_round_trips_with_zero_details() {
    use vesper_objects::syscall_status::NOT_BOUND;

    assert_eq!(NOT_BOUND, 40);
    assert_error!((40, 0, 0), CapError::NotBound, CapError::NotBound);
    for (detail1, detail2) in [(1, 0), (0, 1), (u64::MAX, u64::MAX)] {
        assert_unknown_response((40, detail1, detail2));
    }
}

#[test]
fn invalid_self_reference_names_the_slot_and_the_key() {
    use vesper_objects::syscall_status::{INVALID_SELF_REFERENCE, InvalidSelfReference};

    assert_eq!(INVALID_SELF_REFERENCE, 41);
    for key in [0, 1, 0x1234_5678_9abc_def0, u64::MAX] {
        assert_error!(
            (41, 4, key),
            CapError::InvalidSelfReference {
                reference: InvalidSelfReference::KeyTableKey(KeySlot(4)),
                key: RawKey::from_wire(key),
            },
            CapError::InvalidSelfReference {
                reference: InvalidSelfReference::KeyTableKey(KeySlot(4)),
                key: actual,
            } if actual.to_wire() == key
        );
    }
    // Only well-known self slots are self references.
    for slot in [0, 1, 2, 3, 5, u64::MAX] {
        assert_unknown_response((41, slot, 0));
    }
}

#[test]
fn stale_generation_status_round_trips_the_current_generation() {
    use vesper_objects::syscall_status::STALE_GENERATION;

    assert_eq!(STALE_GENERATION, 42);
    for current in [0, 1, 0x1234_5678_9abc_def0, u64::MAX] {
        assert_error!(
            (42, current, 0),
            CapError::StaleGeneration { current },
            CapError::StaleGeneration { current: actual } if actual == current
        );
        assert_unknown_response((42, current, 1));
    }
}

#[test]
fn timed_out_status_round_trips_the_value() {
    use vesper_objects::syscall_status::TIMED_OUT;

    assert_eq!(TIMED_OUT, 43);
    for value in [0, 1, 0x1234_5678_9abc_def0, u64::MAX] {
        assert_error!(
            (43, value, 0),
            CapError::TimedOut { value },
            CapError::TimedOut { value: actual } if actual == value
        );
        assert_unknown_response((43, value, 1));
    }
}

#[test]
fn watched_status_has_no_details() {
    use vesper_objects::syscall_status::WATCHED;

    assert_eq!(WATCHED, 44);
    assert_error!((44, 0, 0), CapError::Watched, CapError::Watched);
    assert_eq!(CapError::Watched.code(), (44, 0, 0));
    assert_unknown_response((44, 1, 0));
    assert_unknown_response((44, 0, 1));
}

#[test]
fn member_retired_status_round_trips_the_slot() {
    use vesper_objects::syscall_status::MEMBER_RETIRED;

    assert_eq!(MEMBER_RETIRED, 45);
    for slot in [0, 7, u64::MAX] {
        assert_error!(
            (45, slot, 0),
            CapError::MemberRetired { slot },
            CapError::MemberRetired { slot: actual } if actual == slot
        );
        assert_unknown_response((45, slot, 1));
    }
}

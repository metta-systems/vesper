use vesper_objects::{
    CapError, Key, KeySlot, KeyTableKey, ObjectType, RawKey, Rights, decode_syscall_result,
};

#[cfg(test)]
#[path = "support/cap_error.rs"]
mod cap_error;

#[cfg(test)]
#[path = "support/key_identity.rs"]
mod key_identity;

// Compile the actual wrapper methods with a test-only transport. DCB accessors
// remain uncalled: constructing a test handle does not establish a user mapping.
#[cfg(test)]
#[path = "../src/asid_pool.rs"]
pub mod asid_pool_client;

#[cfg(test)]
#[path = "../src/domain.rs"]
pub mod domain_client;

#[cfg(test)]
#[path = "../src/frame.rs"]
pub mod frame_client;

#[cfg(test)]
#[path = "../src/key_table.rs"]
pub mod key_table_client;

#[cfg(test)]
#[path = "../src/notification.rs"]
pub mod notification_client;

#[cfg(test)]
#[path = "../src/page_table.rs"]
pub mod page_table_client;

#[cfg(test)]
#[path = "../src/untyped.rs"]
pub mod untyped_client;

#[cfg(test)]
mod tests {
    use core::mem::{align_of, size_of};

    use vesper_objects::{ArchType, CapError, CoreType, ObjectType};

    #[test]
    fn untyped_operation_matches_existing_wire_id() {
        use vesper_objects::untyped::UntypedOp;

        assert_eq!(UntypedOp::Retype as u8, 0);
        assert_eq!(UntypedOp::Retype as u32, 0);
        assert!(matches!(UntypedOp::try_from(0_u32), Ok(UntypedOp::Retype)));
        assert!(matches!(UntypedOp::try_from(0_u64), Ok(UntypedOp::Retype)));
        assert_eq!(size_of::<UntypedOp>(), 1);
        assert_eq!(align_of::<UntypedOp>(), 1);
    }

    #[test]
    fn untyped_operation_rejects_every_unassigned_value() {
        use vesper_objects::untyped::UntypedOp;

        for raw in 1_u32..=255 {
            assert!(matches!(
                UntypedOp::try_from(raw),
                Err(CapError::InvalidOperation)
            ));
            assert!(matches!(
                UntypedOp::try_from(u64::from(raw)),
                Err(CapError::InvalidOperation)
            ));
        }
        for raw in [256_u64, 1 << 16, u64::from(u32::MAX), u64::MAX] {
            assert!(matches!(
                UntypedOp::try_from(raw),
                Err(CapError::InvalidOperation)
            ));
        }
    }

    #[test]
    fn untyped_operation_rejects_high_bit_aliases_before_narrowing() {
        use vesper_objects::untyped::UntypedOp;

        for bit in 8..64 {
            assert!(matches!(
                UntypedOp::try_from(1_u64 << bit),
                Err(CapError::InvalidOperation)
            ));
        }
    }

    #[test]
    fn key_table_operations_match_existing_wire_ids() {
        use vesper_objects::key_table::KeyTableOp;

        for (op, raw) in [
            (KeyTableOp::CopyDerive, 0_u32),
            (KeyTableOp::Move, 1),
            (KeyTableOp::Delete, 2),
            (KeyTableOp::Revoke, 4),
        ] {
            assert_eq!(op as u32, raw);
            assert_eq!(KeyTableOp::try_from(raw).map_err(CapError::code), Ok(op));
            assert_eq!(
                KeyTableOp::try_from(u64::from(raw)).map_err(CapError::code),
                Ok(op)
            );
        }
        assert_eq!(size_of::<KeyTableOp>(), 1);
        assert_eq!(align_of::<KeyTableOp>(), 1);
    }

    #[test]
    fn key_table_operations_reject_every_unassigned_byte() {
        use vesper_objects::key_table::KeyTableOp;

        for raw in 0_u32..=255 {
            if matches!(raw, 0 | 1 | 2 | 4) {
                continue;
            }
            assert!(matches!(
                KeyTableOp::try_from(raw),
                Err(CapError::InvalidOperation)
            ));
            assert!(matches!(
                KeyTableOp::try_from(u64::from(raw)),
                Err(CapError::InvalidOperation)
            ));
        }
    }

    #[test]
    fn key_table_operations_reject_high_bit_aliases_before_narrowing() {
        use vesper_objects::key_table::KeyTableOp;

        for low in [0_u64, 1, 2, 4] {
            for bit in 8..64 {
                let raw = low | (1_u64 << bit);
                assert!(matches!(
                    KeyTableOp::try_from(raw),
                    Err(CapError::InvalidOperation)
                ));
                if let Ok(raw) = u32::try_from(raw) {
                    assert!(matches!(
                        KeyTableOp::try_from(raw),
                        Err(CapError::InvalidOperation)
                    ));
                }
            }
        }
        for raw in [u64::from(u32::MAX), u64::MAX] {
            assert!(matches!(
                KeyTableOp::try_from(raw),
                Err(CapError::InvalidOperation)
            ));
        }
        assert!(matches!(
            KeyTableOp::try_from(u32::MAX),
            Err(CapError::InvalidOperation)
        ));
    }

    #[test]
    fn frame_operations_match_existing_wire_ids() {
        use vesper_objects::frame::FrameOp;

        assert_eq!(FrameOp::Map as u8, 0);
        assert_eq!(FrameOp::Map as u32, 0);
        assert_eq!(FrameOp::Unmap as u8, 1);
        assert_eq!(FrameOp::GetAddress as u8, 2);
        assert_eq!(FrameOp::Remap as u8, 3);
        for value in [4, 255, 256, 1 << 32, u64::MAX] {
            assert!(matches!(
                FrameOp::try_from(value),
                Err(CapError::InvalidOperation)
            ));
        }
    }

    #[test]
    fn page_table_operations_match_existing_wire_ids() {
        use vesper_objects::page_table::PageTableOp;

        assert_eq!(PageTableOp::Map as u8, 0);
        assert_eq!(PageTableOp::Map as u32, 0);
        assert_eq!(PageTableOp::Unmap as u8, 1);
        for value in [2, 255, 256, 1 << 32, u64::MAX] {
            assert!(matches!(
                PageTableOp::try_from(value),
                Err(CapError::InvalidOperation)
            ));
        }
    }

    #[test]
    fn asid_pool_operations_match_existing_wire_ids() {
        use vesper_objects::asid_pool::ASIDPoolOp;

        assert_eq!(ASIDPoolOp::Assign as u8, 0);
        assert_eq!(ASIDPoolOp::Assign as u32, 0);
        for value in [1, 255, 256, 1 << 32, u64::MAX] {
            assert!(matches!(
                ASIDPoolOp::try_from(value),
                Err(CapError::InvalidOperation)
            ));
        }
    }

    #[test]
    fn notification_operations_match_existing_wire_ids() {
        use vesper_objects::notification::NotificationOp;

        assert_eq!(NotificationOp::Signal as u8, 0);
        assert_eq!(NotificationOp::Wait as u8, 1);
        assert_eq!(NotificationOp::Poll as u8, 2);
        assert_eq!(size_of::<NotificationOp>(), 1);
        assert_eq!(align_of::<NotificationOp>(), 1);
        for id in [0_u32, 1, 2] {
            assert!(NotificationOp::try_from(id).is_ok());
            assert!(NotificationOp::try_from(u64::from(id)).is_ok());
        }
    }

    #[test]
    fn notification_operation_rejects_every_unassigned_value() {
        use vesper_objects::notification::NotificationOp;

        for raw in 3_u32..=255 {
            assert!(matches!(
                NotificationOp::try_from(raw),
                Err(CapError::InvalidOperation)
            ));
            assert!(matches!(
                NotificationOp::try_from(u64::from(raw)),
                Err(CapError::InvalidOperation)
            ));
        }
        for raw in [256_u64, 1 << 16, u64::from(u32::MAX), u64::MAX] {
            assert!(matches!(
                NotificationOp::try_from(raw),
                Err(CapError::InvalidOperation)
            ));
        }
    }

    #[test]
    fn core_and_arch_types_display_their_variant_names() {
        use std::fmt::Write as _;

        fn name(kind: impl core::fmt::Display) -> String {
            let mut out = String::new();
            write!(out, "{kind}").unwrap();
            out
        }

        for (kind, expected) in [
            (CoreType::Null, "Null"),
            (CoreType::Untyped, "Untyped"),
            (CoreType::Domain, "Domain"),
            (CoreType::KeyTable, "KeyTable"),
            (CoreType::Time, "Time"),
            (CoreType::Endpoint, "Endpoint"),
            (CoreType::Notification, "Notification"),
            (CoreType::EventCount, "EventCount"),
            (CoreType::Reply, "Reply"),
            (CoreType::DebugConsole, "DebugConsole"),
        ] {
            assert_eq!(name(kind), expected);
        }
        for (kind, expected) in [
            (ArchType::Frame, "Frame"),
            (ArchType::PageTable, "PageTable"),
            (ArchType::VSpace, "VSpace"),
            (ArchType::ASIDPool, "ASIDPool"),
            (ArchType::ASID, "ASID"),
            (ArchType::IOSpace, "IOSpace"),
            (ArchType::IOPort, "IOPort"),
            (ArchType::IRQHandler, "IRQHandler"),
            (ArchType::IRQControl, "IRQControl"),
        ] {
            assert_eq!(name(kind), expected);
        }
    }

    #[test]
    fn debug_console_operation_stays_defined_without_kernel_availability() {
        use vesper_objects::debug_console::DebugConsoleOp;

        assert_eq!(DebugConsoleOp::Write as u8, 0);
        assert!(matches!(
            DebugConsoleOp::try_from(0_u32),
            Ok(DebugConsoleOp::Write)
        ));
        assert!(matches!(
            DebugConsoleOp::try_from(0_u64),
            Ok(DebugConsoleOp::Write)
        ));
        for raw in [1_u32, 127, 255, 256, 1 << 16, u32::MAX] {
            assert!(matches!(
                DebugConsoleOp::try_from(raw),
                Err(CapError::InvalidOperation)
            ));
        }
    }

    #[test]
    fn debug_console_operation_rejects_all_full_width_aliases() {
        use vesper_objects::debug_console::DebugConsoleOp;

        for bit in 0..64 {
            assert!(matches!(
                DebugConsoleOp::try_from(1_u64 << bit),
                Err(CapError::InvalidOperation)
            ));
        }
        for raw in [u64::from(u32::MAX), u64::MAX] {
            assert!(matches!(
                DebugConsoleOp::try_from(raw),
                Err(CapError::InvalidOperation)
            ));
        }
    }

    #[cfg(feature = "debug_kernel")]
    #[test]
    fn debug_console_client_is_available_for_debug_kernels() {
        use vesper_objects::{DebugConsoleKey, KeySlot, RawKey};

        // Construct handles only: host tests must not execute the SVC transport.
        // Model two issued keys, including a later occupant of the same slot.
        const ISSUED: RawKey = RawKey::new(KeySlot(127), 0x1234_5678);
        const REISSUED: RawKey = RawKey::new(KeySlot(127), 0x8765_4321);
        const CONSOLE: DebugConsoleKey = DebugConsoleKey::from_key(ISSUED);
        const OTHER: DebugConsoleKey = DebugConsoleKey::from_key(REISSUED);
        assert_eq!(CONSOLE.raw().to_wire(), 0x1234_5678_0000_007f);
        assert_eq!(OTHER.raw().to_wire(), 0x8765_4321_0000_007f);
    }

    // Literal ABI oracles: do not derive these IDs from the production catalogue.
    const CORE_TYPES: [(CoreType, ObjectType, u8); 10] = [
        (CoreType::Null, ObjectType::NULL, 0),
        (CoreType::Untyped, ObjectType::UNTYPED, 1),
        (CoreType::Domain, ObjectType::DOMAIN, 2),
        (CoreType::KeyTable, ObjectType::KEY_TABLE, 3),
        (CoreType::Time, ObjectType::TIME, 4),
        (CoreType::Endpoint, ObjectType::ENDPOINT, 5),
        (CoreType::Notification, ObjectType::NOTIFICATION, 6),
        (CoreType::EventCount, ObjectType::EVENT_COUNT, 7),
        (CoreType::Reply, ObjectType::REPLY, 8),
        (CoreType::DebugConsole, ObjectType::DEBUG_CONSOLE, 127),
    ];

    // Architecture entries carry both the category-local index and the wire ID.
    const ARCH_TYPES: [(ArchType, ObjectType, u8, u8); 9] = [
        (ArchType::Frame, ObjectType::FRAME, 0, 0x80),
        (ArchType::PageTable, ObjectType::PAGE_TABLE, 1, 0x81),
        (ArchType::VSpace, ObjectType::VSPACE, 2, 0x82),
        (ArchType::ASIDPool, ObjectType::ASID_POOL, 3, 0x83),
        (ArchType::ASID, ObjectType::ASID, 4, 0x84),
        (ArchType::IOSpace, ObjectType::IO_SPACE, 5, 0x85),
        (ArchType::IOPort, ObjectType::IO_PORT, 6, 0x86),
        (ArchType::IRQHandler, ObjectType::IRQ_HANDLER, 7, 0x87),
        (ArchType::IRQControl, ObjectType::IRQ_CONTROL, 8, 0x88),
    ];

    #[test]
    fn core_catalogue_matches_wire_abi() {
        for (kind, alias, wire) in CORE_TYPES {
            let object = ObjectType::from(wire);
            assert_eq!(kind.as_u8(), wire, "{kind:?}");
            assert_eq!(alias.as_u8(), wire, "{kind:?}");
            assert_eq!(alias, object, "{kind:?}");
            assert_eq!(ObjectType::from_core(kind), object, "{kind:?}");
            assert_eq!(ObjectType::from(kind), object, "{kind:?}");
            assert_eq!(CoreType::try_from(wire).map_err(CapError::code), Ok(kind));
            assert_eq!(CoreType::try_from(alias).map_err(CapError::code), Ok(kind));
        }
    }

    #[test]
    fn arch_catalogue_matches_wire_abi() {
        for (kind, alias, index, wire) in ARCH_TYPES {
            let object = ObjectType::from(wire);
            assert_eq!(kind.as_u8(), index, "{kind:?}");
            assert_eq!(alias.as_u8(), wire, "{kind:?}");
            assert_eq!(alias, object, "{kind:?}");
            assert_eq!(ObjectType::from_arch(kind), object, "{kind:?}");
            assert_eq!(ObjectType::from(kind), object, "{kind:?}");
            assert_eq!(ArchType::try_from(index).map_err(CapError::code), Ok(kind));
            assert_eq!(ArchType::try_from(alias).map_err(CapError::code), Ok(kind));
        }
    }

    #[test]
    fn every_raw_byte_preserves_its_wire_id_and_category() {
        assert_eq!(ObjectType::ARCH_BIT, 0x80);
        for raw in u8::MIN..=u8::MAX {
            let object = ObjectType::from(raw);
            assert_eq!(object.as_u8(), raw, "raw {raw:#04x}");
            assert_eq!(object.index(), raw & 0x7f, "raw {raw:#04x}");
            assert_eq!(object.is_core(), raw < 0x80, "raw {raw:#04x}");
            assert_eq!(object.is_arch(), raw >= 0x80, "raw {raw:#04x}");
        }
    }

    #[test]
    fn every_object_conversion_checks_category_before_reserved_index() {
        for raw in u8::MIN..=u8::MAX {
            let object = ObjectType::from(raw);
            let expected_core = if raw < 0x80 {
                CORE_TYPES
                    .iter()
                    .find(|entry| entry.2 == raw)
                    .map(|entry| entry.0)
                    .ok_or((15, u64::from(raw), 0))
            } else {
                Err((14, u64::from(raw), 0))
            };
            let expected_arch = if raw >= 0x80 {
                ARCH_TYPES
                    .iter()
                    .find(|entry| entry.3 == raw)
                    .map(|entry| entry.0)
                    .ok_or((18, u64::from(raw & 0x7f), 0))
            } else {
                Err((17, u64::from(raw), 0))
            };

            assert_eq!(
                CoreType::try_from(object).map_err(CapError::code),
                expected_core,
                "core conversion of {raw:#04x}"
            );
            assert_eq!(
                ArchType::try_from(object).map_err(CapError::code),
                expected_arch,
                "arch conversion of {raw:#04x}"
            );
        }
    }

    #[test]
    fn every_local_index_conversion_rejects_reserved_and_high_bit_values() {
        for raw in u8::MIN..=u8::MAX {
            let expected_core = CORE_TYPES
                .iter()
                .find(|entry| entry.2 == raw)
                .map(|entry| entry.0)
                .ok_or((15, u64::from(raw), 0));
            let expected_arch = ARCH_TYPES
                .iter()
                .find(|entry| entry.2 == raw)
                .map(|entry| entry.0)
                .ok_or((18, u64::from(raw), 0));

            assert_eq!(
                CoreType::try_from(raw).map_err(CapError::code),
                expected_core,
                "core local index {raw:#04x}"
            );
            assert_eq!(
                ArchType::try_from(raw).map_err(CapError::code),
                expected_arch,
                "arch local index {raw:#04x}"
            );
        }
    }

    #[test]
    fn object_types_have_one_byte_size_and_alignment() {
        assert_eq!(size_of::<ObjectType>(), 1);
        assert_eq!(align_of::<ObjectType>(), 1);
        assert_eq!(size_of::<CoreType>(), 1);
        assert_eq!(align_of::<CoreType>(), 1);
        assert_eq!(size_of::<ArchType>(), 1);
        assert_eq!(align_of::<ArchType>(), 1);
    }

    #[test]
    fn constructors_and_accessors_work_in_const_contexts() {
        const CORE: ObjectType = ObjectType::from_core(CoreType::Time);
        const ARCH: ObjectType = ObjectType::from_arch(ArchType::IRQControl);
        const CORE_WIRE: u8 = CORE.as_u8();
        const ARCH_WIRE: u8 = ARCH.as_u8();
        const CORE_INDEX: u8 = CORE.index();
        const ARCH_INDEX: u8 = ARCH.index();
        const CORE_CATEGORY: (bool, bool) = (CORE.is_core(), CORE.is_arch());
        const ARCH_CATEGORY: (bool, bool) = (ARCH.is_core(), ARCH.is_arch());
        const CORE_LOCAL: u8 = CoreType::Time.as_u8();
        const ARCH_LOCAL: u8 = ArchType::IRQControl.as_u8();

        assert_eq!(CORE, ObjectType::TIME);
        assert_eq!(ARCH, ObjectType::IRQ_CONTROL);
        assert_eq!((CORE_WIRE, CORE_INDEX, CORE_LOCAL), (4, 4, 4));
        assert_eq!((ARCH_WIRE, ARCH_INDEX, ARCH_LOCAL), (0x88, 8, 8));
        assert_eq!(CORE_CATEGORY, (true, false));
        assert_eq!(ARCH_CATEGORY, (false, true));
    }

    #[test]
    fn cap_error_raw_type_payloads_preserve_all_bits() {
        for raw in u8::MIN..=u8::MAX {
            let object = ObjectType::from(raw);
            let payload = u64::from(raw);
            assert_eq!(CapError::NotCoreType(object).code(), (14, payload, 0));
            assert_eq!(CapError::UnknownCoreType(raw).code(), (15, payload, 0));
            assert_eq!(CapError::NotArchType(object).code(), (17, payload, 0));
            assert_eq!(CapError::UnknownArchType(raw).code(), (18, payload, 0));
            assert_eq!(CapError::InvalidObjectType(object).code(), (20, payload, 0));
            assert_eq!(
                CapError::TypeMismatch {
                    expected: object,
                    found: ObjectType::FRAME,
                }
                .code(),
                (21, payload, 0x80)
            );
            assert_eq!(
                CapError::TypeMismatch {
                    expected: ObjectType::FRAME,
                    found: object,
                }
                .code(),
                (21, 0x80, payload)
            );
        }
    }

    #[test]
    fn cap_error_unsupported_types_use_local_indices() {
        for (kind, _, index) in CORE_TYPES {
            assert_eq!(
                CapError::UnsupportedCoreType(kind).code(),
                (16, u64::from(index), 0),
                "{kind:?}"
            );
        }
        for (kind, _, index, _) in ARCH_TYPES {
            assert_eq!(
                CapError::UnsupportedArchType(kind).code(),
                (19, u64::from(index), 0),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn cap_error_type_mismatch_uses_canonical_wire_ids_in_both_directions() {
        for (_, core, core_wire) in CORE_TYPES {
            for (_, arch, _, arch_wire) in ARCH_TYPES {
                assert_eq!(
                    CapError::TypeMismatch {
                        expected: core,
                        found: arch,
                    }
                    .code(),
                    (21, u64::from(core_wire), u64::from(arch_wire))
                );
                assert_eq!(
                    CapError::TypeMismatch {
                        expected: arch,
                        found: core,
                    }
                    .code(),
                    (21, u64::from(arch_wire), u64::from(core_wire))
                );
            }
        }
    }
}

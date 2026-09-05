use vesper_objects::{CapError, Key, KeySlot, Rights, decode_syscall_result};

#[cfg(test)]
#[path = "support/cap_error.rs"]
mod cap_error;

// Compile the actual wrapper methods with a test-only transport. DCB accessors
// remain uncalled: constructing a test handle does not establish a user mapping.
#[cfg(test)]
#[path = "../src/domain.rs"]
pub mod domain_client;

#[cfg(test)]
#[path = "../src/key_table.rs"]
pub mod key_table_client;

#[cfg(test)]
mod tests {
    use core::mem::{align_of, size_of};

    use vesper_objects::{ArchType, CapError, CoreType, ObjectType};

    #[test]
    fn debug_console_operation_stays_defined_without_kernel_availability() {
        use vesper_objects::debug_console::DebugConsoleOp;

        assert_eq!(DebugConsoleOp::Write as u8, 0);
        assert!(matches!(
            DebugConsoleOp::try_from(0),
            Ok(DebugConsoleOp::Write)
        ));
        for raw in [1, 127, 255, 256, 1 << 16, u32::MAX] {
            assert!(matches!(
                DebugConsoleOp::try_from(raw),
                Err(CapError::InvalidOperation)
            ));
        }
    }

    #[cfg(feature = "debug_kernel")]
    #[test]
    fn debug_console_client_is_available_for_debug_kernels() {
        use vesper_objects::{DebugConsoleKey, KeySlot};

        // Construct handles only: host tests must not execute the SVC transport.
        let _console = DebugConsoleKey::new();
        let _other_slot = DebugConsoleKey::new_slot(KeySlot(42));
    }

    // Literal ABI oracles: do not derive these IDs from the production catalogue.
    const CORE_TYPES: [(CoreType, ObjectType, u8); 11] = [
        (CoreType::Null, ObjectType::NULL, 0),
        (CoreType::Untyped, ObjectType::UNTYPED, 1),
        (CoreType::Domain, ObjectType::DOMAIN, 2),
        (CoreType::KeyTable, ObjectType::KEY_TABLE, 3),
        (CoreType::Time, ObjectType::TIME, 4),
        (CoreType::Endpoint, ObjectType::ENDPOINT, 5),
        (CoreType::Notification, ObjectType::NOTIFICATION, 6),
        (CoreType::EventCount, ObjectType::EVENT_COUNT, 7),
        (CoreType::Buffer, ObjectType::BUFFER, 8),
        (CoreType::Reply, ObjectType::REPLY, 9),
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

//! Host tests of the word encoding and call outcomes Meddle-generated code
//! relies on.

#[cfg(test)]
mod tests {
    use metta_meddle_runtime::{
        ARGUMENT_WORDS, ChannelArea, DataArea, ImportRecord, IoRec, MAX_COUNTED_FUNCTIONS,
        OUTCOME_MALFORMED, OUTCOME_OK, RESULT_WORDS, Words, exception, instance_count, malformed,
        nth_instance, ok, unused_words_are_zero,
    };

    fn round_trip<T: Words + PartialEq + core::fmt::Debug>(value: &T) {
        let mut words = [0; 4];
        value.store(&mut words);
        assert_eq!(T::load(&words).as_ref(), Some(value));
    }

    #[test]
    fn unsigned_values_are_zero_extended_and_range_checked() {
        let mut words = [0; 1];
        0xab_u8.store(&mut words);
        assert_eq!(words[0], 0xab);
        assert_eq!(u8::load(&[0x100]), None);
        assert_eq!(u32::load(&[u64::from(u32::MAX) + 1]), None);
        round_trip(&u64::MAX);
        round_trip(&0xffff_u16);
    }

    #[test]
    fn signed_values_are_sign_extended_and_range_checked() {
        let mut words = [0; 1];
        (-1_i8).store(&mut words);
        assert_eq!(words[0], u64::MAX);
        assert_eq!(i8::load(&[u64::MAX]), Some(-1));
        // -129 does not fit an i8; 128 sign-extended is not 128.
        assert_eq!(i8::load(&[(-129_i64).cast_unsigned()]), None);
        assert_eq!(i8::load(&[128]), None);
        round_trip(&i32::MIN);
        round_trip(&i64::MIN);
    }

    #[test]
    fn booleans_are_zero_or_one() {
        round_trip(&true);
        round_trip(&false);
        assert_eq!(bool::load(&[2]), None);
    }

    #[test]
    fn references_take_base_then_length() {
        let rec = IoRec {
            base: 0x4000_0000,
            len: 64,
        };
        let mut words = [0; 2];
        rec.store(&mut words);
        assert_eq!(words, [0x4000_0000, 64]);
        assert_eq!(IoRec::COUNT, 2);
        round_trip(&rec);
    }

    #[test]
    fn outcomes_are_ok_exception_index_plus_one_or_malformed() {
        assert_eq!(ok([7, 0, 0, 0, 0]).words, [OUTCOME_OK, 7, 0, 0, 0, 0]);
        assert_eq!(exception(2, [9, 0, 0, 0, 0]).words, [3, 9, 0, 0, 0, 0]);
        assert_eq!(malformed().words, [OUTCOME_MALFORMED, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn answers_carry_five_ordered_words_after_the_outcome() {
        assert_eq!(RESULT_WORDS, 5);
        assert_eq!(ok([1, 2, 3, 4, 5]).words, [OUTCOME_OK, 1, 2, 3, 4, 5]);
        assert_eq!(
            exception(0, [u64::MAX, 0, 0, 0, 0x55]).words,
            [1, u64::MAX, 0, 0, 0, 0x55]
        );
        // Answer words past the results must be zero, like argument words.
        assert!(unused_words_are_zero(&[1, 2, 0, 0, 0], 2));
        assert!(!unused_words_are_zero(&[1, 2, 0, 0, 9], 2));
    }

    #[test]
    fn unused_words_must_be_zero() {
        let mut arguments = [0; ARGUMENT_WORDS];
        arguments[1] = 5;
        assert!(unused_words_are_zero(&arguments, 2));
        assert!(!unused_words_are_zero(&arguments, 1));
        arguments[5] = 1;
        assert!(!unused_words_are_zero(&arguments, 2));
    }

    #[test]
    fn data_area_references_stay_inside_the_area() {
        let area = DataArea {
            base: 0x3a00_0000,
            len: 0x1000,
        };
        assert_eq!(
            area.reference(0x40, 0x100),
            Some(IoRec {
                base: 0x3a00_0040,
                len: 0x100
            })
        );
        assert!(area.reference(0x1000, 0).is_some());
        assert!(area.reference(0xfff, 2).is_none());
        assert!(area.reference(u64::MAX, 2).is_none());

        for (base, len, inside) in [
            (0x3a00_0000, 0x1000, true),
            (0x3a00_0ff0, 0x10, true),
            (0x3a00_0ff0, 0x11, false),
            (0x39ff_fff0, 0x20, false),
            (u64::MAX - 8, 0x20, false),
        ] {
            assert_eq!(
                area.contains(&IoRec { base, len }),
                inside,
                "{base:#x}+{len:#x}"
            );
        }
    }

    #[test]
    fn data_area_words_round_trip_and_respect_bounds() {
        let mut storage = vec![0_u32; 16];
        let area = DataArea {
            base: storage.as_mut_ptr() as u64,
            len: 64,
        };
        assert!(area.write_words(8, &[1, 2, 3]));
        let mut words = [0; 3];
        assert!(area.read_words(8, &mut words));
        assert_eq!(words, [1, 2, 3]);
        assert_eq!(&storage[2..5], &[1, 2, 3]);
        // Out of range, misaligned and overflowing accesses are refused.
        assert!(!area.write_words(60, &[0, 0]));
        assert!(!area.write_words(2, &[0]));
        assert!(!area.read_words(u64::MAX - 1, &mut words));
        assert_eq!(&storage[2..5], &[1, 2, 3]);
    }

    #[test]
    fn data_area_u64s_round_trip_as_low_then_high_words() {
        let mut storage = vec![0_u32; 8];
        let area = DataArea {
            base: storage.as_mut_ptr() as u64,
            len: 32,
        };
        assert!(area.write_u64s(0, &[0x1122_3344_5566_7788, u64::MAX]));
        assert_eq!(
            &storage[..4],
            &[0x5566_7788, 0x1122_3344, u32::MAX, u32::MAX]
        );
        let mut values = [0; 2];
        assert!(area.read_u64s(0, &mut values));
        assert_eq!(values, [0x1122_3344_5566_7788, u64::MAX]);
        // The last value must fit inside the area.
        assert!(!area.write_u64s(24, &[1, 2]));
        assert!(area.write_u64s(24, &[1]));
    }

    #[test]
    fn data_area_bytes_round_trip_at_any_offset_and_respect_bounds() {
        let mut storage = vec![0_u32; 4];
        let area = DataArea {
            base: storage.as_mut_ptr() as u64,
            len: 16,
        };
        assert!(area.write_bytes(3, &[1, 2, 3, 4, 5]));
        let mut bytes = [0; 5];
        assert!(area.read_bytes(3, &mut bytes));
        assert_eq!(bytes, [1, 2, 3, 4, 5]);
        assert!(
            area.write_bytes(11, &[9; 5]),
            "ends exactly at the area end"
        );
        assert!(
            area.write_bytes(16, &[]),
            "an empty write at the end is inside"
        );
        assert!(!area.write_bytes(12, &[0; 5]));
        assert!(!area.read_bytes(u64::MAX, &mut bytes));
        assert!(!area.read_bytes(14, &mut bytes));
    }

    #[test]
    fn import_records_carry_their_data_area() {
        let record = ImportRecord {
            area_base: 0x3a00_0000,
            area_len: 0x1000,
            ..ImportRecord::default()
        };
        assert_eq!(
            record.data_area(),
            Some(DataArea {
                base: 0x3a00_0000,
                len: 0x1000
            })
        );
        assert_eq!(ImportRecord::default().data_area(), None);
    }

    #[test]
    fn import_records_carry_their_channel_after_the_control_page() {
        let record = ImportRecord {
            channel_base: 0x3a00_1000,
            channel_event: 0x1_0000_0200,
            ..ImportRecord::default()
        };
        assert_eq!(
            record.channel(2),
            Some(ChannelArea {
                control: 0x3a00_1000,
                data: DataArea {
                    base: 0x3a00_2000,
                    len: 0x2000
                },
                event: 0x1_0000_0200,
            })
        );
        assert_eq!(ImportRecord::default().channel(2), None);
    }

    /// A record of `function` of interface `type_code`, its Invocation
    /// `invocation` and its data area at `area_base`.
    const fn record(
        type_code: u64,
        function: u64,
        invocation: u64,
        area_base: u64,
    ) -> ImportRecord {
        ImportRecord {
            type_code,
            function,
            invocation,
            area_base,
            area_len: 4096,
            channel_base: 0,
            channel_event: 0,
        }
    }

    /// Kickstart links an import `via every` function by function, each to
    /// every exporter in load order: the records of two exporters interleave,
    /// with another interface's in between.
    #[test]
    fn every_instance_has_the_nth_record_of_each_function() {
        const DISPLAY: u64 = 0xd15;
        const LOG: u64 = 0x109;
        let imports = [
            record(DISPLAY, 0, 0x10, 0xa000),
            record(DISPLAY, 0, 0x20, 0xb000),
            record(LOG, 0, 0x30, 0),
            record(DISPLAY, 3, 0x11, 0xa000),
            record(DISPLAY, 3, 0x21, 0xb000),
        ];
        assert_eq!(instance_count(&imports, DISPLAY), 2);
        assert_eq!(instance_count(&imports, LOG), 1, "a single provider");
        assert_eq!(instance_count(&imports, 0x5ad), 0, "not imported");
        let invocations = |n| {
            nth_instance(&imports, DISPLAY, n)
                .map(|record| (record.function, record.invocation, record.area_base))
                .collect::<Vec<_>>()
        };
        assert_eq!(invocations(0), [(0, 0x10, 0xa000), (3, 0x11, 0xa000)]);
        assert_eq!(invocations(1), [(0, 0x20, 0xb000), (3, 0x21, 0xb000)]);
        assert!(invocations(2).is_empty(), "no third instance");
        // A single provider's records are instance 0.
        assert_eq!(
            nth_instance(&imports, LOG, 0)
                .map(|record| record.invocation)
                .collect::<Vec<_>>(),
            [0x30]
        );
        // Function indices past the counted ones are left out, not misplaced.
        let far = [record(
            DISPLAY,
            u64::try_from(MAX_COUNTED_FUNCTIONS).unwrap(),
            0x40,
            0,
        )];
        assert_eq!(nth_instance(&far, DISPLAY, 0).count(), 0);
        assert_eq!(instance_count(&far, DISPLAY), 0);
    }
}

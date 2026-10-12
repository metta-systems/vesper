//! Host tests of the word encoding and call outcomes Meddle-generated code
//! relies on.

#[cfg(test)]
mod tests {
    use vesper_meddle_runtime::{
        ARGUMENT_WORDS, IoRec, OUTCOME_MALFORMED, OUTCOME_OK, Words, exception, malformed, ok,
        unused_words_are_zero,
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
        let answer = ok(7);
        assert_eq!((answer.r0, answer.r1), (OUTCOME_OK, 7));
        let answer = exception(2, 9);
        assert_eq!((answer.r0, answer.r1), (3, 9));
        let answer = malformed();
        assert_eq!((answer.r0, answer.r1), (OUTCOME_MALFORMED, 0));
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
}

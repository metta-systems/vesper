//! Host tests of the property buffer checks.

#[cfg(test)]
mod tests {
    use mailbox_impl::{BUFFER_BYTES, Rejected, check_request, is_allowed, tag};

    /// A request of one tag with `value_words` value words, in a buffer of
    /// header, tag, end tag and one padding word.
    fn request(id: u32, value_words: usize) -> Vec<u32> {
        let mut words = vec![0, 0, id, u32::try_from(value_words * 4).unwrap(), 0];
        words.extend(core::iter::repeat_n(0, value_words));
        words.extend([0, 0]);
        words[0] = u32::try_from(words.len() * 4).unwrap();
        words
    }

    #[test]
    fn well_formed_requests_of_allowed_tags_pass() {
        assert_eq!(check_request(&request(tag::GET_BOARD_REVISION, 1)), Ok(()));
        assert_eq!(check_request(&request(tag::GET_ARM_MEMORY, 2)), Ok(()));
        assert_eq!(check_request(&request(tag::SET_PHYSICAL_SIZE, 2)), Ok(()));
        // No tags at all: header, end tag, padding.
        assert_eq!(check_request(&[12, 0, 0]), Ok(()));
    }

    #[test]
    fn several_tags_are_all_checked() {
        let mut words = vec![0, 0];
        words.extend([tag::GET_BOARD_REVISION, 4, 0, 0]);
        words.extend([tag::GET_ARM_MEMORY, 8, 0, 0, 0]);
        words.extend([0, 0]);
        words[0] = u32::try_from(words.len() * 4).unwrap();
        assert_eq!(check_request(&words), Ok(()));

        // The same buffer with a refused tag in the second place.
        words[6] = tag::EXECUTE_CODE;
        assert_eq!(
            check_request(&words),
            Err(Rejected::NotAllowed(tag::EXECUTE_CODE))
        );
    }

    #[test]
    fn tags_that_hand_over_the_system_are_refused() {
        for id in [
            tag::ALLOCATE_MEMORY,
            tag::LOCK_MEMORY,
            tag::UNLOCK_MEMORY,
            tag::RELEASE_MEMORY,
            tag::EXECUTE_CODE,
            tag::SET_CURSOR_INFO,
            tag::SET_CURSOR_STATE,
            tag::SET_POWER_STATE,
            tag::SET_CLOCK_STATE,
            tag::SET_CLOCK_RATE,
            tag::SET_GPIO_STATE,
        ] {
            assert!(!is_allowed(id), "{id:#x}");
            assert_eq!(
                check_request(&request(id, 2)),
                Err(Rejected::NotAllowed(id))
            );
        }
    }

    #[test]
    fn unknown_tags_are_refused() {
        for id in [0x0000_0002, 0x0001_00ff, 0x7fff_ffff, 0xffff_ffff] {
            assert_eq!(
                check_request(&request(id, 1)),
                Err(Rejected::NotAllowed(id))
            );
        }
    }

    #[test]
    fn misshapen_buffers_are_malformed() {
        let good = request(tag::GET_BOARD_REVISION, 1);
        // Too short for a header and an end tag.
        assert_eq!(check_request(&[]), Err(Rejected::Malformed));
        assert_eq!(check_request(&[8, 0]), Err(Rejected::Malformed));
        // The header size must be the buffer's size.
        for size in [0, 4, good[0] - 4, good[0] + 4, u32::MAX] {
            let mut words = good.clone();
            words[0] = size;
            assert_eq!(
                check_request(&words),
                Err(Rejected::Malformed),
                "size {size}"
            );
        }
        // A response, not a request.
        let mut words = good.clone();
        words[1] = 0x8000_0000;
        assert_eq!(check_request(&words), Err(Rejected::Malformed));
        // A tag already marked answered.
        let mut words = good.clone();
        words[4] = 0x8000_0004;
        assert_eq!(check_request(&words), Err(Rejected::Malformed));
        // A value buffer running past the end of the buffer.
        let mut words = good.clone();
        words[3] = 0x100;
        assert_eq!(check_request(&words), Err(Rejected::Malformed));
        words[3] = u32::MAX;
        assert_eq!(check_request(&words), Err(Rejected::Malformed));
        // No end tag.
        let mut words = vec![0, 0, tag::GET_BOARD_REVISION, 4, 0, 0];
        words[0] = u32::try_from(words.len() * 4).unwrap();
        assert_eq!(check_request(&words), Err(Rejected::Malformed));
        // A tag cut off after its id.
        let mut words = vec![0, 0, tag::GET_BOARD_REVISION];
        words[0] = 12;
        assert_eq!(check_request(&words), Err(Rejected::Malformed));
    }

    #[test]
    fn value_buffers_are_padded_to_whole_words() {
        // A 5-byte value buffer takes two words.
        let mut words = vec![0, 0, tag::GET_BOARD_MODEL, 5, 0, 0, 0, 0, 0];
        words[0] = u32::try_from(words.len() * 4).unwrap();
        assert_eq!(check_request(&words), Ok(()));
        // Not enough room for the second word.
        let mut words = vec![0, 0, tag::GET_BOARD_MODEL, 5, 0, 0];
        words[0] = u32::try_from(words.len() * 4).unwrap();
        assert_eq!(check_request(&words), Err(Rejected::Malformed));
    }

    #[test]
    fn buffers_are_limited_to_the_bounce_page() {
        let mut words = vec![0_u32; BUFFER_BYTES / 4];
        words[0] = u32::try_from(BUFFER_BYTES).unwrap();
        assert_eq!(check_request(&words), Ok(()));
        let mut words = vec![0_u32; BUFFER_BYTES / 4 + 1];
        words[0] = u32::try_from(words.len() * 4).unwrap();
        assert_eq!(check_request(&words), Err(Rejected::Malformed));
    }

    #[test]
    fn the_led_pin_is_the_expander_pin_unless_built_for_the_pi_4() {
        // Host builds carry no board cfg: the Raspberry Pi 3's expander pin.
        assert_eq!(
            mailbox_impl::ACT_LED_PIN,
            if cfg!(board_rpi4) { 42 } else { 130 }
        );
        assert!(!mailbox_impl::is_allowed(tag::SET_GPIO_STATE));
    }
}

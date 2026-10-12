//! Host tests of the framebuffer property buffers.

#[cfg(test)]
mod tests {
    use {
        fb_impl::{
            ATTRIBUTE_WORDS, Allocation, Attributes, CONFIGURE_WORDS, attributes_request,
            configure_request, physical_address, read_allocation, read_attributes,
        },
        mailbox_impl::{check_request, tag},
    };

    #[test]
    fn both_requests_pass_the_mailbox_whitelist() {
        assert_eq!(check_request(&configure_request(640, 480, 32)), Ok(()));
        assert_eq!(check_request(&attributes_request()), Ok(()));
    }

    #[test]
    fn the_configuring_request_lays_out_its_tags() {
        let request = configure_request(640, 480, 32);
        assert_eq!(request[0], 88);
        assert_eq!(request[1], 0);
        assert_eq!(request[2..7], [tag::SET_PHYSICAL_SIZE, 8, 0, 640, 480]);
        assert_eq!(request[7..12], [tag::SET_VIRTUAL_SIZE, 8, 0, 640, 480]);
        assert_eq!(request[12..16], [tag::SET_DEPTH, 4, 0, 32]);
        assert_eq!(request[16..21], [tag::ALLOCATE_BUFFER, 8, 0, 16, 0]);
        assert_eq!(request[21], 0, "the end tag");
        assert_eq!(request.len(), CONFIGURE_WORDS);
    }

    #[test]
    fn the_attributes_request_lays_out_its_tags() {
        let request = attributes_request();
        assert_eq!(request[0], 64);
        assert_eq!(request[2..6], [tag::GET_PIXEL_ORDER, 4, 0, 0]);
        assert_eq!(request[6..10], [tag::SET_ALPHA_MODE, 4, 0, 2]);
        assert_eq!(request[10..14], [tag::GET_PITCH, 4, 0, 0]);
        assert_eq!(request[14], 0, "the end tag");
        assert_eq!(request.len(), ATTRIBUTE_WORDS);
    }

    #[test]
    fn answers_are_read_from_the_tags_that_were_answered() {
        let mut answer = configure_request(640, 480, 32);
        answer[18] = 0x8000_0008;
        answer[19] = 0xc3e0_0000;
        answer[20] = 0x12_c000;
        assert_eq!(
            read_allocation(&answer),
            Ok(Allocation {
                bus_address: 0xc3e0_0000,
                bytes: 0x12_c000
            })
        );
        // An allocation the firmware ignored is reported with its code word.
        answer[18] = 8;
        assert_eq!(read_allocation(&answer), Err(8));

        let mut answer = attributes_request();
        answer[4] = 0x8000_0004;
        answer[5] = 1;
        answer[12] = 0x8000_0004;
        answer[13] = 2560;
        assert_eq!(
            read_attributes(&answer),
            Ok(Attributes {
                pixel_order: 1,
                pitch: 2560
            })
        );
        answer[12] = 4;
        assert_eq!(read_attributes(&answer), Err(4));
        answer[12] = 0x8000_0004;
        answer[4] = 0;
        assert_eq!(read_attributes(&answer), Err(0));
    }

    #[test]
    fn bus_addresses_become_physical_addresses() {
        assert_eq!(physical_address(0xc3e0_0000, 0xc000_0000), 0x03e0_0000);
        // Without a usable offset the bus aliases are masked off.
        assert_eq!(physical_address(0xc3e0_0000, u64::MAX), 0x03e0_0000);
        assert_eq!(physical_address(0x4000_1000, u64::MAX), 0x0000_1000);
        // An address below the offset falls back to the same masking.
        assert_eq!(physical_address(0x8000_1000, 0xc000_0000), 0x0000_1000);
    }
}

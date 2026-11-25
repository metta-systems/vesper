//! Host tests of reading a display's modes from its EDID, and of the mailbox
//! request that fetches it.

#[cfg(test)]
mod tests {
    use {
        fb_impl::{
            EDID_WORDS, NoEdid,
            edid::{BLOCK_BYTES, Edid, Invalid, MAX_MODES, Modes, Timing},
            edid_request, read_edid,
        },
        mailbox_impl::{check_request, tag},
    };

    const fn timing(width: u32, height: u32, refresh_mhz: u32) -> Timing {
        Timing {
            width,
            height,
            refresh_mhz,
        }
    }

    /// A detailed timing descriptor: `clock` in 10 kHz units, the active and
    /// blanking pixels and lines, the image size in mm, and byte 17.
    fn descriptor(
        clock: u16,
        (width, horizontal_blank): (u16, u16),
        (height, vertical_blank): (u16, u16),
        (width_mm, height_mm): (u16, u16),
        features: u8,
    ) -> [u8; 18] {
        let low = |value: u16| value.to_le_bytes()[0];
        let high = |value: u16| u8::try_from(value >> 8).unwrap();
        let mut bytes = [0; 18];
        bytes[0..2].copy_from_slice(&clock.to_le_bytes());
        bytes[2] = low(width);
        bytes[3] = low(horizontal_blank);
        bytes[4] = high(width) << 4 | high(horizontal_blank);
        bytes[5] = low(height);
        bytes[6] = low(vertical_blank);
        bytes[7] = high(height) << 4 | high(vertical_blank);
        bytes[12] = low(width_mm);
        bytes[13] = low(height_mm);
        bytes[14] = high(width_mm) << 4 | high(height_mm);
        bytes[17] = features;
        bytes
    }

    /// A monitor's base block: EDID `version`, a 53x30 cm screen, two standard
    /// timings and four descriptors: 1920x1080 at 60 Hz (CEA-861's 148.5 MHz
    /// clock, 2200x1125 total, 531x299 mm), a monitor name, an interlaced
    /// 1920x1080 and 1280x720 at 60 Hz (74.25 MHz, 1650x750 total).
    fn monitor(version: (u8, u8), preferred_mm: (u16, u16)) -> [u8; BLOCK_BYTES] {
        let mut block = [0; BLOCK_BYTES];
        block[0..8].copy_from_slice(&[0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00]);
        block[18] = version.0;
        block[19] = version.1;
        block[21] = 53;
        block[22] = 30;
        // Standard timings: 1280 (0x81) at 5:4 and 60 Hz, 1680 (0xb3) at
        // aspect `00` and 60 Hz, then unused.
        block[38..42].copy_from_slice(&[0x81, 0x80, 0xb3, 0x00]);
        for unused in (42..54).step_by(2) {
            block[unused..unused + 2].copy_from_slice(&[0x01, 0x01]);
        }
        let descriptors = [
            descriptor(14_850, (1920, 280), (1080, 45), preferred_mm, 0x1e),
            {
                // A display descriptor: pixel clock 0, tag 0xfc (the name).
                let mut name = [0; 18];
                name[3] = 0xfc;
                name[5..12].copy_from_slice(b"Monitor");
                name
            },
            descriptor(7_425, (1920, 280), (540, 22), (531, 299), 0x9e),
            descriptor(7_425, (1280, 370), (720, 30), (531, 299), 0x1e),
        ];
        for (index, bytes) in descriptors.iter().enumerate() {
            let start = 54 + index * 18;
            block[start..start + 18].copy_from_slice(bytes);
        }
        let sum = block[..127]
            .iter()
            .fold(0_u8, |sum, byte| sum.wrapping_add(*byte));
        block[127] = 0_u8.wrapping_sub(sum);
        block
    }

    const MONITOR_MM: (u16, u16) = (531, 299);

    #[test]
    fn only_a_block_with_the_header_and_checksum_is_an_edid() {
        let block = monitor((1, 4), MONITOR_MM);
        Edid::new(&block).expect("the monitor's block is an EDID");
        let mut wrong_sum = block;
        wrong_sum[127] = wrong_sum[127].wrapping_add(1);
        assert_eq!(Edid::new(&wrong_sum).err(), Some(Invalid::Checksum));
        let mut no_header = block;
        no_header[0] = 0x01;
        assert_eq!(Edid::new(&no_header).err(), Some(Invalid::Header));
        assert_eq!(
            Edid::new(&[0; BLOCK_BYTES]).err(),
            Some(Invalid::Header),
            "nothing attached"
        );
        assert_eq!(Edid::new(&block).unwrap().version(), (1, 4));
    }

    #[test]
    fn detailed_timings_are_the_progressive_timing_descriptors_in_order() {
        let block = monitor((1, 4), MONITOR_MM);
        let edid = Edid::new(&block).unwrap();
        let timings: Vec<_> = edid.detailed_timings().collect();
        // The name descriptor and the interlaced timing are left out; 148.5
        // MHz over 2200x1125 and 74.25 MHz over 1650x750 are 60 Hz exactly.
        assert_eq!(
            timings,
            [timing(1920, 1080, 60_000), timing(1280, 720, 60_000)]
        );
    }

    #[test]
    fn standard_timings_read_width_aspect_and_refresh() {
        let block = monitor((1, 4), MONITOR_MM);
        let timings: Vec<_> = Edid::new(&block).unwrap().standard_timings().collect();
        // (0x81 + 31) * 8 = 1280 at 5:4; (0xb3 + 31) * 8 = 1680 at 16:10.
        assert_eq!(
            timings,
            [timing(1280, 1024, 60_000), timing(1680, 1050, 60_000)]
        );
        // Before EDID 1.3 aspect `00` was 1:1.
        let block = monitor((1, 2), MONITOR_MM);
        let timings: Vec<_> = Edid::new(&block).unwrap().standard_timings().collect();
        assert_eq!(timings[1], timing(1680, 1680, 60_000));
    }

    #[test]
    fn the_modes_put_the_preferred_timing_first_and_list_each_size_once() {
        let block = monitor((1, 4), MONITOR_MM);
        let modes = Edid::new(&block).unwrap().modes();
        assert_eq!(
            modes.as_slice(),
            [
                timing(1920, 1080, 60_000),
                timing(1280, 720, 60_000),
                timing(1280, 1024, 60_000),
                timing(1680, 1050, 60_000),
            ]
        );
        let mut modes = Modes::empty();
        assert!(modes.push(timing(800, 600, 60_000)));
        assert!(
            !modes.push(timing(800, 600, 75_000)),
            "a size is listed once, at its first refresh"
        );
        for width in 1..=u32::try_from(MAX_MODES).unwrap() {
            modes.push(timing(width, 1, 0));
        }
        assert_eq!(modes.as_slice().len(), MAX_MODES, "full");
        assert_eq!(
            Modes::fallback(timing(1024, 768, 0)).as_slice(),
            [timing(1024, 768, 0)]
        );
    }

    #[test]
    fn the_image_size_is_the_preferred_timings_else_the_screens() {
        let block = monitor((1, 4), MONITOR_MM);
        assert_eq!(Edid::new(&block).unwrap().image_size_mm(), Some((531, 299)));
        // No size in the timing: the screen size, in centimetres.
        let block = monitor((1, 4), (0, 0));
        assert_eq!(Edid::new(&block).unwrap().image_size_mm(), Some((530, 300)));
        // Neither: unknown (a zero centimetre byte is an aspect ratio).
        let mut block = monitor((1, 4), (0, 0));
        block[22] = 0;
        block[127] = block[127].wrapping_add(30);
        assert_eq!(Edid::new(&block).unwrap().image_size_mm(), None);
    }

    /// The board's 1024x600 HDMI panel (about 165x95 mm) says 255x255 mm in
    /// its timing and 18x13 cm in its screen size bytes: 255x255 names no
    /// size, so the centimetres count.
    #[test]
    fn a_timing_size_of_255_by_255_mm_names_no_size() {
        let mut block = monitor((1, 3), (255, 255));
        block[21] = 18;
        block[22] = 13;
        let sum = block[..127]
            .iter()
            .fold(0_u8, |sum, byte| sum.wrapping_add(*byte));
        block[127] = 0_u8.wrapping_sub(sum);
        let edid = Edid::new(&block).unwrap();
        assert_eq!(edid.timing_size_mm(), Some((255, 255)), "as the EDID says");
        assert_eq!(edid.screen_size_mm(), Some((180, 130)));
        assert_eq!(edid.image_size_mm(), Some((180, 130)));
        // Only exactly 255x255 is the marker: one side of 255 is a size.
        let mut wide = monitor((1, 3), (255, 140));
        let sum = wide[..127]
            .iter()
            .fold(0_u8, |sum, byte| sum.wrapping_add(*byte));
        wide[127] = 0_u8.wrapping_sub(sum);
        assert_eq!(Edid::new(&wide).unwrap().image_size_mm(), Some((255, 140)));
    }

    #[test]
    fn the_edid_request_asks_for_one_block_and_passes_the_mailbox() {
        let request = edid_request(0);
        assert_eq!(request.len(), EDID_WORDS);
        assert_eq!(request[0], 160, "40 words");
        assert_eq!(request[2..6], [tag::GET_EDID_BLOCK, 136, 0, 0]);
        assert_eq!(request[39], 0, "the end tag");
        assert_eq!(check_request(&request), Ok(()));
        assert_eq!(edid_request(1)[5], 1);
    }

    #[test]
    fn the_answer_holds_the_block_in_its_own_byte_order() {
        let block = monitor((1, 4), MONITOR_MM);
        let mut answer = edid_request(0);
        answer[4] = 0x8000_0000 | 136;
        answer[5] = 0;
        answer[6] = 0;
        for (word, bytes) in answer[7..39].iter_mut().zip(block.as_chunks::<4>().0) {
            *word = u32::from_le_bytes(*bytes);
        }
        assert_eq!(read_edid(&answer), Ok(block));

        let mut none = answer;
        none[6] = 1;
        assert_eq!(read_edid(&none), Err(NoEdid::Status(1)), "no such block");
        let unanswered = edid_request(0);
        assert_eq!(read_edid(&unanswered), Err(NoEdid::NotAnswered(0)));
    }
}

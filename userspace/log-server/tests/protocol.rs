//! Host tests of what the log server and its clients agree on: the header
//! encoding, the buffer set, line cutting and the server's output line.

#[cfg(test)]
mod tests {
    use {
        core::fmt::Write,
        libio_channel::IoRec,
        log_server::{
            BUFFERS, FreeBuffers, Header, LINE_BYTES, LINE_LEN, Level, LineWriter,
            OUTPUT_LINE_BYTES, buffer_of, buffer_record, format_line,
        },
    };

    #[test]
    fn header_round_trips() {
        let header = Header {
            level: Some(Level::Warn),
            cut: true,
            dropped: 513,
            micros: 12_345_678,
        };
        assert_eq!(Header::decode(header.encode()), header);
        let plain = Header {
            level: None,
            cut: false,
            dropped: 0,
            micros: 0,
        };
        assert_eq!(plain.encode(), 0);
        assert_eq!(Header::decode(0), plain);
    }

    #[test]
    fn header_keeps_forty_bits_of_time() {
        let header = Header {
            level: Some(Level::Info),
            cut: false,
            dropped: 0,
            micros: (1 << 40) + 7,
        };
        assert_eq!(Header::decode(header.encode()).micros, 7);
    }

    #[test]
    fn unknown_level_reads_as_none() {
        assert_eq!(Header::decode(6).level, None);
        assert_eq!(Header::decode(7).level, None);
    }

    #[test]
    fn every_buffer_is_taken_once() {
        let mut free = FreeBuffers::all();
        assert_eq!(free.count() as usize, BUFFERS);
        let mut seen = 0_u64;
        while let Some(index) = free.take() {
            assert_eq!(seen & (1 << index), 0, "buffer {index} taken twice");
            seen |= 1 << index;
        }
        assert_eq!(seen.count_ones() as usize, BUFFERS);
        free.give(5);
        free.give(99);
        assert_eq!(free.count(), 1);
        assert_eq!(free.take(), Some(5));
    }

    #[test]
    fn records_name_their_buffer() {
        let base = 0x4000_0000;
        for index in 0..u32::try_from(BUFFERS).unwrap() {
            assert_eq!(buffer_of(base, buffer_record(base, index, 10)), Some(index));
        }
        let outside = IoRec {
            base: base + BUFFERS as u64 * LINE_BYTES,
            len: 1,
        };
        assert_eq!(buffer_of(base, outside), None);
        let unaligned = IoRec {
            base: base + 3,
            len: 1,
        };
        assert_eq!(buffer_of(base, unaligned), None);
        let below = IoRec {
            base: base - LINE_BYTES,
            len: 1,
        };
        assert_eq!(buffer_of(base, below), None);
    }

    #[test]
    fn writer_cuts_at_a_character_boundary() {
        let mut buffer = [0_u8; 8];
        let mut writer = LineWriter::new(&mut buffer);
        write!(writer, "abcdefgé!").unwrap();
        let (len, cut) = writer.finish();
        assert!(cut);
        assert_eq!(&buffer[..len], b"abcdefg");
    }

    #[test]
    fn writer_drops_trailing_line_ends() {
        let mut buffer = [0_u8; 32];
        let mut writer = LineWriter::new(&mut buffer);
        write!(writer, "done\r\n\n").unwrap();
        let (len, cut) = writer.finish();
        assert!(!cut);
        assert_eq!(&buffer[..len], b"done");
    }

    fn formatted(header: Header, text: &[u8]) -> String {
        let mut out = [0_u8; OUTPUT_LINE_BYTES];
        let len = format_line(header, text, &mut out);
        String::from_utf8(out[..len].to_vec()).unwrap()
    }

    #[test]
    fn line_has_time_level_and_end() {
        let header = Header {
            level: Some(Level::Info),
            cut: false,
            dropped: 0,
            micros: 12_345_678,
        };
        assert_eq!(
            formatted(header, b"wood: started"),
            "[    12.345678] I wood: started\r\n"
        );
    }

    #[test]
    fn line_without_level_and_with_drops_and_cut() {
        let header = Header {
            level: None,
            cut: true,
            dropped: 3,
            micros: 5,
        };
        assert_eq!(
            formatted(header, b"x"),
            "[     0.000005] - (3 dropped) x…\r\n"
        );
    }

    #[test]
    fn control_bytes_are_replaced() {
        let header = Header {
            level: Some(Level::Error),
            cut: false,
            dropped: 0,
            micros: 0,
        };
        assert_eq!(
            formatted(header, b"a\x1b[2Jb\rc\x7f"),
            "[     0.000000] E a?[2Jb?c?\r\n"
        );
    }

    #[test]
    fn longest_line_fits() {
        let header = Header {
            level: Some(Level::Trace),
            cut: true,
            dropped: u16::MAX,
            micros: (1 << 40) - 1,
        };
        let text = [b'z'; LINE_LEN];
        let line = formatted(header, &text);
        assert!(line.ends_with("z…\r\n"), "{line}");
        assert_eq!(line.matches('z').count(), LINE_LEN);
    }

    #[test]
    fn short_output_still_ends_the_line() {
        let header = Header {
            level: Some(Level::Info),
            cut: false,
            dropped: 0,
            micros: 0,
        };
        let mut out = [0_u8; 20];
        let len = format_line(header, b"a long line of text", &mut out);
        assert_eq!(len, 20);
        assert_eq!(&out[18..20], b"\r\n");
    }
}

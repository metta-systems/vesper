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
            report::{Cause, FaultReport},
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

    /// Kickstart's layout: `[guard][stack 0][guard][stack 1]…` from the
    /// stack region's base, 8 pages a stack.
    const REGION: (u64, u64) = (0x3000_0000, 0x3020_0000);
    const STACKS: [(u64, u64); 2] = [(0x3000_1000, 0x3000_9000), (0x3000_a000, 0x3001_2000)];
    /// `ESR_EL1` of a data abort from EL0, translation fault at level 3, on a
    /// write.
    const TRANSLATION_FAULT_WRITE: u64 = (0x24 << 26) | (1 << 25) | (1 << 6) | 0x07;

    fn fault(kind: u64, syndrome: u64, fault_address: u64, sp: u64) -> FaultReport<'static> {
        FaultReport {
            component: "usb_core",
            kind,
            syndrome,
            fault_address,
            pc: 0x20_1234,
            sp,
            thread_stacks: &STACKS,
            stack_region: REGION,
        }
    }

    fn line(report: &FaultReport<'_>) -> String {
        let mut text = String::new();
        write!(text, "{report}").expect("formatting into a String");
        text
    }

    #[test]
    fn a_guard_page_hit_is_a_stack_overflow_of_its_thread() {
        // Thread 1 ran into the guard page below its stack: SP and FAR there.
        let overflow = fault(0, TRANSLATION_FAULT_WRITE, 0x3000_9ff0, 0x3000_9ff0);
        assert_eq!(overflow.cause(), Cause::StackOverflow);
        assert_eq!(overflow.thread(), Some(1));
        assert_eq!(
            line(&overflow),
            "FAULT: usb_core thread 1: stack overflow (guard page) at pc 0x201234, far 0x30009ff0, sp 0x30009ff0, esr 0x92000047"
        );
        // A large frame moved SP further down, onto thread 0's stack; the
        // store hit the guard of thread 1, which names the Thread.
        let far_below = fault(0, TRANSLATION_FAULT_WRITE, 0x3000_9800, 0x3000_8800);
        assert_eq!(far_below.cause(), Cause::StackOverflow);
        assert_eq!(far_below.thread(), Some(1));
        // Thread 0's guard is the region's first page.
        let first = fault(0, TRANSLATION_FAULT_WRITE, 0x3000_0ff8, 0x3000_0ff8);
        assert_eq!(first.thread(), Some(0));
    }

    #[test]
    fn other_faults_keep_their_cause() {
        // A translation fault outside the stack region is a plain data abort.
        let null = fault(0, TRANSLATION_FAULT_WRITE, 0x8, 0x3000_8000);
        assert_eq!(null.cause(), Cause::DataAbort);
        assert_eq!(null.thread(), Some(0));
        // A permission fault in the region (status 0x0f) is not an overflow.
        let permission = fault(0, (0x24 << 26) | 0x0f, 0x3000_5000, 0x3000_8000);
        assert_eq!(permission.cause(), Cause::DataAbort);
        assert_eq!(
            fault(0, 0x3c << 26, 0, 0x3000_8000).cause(),
            Cause::Breakpoint
        );
        assert_eq!(
            fault(0, 0, 0, 0x3000_8000).cause(),
            Cause::UndefinedInstruction
        );
        assert_eq!(fault(1, 0, 0, 0x3000_8000).cause(), Cause::IllegalReturn);
        assert_eq!(
            fault(2, 0, 0, 0x3000_8000).cause(),
            Cause::ReturnTargetRetired
        );
        assert_eq!(fault(9, 0, 0, 0x3000_8000).cause(), Cause::UnknownKind(9));
        assert_eq!(fault(0, 0x31 << 26, 0, 0).cause(), Cause::Exception(0x31));
    }

    #[test]
    fn a_fault_off_every_thread_stack_is_in_an_export_call() {
        // SP on a stack of the export pool, past the Threads' stacks.
        let in_export = fault(0, 0x3c << 26, 0, 0x3005_0000);
        assert_eq!(in_export.thread(), None);
        assert!(
            line(&in_export).starts_with("FAULT: usb_core in an export call: breakpoint at pc"),
            "{}",
            line(&in_export)
        );
    }
}

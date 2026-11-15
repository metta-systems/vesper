//! Host tests of the frame walk on modelled stacks.

#[cfg(test)]
mod tests {
    use {
        std::collections::HashMap,
        vesper_stacktrace::{
            Backtrace, ENABLED, MAX_FRAME_BYTES, MAX_FRAMES, frames, write_trace,
        },
    };

    /// A modelled stack: frame records at their addresses.
    struct Stack {
        words: HashMap<u64, u64>,
    }

    impl Stack {
        fn new() -> Self {
            Self {
                words: HashMap::new(),
            }
        }

        /// A frame record at `record`: the caller's record and the return
        /// address.
        fn record(mut self, record: u64, caller: u64, return_address: u64) -> Self {
            self.words.insert(record, caller);
            self.words.insert(record + 8, return_address);
            self
        }

        fn read(&self) -> impl Fn(u64) -> Option<u64> + '_ {
            |address| self.words.get(&address).copied()
        }
    }

    #[test]
    fn follows_the_chain_up_to_the_first_frame() {
        let stack = Stack::new()
            .record(0x3000_8e00, 0x3000_8e40, 0x40_1234)
            .record(0x3000_8e40, 0x3000_8f00, 0x40_0c80)
            .record(0x3000_8f00, 0, 0x40_0010);
        let walked: Vec<_> = frames(0x3000_8e00, stack.read()).collect();
        assert_eq!(walked, [0x40_1234, 0x40_0c80, 0x40_0010]);
    }

    #[test]
    fn stops_at_a_record_that_does_not_move_up_the_stack() {
        // A loop back to itself, and one back down.
        let looping = Stack::new().record(0x1000, 0x1000, 0x40_0001);
        assert_eq!(frames(0x1000, looping.read()).collect::<Vec<_>>(), [0x40_0001]);
        let downward = Stack::new()
            .record(0x2000, 0x1000, 0x40_0002)
            .record(0x1000, 0, 0x40_0003);
        assert_eq!(frames(0x2000, downward.read()).collect::<Vec<_>>(), [0x40_0002]);
    }

    #[test]
    fn stops_at_a_caller_record_beyond_a_frame() {
        let far = 0x1_0000 + MAX_FRAME_BYTES + 16;
        let stack = Stack::new()
            .record(0x1_0000, far, 0x40_0004)
            .record(far, 0, 0x40_0005);
        assert_eq!(frames(0x1_0000, stack.read()).collect::<Vec<_>>(), [0x40_0004]);
    }

    #[test]
    fn stops_at_a_misaligned_or_zero_frame_pointer() {
        let stack = Stack::new().record(0x1008, 0, 0x40_0006);
        assert_eq!(frames(0x1008, stack.read()).count(), 0);
        assert_eq!(frames(0, stack.read()).count(), 0);
    }

    #[test]
    fn stops_where_a_word_cannot_be_read() {
        // The second record is not on the modelled stack.
        let stack = Stack::new().record(0x1000, 0x1100, 0x40_0007);
        assert_eq!(frames(0x1000, stack.read()).collect::<Vec<_>>(), [0x40_0007]);
    }

    #[test]
    fn lists_at_most_max_frames() {
        let mut stack = Stack::new();
        for index in 0..40_u64 {
            stack = stack.record(0x1000 + index * 0x40, 0x1000 + (index + 1) * 0x40, 0x40_0000 + index);
        }
        assert_eq!(frames(0x1000, stack.read()).count(), MAX_FRAMES);
    }

    #[test]
    fn writes_the_addresses_on_one_line() {
        let mut line = String::new();
        write_trace(&mut line, [0x40_1234, 0xffff_8000_0001_0fdc].into_iter()).unwrap();
        assert_eq!(line, "backtrace: 0x401234 0xffff800000010fdc");
        let mut empty = String::new();
        write_trace(&mut empty, core::iter::empty()).unwrap();
        assert_eq!(empty, "");
    }

    #[test]
    fn prints_nothing_without_stacktrace() {
        // The host tests build without `--cfg stacktrace`.
        const { assert!(!ENABLED) };
        let stack = Stack::new().record(0x1000, 0, 0x40_0008);
        let trace = Backtrace::new(Some(0x40_0100), 0x1000, stack.read());
        assert_eq!(trace.to_string(), "");
    }
}

//! Host tests of the power management register values.

#[cfg(test)]
mod tests {
    use power_impl::{PASSWORD, full_reset_control, halt_status, register, watchdog_start};

    #[test]
    fn every_value_carries_the_password() {
        for value in [halt_status(0), full_reset_control(0), watchdog_start()] {
            assert_eq!(value & 0xff00_0000, PASSWORD);
        }
    }

    #[test]
    fn halt_marks_partition_63_and_keeps_other_bits() {
        // Partition 63 spreads over bits 0, 2, 4, 6, 8 and 10.
        assert_eq!(halt_status(0) & 0xfff, 0x555);
        assert_eq!(halt_status(0x2), PASSWORD | 0x557);
        assert_eq!(halt_status(0x0000_f000) & 0x0000_f000, 0xf000);
    }

    #[test]
    fn reset_control_selects_a_full_reset_and_keeps_other_bits() {
        let control = full_reset_control(0x0000_0030);
        assert_eq!(
            control & 0x30,
            0x20,
            "the watchdog configuration is a full reset"
        );
        assert_eq!(full_reset_control(0x0000_0f00) & 0xf00, 0xf00);
    }

    #[test]
    fn the_watchdog_counts_ten_ticks() {
        assert_eq!(watchdog_start(), PASSWORD | 10);
    }

    #[test]
    fn registers_follow_the_block_layout() {
        assert_eq!(
            [
                register::RESET_CONTROL,
                register::RESET_STATUS,
                register::WATCHDOG
            ],
            [0x1c, 0x20, 0x24]
        );
    }
}

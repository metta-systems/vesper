//! Host tests of the power management register values.

#[cfg(test)]
mod tests {
    use power_impl::{
        PASSWORD, ResetControl, ResetStatus, full_reset_control, halt_status, watchdog_start,
    };

    const PASSWORD_BITS: u32 = (PASSWORD as u32) << 24;

    #[test]
    fn every_value_carries_the_password() {
        for value in [
            halt_status(ResetStatus::ZERO).raw_value(),
            full_reset_control(ResetControl::ZERO).raw_value(),
            watchdog_start().raw_value(),
        ] {
            assert_eq!(value & 0xff00_0000, PASSWORD_BITS);
        }
    }

    #[test]
    fn halt_marks_partition_63_and_keeps_other_bits() {
        let halt = |current| halt_status(ResetStatus::new_with_raw_value(current)).raw_value();
        // Partition 63 spreads over bits 0, 2, 4, 6, 8 and 10.
        assert_eq!(halt(0) & 0xfff, 0x555);
        assert_eq!(halt(0x2), PASSWORD_BITS | 0x557);
        assert_eq!(halt(0x0000_f000) & 0x0000_f000, 0xf000);
    }

    #[test]
    fn reset_control_selects_a_full_reset_and_keeps_other_bits() {
        let control =
            |current| full_reset_control(ResetControl::new_with_raw_value(current)).raw_value();
        assert_eq!(
            control(0x0000_0030) & 0x30,
            0x20,
            "the watchdog configuration is a full reset"
        );
        assert_eq!(control(0x0000_0f00) & 0xf00, 0xf00);
    }

    #[test]
    fn the_watchdog_counts_ten_ticks() {
        assert_eq!(watchdog_start().raw_value(), PASSWORD_BITS | 10);
    }
}

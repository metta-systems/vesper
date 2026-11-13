//! Host tests of the SDHOST register values.

#[cfg(test)]
mod tests {
    use {
        libsd_card::{
            ALL_SEND_CID, GO_IDLE_STATE, IDENTIFICATION_HZ, READ_SINGLE_BLOCK, SEND_STATUS,
            TRANSFER_HZ, WRITE_BLOCK,
        },
        sdhost_bcm2835::{card_clock, clock_divider, command_register},
    };

    #[test]
    fn the_clock_never_runs_above_the_rate_asked() {
        for core_hz in [250_000_000, 350_000_000, 400_000_000] {
            for hz in [IDENTIFICATION_HZ, TRANSFER_HZ] {
                let divider = clock_divider(core_hz, hz);
                assert!(card_clock(core_hz, divider) <= hz, "{core_hz} / {hz}");
                assert!(
                    card_clock(core_hz, divider - 1) > hz,
                    "the fastest such clock"
                );
            }
        }
        // 250 MHz to 400 kHz is 625, so a divider of 623, as Linux computes.
        assert_eq!(clock_divider(250_000_000, 400_000), 623);
        // The divider is at least zero (a clock of half the core) and at most 0x7ff.
        assert_eq!(clock_divider(250_000_000, 250_000_000), 0);
        assert_eq!(clock_divider(250_000_000, 1000), 0x7ff);
    }

    #[test]
    fn command_register_values() {
        // Linux's `SDCMD_*` bits: new 0x8000, long 0x200, none 0x400, read
        // 0x40, write 0x80.
        assert_eq!(command_register(GO_IDLE_STATE).raw_value(), 0x8400);
        assert_eq!(command_register(ALL_SEND_CID).raw_value(), 0x8202);
        assert_eq!(command_register(SEND_STATUS).raw_value(), 0x800d);
        assert_eq!(command_register(READ_SINGLE_BLOCK).raw_value(), 0x8051);
        assert_eq!(command_register(WRITE_BLOCK).raw_value(), 0x8098);
    }
}

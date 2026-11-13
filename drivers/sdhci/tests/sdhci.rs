//! Host tests of the SDHCI register values, clock dividers, long responses
//! and ADMA2 descriptors.

#[cfg(test)]
mod tests {
    use {
        libsd_card::{
            ALL_SEND_CID, BusError, GO_IDLE_STATE, IDENTIFICATION_HZ, READ_MULTIPLE_BLOCK,
            READ_SINGLE_BLOCK, SD_SEND_OP_COND, SEND_STATUS, TRANSFER_HZ, WRITE_BLOCK,
            WRITE_MULTIPLE_BLOCK,
        },
        sdhci::{
            ADMA2_MAX_BYTES, Interrupts, adma2_descriptor, card_clock, clock_divider,
            long_response, transfer_command,
        },
    };

    #[test]
    fn command_words() {
        // Command index in bits 29-24, data present 21, index check 20, CRC
        // check 19, response type 17-16; transfer mode below (§2.2.5, §2.2.6).
        assert_eq!(
            transfer_command(GO_IDLE_STATE, 0, false).raw_value(),
            0x0000_0000
        );
        // R2: 136 bits, CRC checked, no index.
        assert_eq!(
            transfer_command(ALL_SEND_CID, 0, false).raw_value(),
            0x0209_0000
        );
        // R1: 48 bits, CRC and index checked.
        assert_eq!(
            transfer_command(SEND_STATUS, 0, false).raw_value(),
            0x0d1a_0000
        );
        // R3: 48 bits, neither checked.
        assert_eq!(
            transfer_command(SD_SEND_OP_COND, 0, false).raw_value(),
            0x2902_0000
        );
    }

    #[test]
    fn data_command_words() {
        // One block read through the port: data present, read, block count.
        assert_eq!(
            transfer_command(READ_SINGLE_BLOCK, 1, false).raw_value(),
            0x113a_0012
        );
        // One block written by DMA.
        assert_eq!(
            transfer_command(WRITE_BLOCK, 1, true).raw_value(),
            0x183a_0003
        );
        // Several blocks: multiple, Auto CMD12 (bits 3-2 = 01).
        assert_eq!(
            transfer_command(READ_MULTIPLE_BLOCK, 8, true).raw_value(),
            0x123a_0037
        );
        assert_eq!(
            transfer_command(WRITE_MULTIPLE_BLOCK, 8, false).raw_value(),
            0x193a_0026
        );
    }

    #[test]
    fn the_clock_never_runs_above_the_rate_asked() {
        for base_hz in [52_000_000, 100_000_000, 200_000_000] {
            for hz in [IDENTIFICATION_HZ, TRANSFER_HZ] {
                // Before version 3.00 the clock divides by 256 at most.
                let reachable = base_hz / 256 <= hz;
                for ten_bit in [true, false].into_iter().filter(|ten| *ten || reachable) {
                    let divider = clock_divider(base_hz, hz, ten_bit);
                    assert!(
                        card_clock(base_hz, divider) <= hz,
                        "{base_hz} / {hz} / {ten_bit}"
                    );
                }
                // The 10-bit mode gives the fastest such clock.
                let divider = clock_divider(base_hz, hz, true);
                assert!(card_clock(base_hz, divider - 1) > hz, "{base_hz} / {hz}");
            }
        }
        // 200 MHz to 400 kHz: 250 in the 10-bit mode; before it the largest
        // divider, 128 (a clock of 781.25 kHz, too fast for identification).
        assert_eq!(clock_divider(200_000_000, 400_000, true), 250);
        assert_eq!(clock_divider(200_000_000, 400_000, false), 0x80);
        // At or above the base clock the card runs at the base clock.
        assert_eq!(clock_divider(25_000_000, TRANSFER_HZ, true), 0);
        assert_eq!(card_clock(25_000_000, 0), 25_000_000);
        // The power-of-two dividers before version 3.00.
        assert_eq!(clock_divider(52_000_000, TRANSFER_HZ, false), 2);
    }

    #[test]
    fn a_long_response_regains_its_crc_byte() {
        // The registers hold response bits 127-8; the protocol reads bit 0 in
        // word 0, so everything moves up by eight bits.
        let registers = [0x4433_2211, 0x8877_6655, 0xccbb_aa99, 0x00ff_eedd];
        assert_eq!(
            long_response(registers),
            [0x3322_1100, 0x7766_5544, 0xbbaa_9988, 0xffee_ddcc]
        );
    }

    #[test]
    fn adma2_descriptors() {
        // Valid, transfer, length in bits 31-16, address in the high word.
        assert_eq!(
            adma2_descriptor(0xc010_2000, 4096, false),
            0xc010_2000_1000_0021
        );
        // The last one ends the table.
        assert_eq!(
            adma2_descriptor(0x0000_1000, 512, true),
            0x0000_1000_0200_0023
        );
        // 65536 bytes are written as a length of zero.
        assert_eq!(
            adma2_descriptor(0x8000_0000, ADMA2_MAX_BYTES, true),
            0x8000_0000_0000_0023
        );
    }

    #[test]
    fn interrupt_errors() {
        let status = |raw| Interrupts::new_with_raw_value(raw);
        // Command timeout, data timeout.
        assert_eq!(status(1 << 16).bus_error(), BusError::Timeout);
        assert_eq!(status(1 << 20).bus_error(), BusError::Timeout);
        // Command CRC, data CRC.
        assert_eq!(status(1 << 17).bus_error(), BusError::Crc);
        assert_eq!(status(1 << 21).bus_error(), BusError::Crc);
        // An ADMA error is neither.
        assert_eq!(status(1 << 25).bus_error(), BusError::Failed);
        // The waited events: command and transfer complete, both buffer
        // ready events and every error; no card insertion or removal.
        assert_eq!(Interrupts::WAITED.raw_value(), 0x03ff_0033);
        assert!(!Interrupts::WAITED.any(status(0b1100_0000)));
    }
}

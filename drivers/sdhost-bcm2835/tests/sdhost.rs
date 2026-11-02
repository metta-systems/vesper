//! Host tests of the SD card protocol against a scripted card, and of the
//! SDHOST register values.

#[cfg(test)]
mod tests {
    use sdhost_bcm2835::{
        card::{
            ALL_SEND_CID, APP_CMD, BLOCK_BYTES, Bus, BusError, Card, CardError, Command,
            GO_IDLE_STATE, HCS, IDENTIFICATION_HZ, IF_COND, OCR_CCS, OCR_POWERED_UP,
            READ_SINGLE_BLOCK, SD_SEND_OP_COND, SELECT_CARD, SEND_CSD, SEND_IF_COND,
            SEND_RELATIVE_ADDR, SEND_STATUS, SET_BLOCKLEN, SET_BUS_WIDTH, STATE_TRANSFER, Step,
            TRANSFER_HZ, WRITE_BLOCK, csd_blocks,
        },
        card_clock, clock_divider, command_register,
    };

    /// A long response with `value` in bits `high..=low`.
    fn with_bits(mut words: [u32; 4], high: u32, low: u32, value: u64) -> [u32; 4] {
        for bit in low..=high {
            let word = &mut words[(bit / 32) as usize];
            let mask = 1 << (bit % 32);
            if (value >> (bit - low)) & 1 == 1 {
                *word |= mask;
            } else {
                *word &= !mask;
            }
        }
        words
    }

    /// A version 2.0 CSD of a card with `c_size`.
    fn csd_v2(c_size: u64) -> [u32; 4] {
        with_bits(with_bits([0; 4], 127, 126, 1), 69, 48, c_size)
    }

    /// A version 1.0 CSD.
    fn csd_v1(c_size: u64, multiplier: u64, block_length: u64) -> [u32; 4] {
        let csd = with_bits([0; 4], 73, 62, c_size);
        let csd = with_bits(csd, 49, 47, multiplier);
        with_bits(csd, 83, 80, block_length)
    }

    #[test]
    fn a_version_2_csd_counts_512_kib_units() {
        // An 8 GiB card: (C_SIZE + 1) * 512 KiB.
        assert_eq!(csd_blocks(&csd_v2(16383)), Some(16384 * 1024));
    }

    #[test]
    fn a_version_1_csd_multiplies_size_and_block_length() {
        // (C_SIZE + 1) * 2^(C_SIZE_MULT + 2) blocks of 2^READ_BL_LEN bytes:
        // 4096 * 128 * 512 bytes, 256 MiB.
        assert_eq!(csd_blocks(&csd_v1(4095, 5, 9)), Some(4096 * 128));
        // 1024-byte blocks count twice.
        assert_eq!(csd_blocks(&csd_v1(4095, 5, 10)), Some(4096 * 128 * 2));
    }

    #[test]
    fn an_unknown_csd_structure_has_no_size() {
        assert_eq!(csd_blocks(&with_bits([0; 4], 127, 126, 3)), None);
    }

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

    /// A card the tests script: it answers each command as the
    /// specification's state machine does, and records what the host did.
    #[allow(
        clippy::struct_excessive_bools,
        reason = "the switches of one scripted card"
    )]
    struct ScriptedCard {
        present: bool,
        version2: bool,
        high_capacity: bool,
        /// ACMD41 polls before the card is powered up
        busy_polls: u32,
        csd: [u32; 4],
        sent: Vec<(u8, u32)>,
        clocks: Vec<u32>,
        wide: bool,
        delayed_ms: u32,
        app_command: bool,
    }

    impl ScriptedCard {
        fn new(version2: bool, high_capacity: bool, csd: [u32; 4]) -> Self {
            Self {
                present: true,
                version2,
                high_capacity,
                busy_polls: 3,
                csd,
                sent: Vec::new(),
                clocks: Vec::new(),
                wide: false,
                delayed_ms: 0,
                app_command: false,
            }
        }

        fn indices(&self) -> Vec<u8> {
            self.sent.iter().map(|(index, _)| *index).collect()
        }
    }

    const RCA: u16 = 0xb368;
    /// The argument naming the card at [`RCA`].
    const RCA_ARGUMENT: u32 = (RCA as u32) << 16;

    impl Bus for ScriptedCard {
        fn command(&mut self, command: Command, argument: u32) -> Result<[u32; 4], BusError> {
            self.sent.push((command.index, argument));
            let app = core::mem::take(&mut self.app_command);
            if !self.present && command != GO_IDLE_STATE {
                return Err(BusError::Timeout);
            }
            let short = |word| Ok([word, 0, 0, 0]);
            let ready = (STATE_TRANSFER << 9) | (1 << 8);
            match (app, command) {
                (_, GO_IDLE_STATE) => Ok([0; 4]),
                (false, SEND_IF_COND) if self.version2 => short(argument & 0xfff),
                (false, SEND_IF_COND) => Err(BusError::Timeout),
                (_, APP_CMD) => {
                    self.app_command = true;
                    short(1 << 5)
                }
                (true, SD_SEND_OP_COND) => {
                    if self.busy_polls > 0 {
                        self.busy_polls -= 1;
                        return short(0x00ff_8000);
                    }
                    let ccs = if self.high_capacity && argument & HCS != 0 {
                        OCR_CCS
                    } else {
                        0
                    };
                    short(OCR_POWERED_UP | ccs | 0x00ff_8000)
                }
                (false, ALL_SEND_CID) => Ok([1, 2, 3, 4]),
                (false, SEND_RELATIVE_ADDR) => short(RCA_ARGUMENT),
                (false, SEND_CSD) if argument == RCA_ARGUMENT => Ok(self.csd),
                (false, SELECT_CARD) if argument == RCA_ARGUMENT => short(0),
                (false, SEND_STATUS) if argument == RCA_ARGUMENT => short(ready),
                (true, SET_BUS_WIDTH) if argument == 2 => short(ready),
                (false, SET_BLOCKLEN) if argument == BLOCK_BYTES => short(ready),
                _ => Err(BusError::Failed),
            }
        }

        fn set_clock(&mut self, hz: u32) {
            self.clocks.push(hz);
        }

        fn set_wide_bus(&mut self) {
            self.wide = true;
        }

        fn delay_ms(&mut self, milliseconds: u32) {
            self.delayed_ms += milliseconds;
        }
    }

    #[test]
    fn a_high_capacity_card_comes_up_addressed_in_blocks() {
        let mut bus = ScriptedCard::new(true, true, csd_v2(16383));
        let card = Card::bring_up(&mut bus).unwrap();
        assert_eq!(
            card,
            Card {
                rca: RCA,
                high_capacity: true,
                blocks: 16384 * 1024,
            }
        );
        // Identified slowly, then run at the default speed on four lines.
        assert_eq!(bus.clocks, [IDENTIFICATION_HZ, TRANSFER_HZ]);
        assert!(bus.wide);
        // The host asked for high capacity and polled while the card was busy.
        assert_eq!(bus.sent[1], (8, IF_COND));
        assert!(bus.sent.contains(&(41, 0x00ff_8000 | HCS)));
        assert_eq!(
            bus.delayed_ms, 30,
            "three busy polls, ten milliseconds apart"
        );
        // No block length for a high capacity card.
        assert!(!bus.indices().contains(&16));
        assert_eq!(card.argument(7), Some(7));
    }

    #[test]
    fn a_standard_capacity_card_is_addressed_in_bytes() {
        let mut bus = ScriptedCard::new(true, false, csd_v1(4095, 5, 9));
        let card = Card::bring_up(&mut bus).unwrap();
        assert!(!card.high_capacity);
        assert_eq!(card.blocks, 4096 * 128);
        assert!(
            bus.sent.contains(&(16, BLOCK_BYTES)),
            "the block length is set"
        );
        assert_eq!(card.argument(7), Some(7 * 512));
        // Byte addresses end at 4 GiB.
        assert_eq!(card.argument(1 << 23), None);
    }

    #[test]
    fn a_version_1_card_is_never_high_capacity() {
        // It ignores CMD8 and the HCS bit; a card claiming CCS is not trusted.
        let mut bus = ScriptedCard::new(false, true, csd_v1(4095, 5, 9));
        let card = Card::bring_up(&mut bus).unwrap();
        assert!(!card.high_capacity);
        assert!(
            bus.sent.contains(&(41, 0x00ff_8000)),
            "no HCS for a version 1 card"
        );
    }

    #[test]
    fn an_empty_slot_has_no_card() {
        let mut bus = ScriptedCard::new(true, true, csd_v2(1));
        bus.present = false;
        assert_eq!(Card::bring_up(&mut bus), Err(CardError::NoCard));
        // CMD0, CMD8 (silent like a version 1 card), then the first CMD55.
        assert_eq!(bus.indices(), [0, 8, 55]);
    }

    #[test]
    fn a_card_that_stays_busy_fails_its_power_up() {
        let mut bus = ScriptedCard::new(true, true, csd_v2(1));
        bus.busy_polls = u32::MAX;
        assert_eq!(
            Card::bring_up(&mut bus),
            Err(CardError::Failed {
                step: Step::PowerUp,
                error: BusError::Timeout
            })
        );
        assert_eq!(bus.delayed_ms, 1000, "a second of polls");
    }

    #[test]
    fn a_wrong_echo_fails_the_interface_condition() {
        struct WrongEcho(ScriptedCard);
        impl Bus for WrongEcho {
            fn command(&mut self, command: Command, argument: u32) -> Result<[u32; 4], BusError> {
                if command == SEND_IF_COND {
                    return Ok([0x155, 0, 0, 0]);
                }
                self.0.command(command, argument)
            }
            fn set_clock(&mut self, hz: u32) {
                self.0.set_clock(hz);
            }
            fn set_wide_bus(&mut self) {
                self.0.set_wide_bus();
            }
            fn delay_ms(&mut self, milliseconds: u32) {
                self.0.delay_ms(milliseconds);
            }
        }
        let mut bus = WrongEcho(ScriptedCard::new(true, true, csd_v2(1)));
        assert_eq!(
            Card::bring_up(&mut bus),
            Err(CardError::Failed {
                step: Step::InterfaceCondition,
                error: BusError::Failed
            })
        );
    }
}

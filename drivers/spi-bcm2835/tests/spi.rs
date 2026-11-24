//! Host tests of the SPI0 divisor and the polled write sequence.

#[cfg(test)]
mod tests {
    use {
        spi_bcm2835::{
            Bus, ControlStatus, MAX_DIVISOR, Mode, RegisterBlock, TimedOut, clock_register,
            divisor, idle, rate, set_rate, write,
        },
        std::collections::VecDeque,
    };

    /// Raw `CS` bits (table 160).
    const TA: u32 = 1 << 7;
    const CLEAR_BOTH: u32 = 0b11 << 4;
    const READ_ENABLE: u32 = 1 << 12;
    const DONE: u32 = 1 << 16;
    const RXD: u32 = 1 << 17;
    const TXD: u32 = 1 << 18;

    #[test]
    fn the_divisor_never_runs_sclk_above_the_asked_rate() {
        // The Raspberry Pi 4's core clock at its fastest (500 MHz): 32 MHz
        // asks for at least 15.6, the next even divisor is 16, 31.25 MHz.
        assert_eq!(divisor(500_000_000, 32_000_000), 16);
        assert_eq!(rate(500_000_000, 16), 31_250_000);
        // 10 MHz (the ILI9341's write cycle, 100 ns) divides 500 MHz exactly.
        assert_eq!(divisor(500_000_000, 10_000_000), 50);
        // An odd quotient rounds up to the next even divisor.
        assert_eq!(divisor(350_000_000, 50_000_000), 8);
        // Faster than the core clock allows: the smallest divisor, 2.
        assert_eq!(divisor(500_000_000, 1_000_000_000), 2);
        // Slower than the largest divisor reaches: 65536, written as 0.
        assert_eq!(divisor(500_000_000, 1_000), MAX_DIVISOR);
        assert_eq!(clock_register(MAX_DIVISOR), 0);
        assert_eq!(clock_register(16), 16);
        for max_hz in (100_000..=125_000_000).step_by(1_234_567) {
            let divisor = divisor(500_000_000, max_hz);
            assert!(divisor.is_multiple_of(2));
            assert!(rate(500_000_000, divisor) <= max_hz, "{max_hz} Hz");
        }
    }

    #[test]
    fn the_modes_set_cpol_and_cpha() {
        let raw = |number| idle(Mode::numbered(number)).raw_value();
        assert_eq!(raw(0), READ_ENABLE);
        assert_eq!(raw(1), READ_ENABLE | 1 << 2, "CPHA");
        assert_eq!(raw(2), READ_ENABLE | 1 << 3, "CPOL");
        assert_eq!(raw(3), READ_ENABLE | 1 << 2 | 1 << 3);
    }

    /// A stand-in for the block: a TX FIFO of `room` bytes the "wire" empties
    /// one byte per status read, each byte out bringing one byte in.
    struct Wire {
        room: usize,
        control: ControlStatus,
        tx: VecDeque<u8>,
        rx: VecDeque<u8>,
        sent: Vec<u8>,
        writes: Vec<u32>,
        /// Status reads that find the block stuck (no room, never done)
        stuck: bool,
    }

    impl Wire {
        fn new(room: usize) -> Self {
            Self {
                room,
                control: ControlStatus::ZERO,
                tx: VecDeque::new(),
                rx: VecDeque::new(),
                sent: Vec::new(),
                writes: Vec::new(),
                stuck: false,
            }
        }
    }

    impl Bus for Wire {
        fn control_status(&mut self) -> ControlStatus {
            if self.stuck {
                return ControlStatus::new_with_raw_value(self.control.raw_value());
            }
            if self.control.transfer_active()
                && let Some(byte) = self.tx.pop_front()
            {
                self.sent.push(byte);
                self.rx.push_back(!byte);
            }
            let mut raw = self.control.raw_value();
            if self.tx.len() < self.room {
                raw |= TXD;
            }
            if !self.rx.is_empty() {
                raw |= RXD;
            }
            if self.tx.is_empty() {
                raw |= DONE;
            }
            ControlStatus::new_with_raw_value(raw)
        }

        fn set_control_status(&mut self, value: ControlStatus) {
            assert!(
                !value.done() && !value.tx_room(),
                "status bits are read-only"
            );
            self.writes.push(value.raw_value());
            if value.raw_value() & CLEAR_BOTH != 0 {
                self.tx.clear();
                self.rx.clear();
            }
            self.control = value.with_clear(arbitrary_int::u2::new(0));
        }

        fn push(&mut self, byte: u8) {
            assert!(
                self.control.transfer_active(),
                "a byte outside the transfer"
            );
            assert!(self.tx.len() < self.room, "a byte into a full FIFO");
            self.tx.push_back(byte);
        }

        fn pop(&mut self) -> u8 {
            self.rx.pop_front().expect("RXD said a byte was in")
        }
    }

    #[test]
    fn a_write_clocks_every_byte_out_in_order_and_drains_the_rx_fifo() {
        // A FIFO of two bytes, so the sequence must wait for room.
        let mut wire = Wire::new(2);
        let bytes: Vec<u8> = (0..40).collect();
        assert_eq!(
            write(&mut wire, Mode::numbered(0), bytes.iter().copied(), || true),
            Ok(())
        );
        assert_eq!(wire.sent, bytes);
        assert!(wire.rx.is_empty(), "every byte in was read");
        // Clear both FIFOs with TA clear, set TA, and clear it at the end.
        assert_eq!(
            wire.writes,
            [READ_ENABLE | CLEAR_BOTH, READ_ENABLE | TA, READ_ENABLE]
        );
    }

    #[test]
    fn a_stuck_controller_times_out_and_still_ends_the_transfer() {
        let mut wire = Wire::new(2);
        wire.stuck = true;
        let mut waits = 0;
        assert_eq!(
            write(&mut wire, Mode::numbered(3), [1, 2, 3], || {
                waits += 1;
                waits < 5
            }),
            Err(TimedOut)
        );
        assert_eq!(waits, 5);
        assert_eq!(
            wire.writes.last(),
            Some(&(READ_ENABLE | 1 << 2 | 1 << 3)),
            "TA cleared, the mode kept"
        );
        assert!(wire.sent.is_empty());
    }

    #[test]
    fn an_empty_write_only_starts_and_ends_the_transfer() {
        let mut wire = Wire::new(2);
        assert_eq!(write(&mut wire, Mode::numbered(0), [], || true), Ok(()));
        assert_eq!(wire.writes.len(), 3);
        assert!(wire.sent.is_empty());
    }

    /// Memory standing in for the block's six registers.
    #[repr(C, align(4))]
    struct Block([u32; 6]);

    #[test]
    fn the_divisor_lands_in_clk() {
        let mut block = Block([0; 6]);
        {
            // SAFETY: `block` outlives the handle and nothing else touches it.
            let mut registers =
                unsafe { RegisterBlock::new_mmio(block.0.as_mut_ptr().cast::<RegisterBlock>()) };
            assert_eq!(
                set_rate(&mut registers, 500_000_000, 32_000_000),
                31_250_000
            );
        }
        assert_eq!(block.0[2], 16, "CLK at 0x08");
        assert!(
            block
                .0
                .iter()
                .enumerate()
                .all(|(index, &word)| index == 2 || word == 0)
        );
    }
}

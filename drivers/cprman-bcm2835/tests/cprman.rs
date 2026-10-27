//! Host tests of the clock manager's divisor choice and register sequence.

#[cfg(test)]
mod tests {
    use {
        arbitrary_int::u12,
        cprman_bcm2835::{
            ClockControl, ClockDivisor, ClockSource, Generator, Mash, PASSWORD, PWM_CLOCK,
            PwmGenerator, RegisterBlock, Setting, StillBusy, choose, program, rate, stop,
        },
    };

    const PASSWORD_BITS: u32 = (PASSWORD as u32) << 24;

    /// The rates of the table in `doc/agentic/audio-design.md` (Sample rate and
    /// resolution): 100 MHz is PLLD per / 5, 19.2 MHz the oscillator itself,
    /// 250 MHz PLLD per / 2.
    #[test]
    fn the_design_rates_have_integer_divisors() {
        assert_eq!(
            choose(100_000_000),
            Some(Setting {
                source: ClockSource::PlldPer,
                divisor: 5
            })
        );
        assert_eq!(
            choose(19_200_000),
            Some(Setting {
                source: ClockSource::Oscillator,
                divisor: 1
            })
        );
        assert_eq!(
            choose(250_000_000),
            Some(Setting {
                source: ClockSource::PlldPer,
                divisor: 2
            })
        );
        // The device tree's default PWM rate.
        assert_eq!(
            choose(10_000_000).map(|setting| setting.hz()),
            Some(10_000_000)
        );
    }

    #[test]
    fn rates_no_integer_divisor_reaches_are_refused() {
        assert_eq!(choose(0), None);
        // 500 MHz / 10 and / 11 miss 48 MHz by 4 % and 5 %, the oscillator by far.
        assert_eq!(choose(48_000_000), None);
        // Above every source.
        assert_eq!(choose(600_000_000), None);
        // Below 19.2 MHz / 4095 nothing divides far enough.
        assert_eq!(choose(1_000), None);
    }

    #[test]
    fn the_chosen_rate_is_within_a_thousandth() {
        for hz in (1_000_000..=125_000_000).step_by(997_331) {
            if let Some(setting) = choose(hz) {
                assert!(u64::from(setting.hz().abs_diff(hz)) * 1000 <= u64::from(hz));
                assert!((1..=4095).contains(&setting.divisor));
            }
        }
    }

    #[test]
    fn rate_reads_back_source_and_divisor() {
        let control = ClockControl::ZERO
            .with_source(ClockSource::PlldPer)
            .with_enable(true);
        let divisor = ClockDivisor::ZERO.with_integer(u12::new(5));
        assert_eq!(rate(control, divisor), 100_000_000);
        assert_eq!(rate(control.with_enable(false), divisor), 0, "stopped");
        assert_eq!(
            rate(control.with_source(ClockSource::PllcPer), divisor),
            0,
            "a source of unknown rate"
        );
        assert_eq!(rate(control, ClockDivisor::ZERO), 0, "no divisor");
        // The fraction counts with a MASH divider only: 5.5 gives 90.9 MHz.
        let half = divisor.with_fraction(u12::new(2048));
        assert_eq!(rate(control, half), 100_000_000);
        assert_eq!(rate(control.with_mash(Mash::OneStage), half), 90_909_090);
    }

    #[test]
    fn the_register_layout_matches_the_datasheet() {
        let control = ClockControl::ZERO
            .with_source(ClockSource::PlldPer)
            .with_enable(true)
            .with_mash(Mash::ThreeStage)
            .with_password(PASSWORD);
        assert_eq!(control.raw_value(), PASSWORD_BITS | 0x600 | 0x10 | 6);
        let divisor = ClockDivisor::ZERO
            .with_integer(u12::new(5))
            .with_fraction(u12::new(0x123))
            .with_password(PASSWORD);
        assert_eq!(divisor.raw_value(), PASSWORD_BITS | (5 << 12) | 0x123);
        assert!(ClockControl::new_with_raw_value(0x80).busy());
        assert_eq!(PWM_CLOCK, 30);
    }

    /// A generator recording every write, whose `BUSY` stays set for
    /// `busy_reads` reads after `ENAB` clears.
    struct Recorder {
        control: ClockControl,
        divisor: ClockDivisor,
        busy_reads: u32,
        writes: Vec<u32>,
    }

    impl Recorder {
        fn running() -> Self {
            Self {
                control: ClockControl::new_with_raw_value(0x80 | 0x10 | 1),
                divisor: ClockDivisor::ZERO.with_integer(u12::new(1)),
                busy_reads: 2,
                writes: Vec::new(),
            }
        }
    }

    impl Generator for Recorder {
        fn control(&mut self) -> ClockControl {
            if !self.control.enable() {
                if self.busy_reads == 0 {
                    self.control =
                        ClockControl::new_with_raw_value(self.control.raw_value() & !0x80);
                } else {
                    self.busy_reads -= 1;
                }
            }
            self.control
        }

        fn set_control(&mut self, value: ClockControl) {
            assert_eq!(value.raw_value() & 0xff00_0000, PASSWORD_BITS);
            self.writes.push(value.raw_value());
            // `BUSY` is read-only: the write keeps the hardware's value.
            let busy = self.control.raw_value() & 0x80;
            self.control = ClockControl::new_with_raw_value((value.raw_value() & !0x80) | busy);
            if value.enable() {
                self.control = ClockControl::new_with_raw_value(self.control.raw_value() | 0x80);
            }
        }

        fn divisor(&mut self) -> ClockDivisor {
            self.divisor
        }

        fn set_divisor(&mut self, value: ClockDivisor) {
            assert!(!self.control.busy(), "the divisor changed while BUSY");
            assert_eq!(value.raw_value() & 0xff00_0000, PASSWORD_BITS);
            self.writes.push(value.raw_value());
            self.divisor = value;
        }
    }

    #[test]
    fn program_follows_the_datasheet_sequence() {
        let mut generator = Recorder::running();
        let mut waits = 0;
        let setting = choose(100_000_000).unwrap();
        program(&mut generator, setting, true, || {
            waits += 1;
            true
        })
        .unwrap();
        assert_eq!(waits, 2, "waited while BUSY was set");
        assert_eq!(
            generator.writes,
            [
                // Stop: ENAB clear, source kept.
                PASSWORD_BITS | 1,
                // DIVI 5 while stopped.
                PASSWORD_BITS | (5 << 12),
                // PLLD per, integer division, ENAB still clear.
                PASSWORD_BITS | 6,
                // Then ENAB alone.
                PASSWORD_BITS | 0x10 | 6,
            ]
        );
        assert_eq!(generator.rate(), 100_000_000);
    }

    #[test]
    fn writes_never_carry_busy_or_kill() {
        let mut generator = Recorder::running();
        // FLIP and 3-stage MASH survive a stop; BUSY (read-only) and KILL do not.
        generator.control =
            ClockControl::new_with_raw_value(0x600 | 0x100 | 0x80 | 0x20 | 0x10 | 1);
        stop(&mut generator, || true).unwrap();
        assert_eq!(generator.writes, [PASSWORD_BITS | 0x600 | 0x100 | 1]);
    }

    #[test]
    fn program_can_leave_the_generator_stopped() {
        let mut generator = Recorder::running();
        program(&mut generator, choose(19_200_000).unwrap(), false, || true).unwrap();
        assert_eq!(generator.writes.last(), Some(&(PASSWORD_BITS | 1)));
        assert_eq!(generator.rate(), 0);
    }

    #[test]
    fn a_generator_that_stays_busy_is_left_alone() {
        let mut generator = Recorder::running();
        generator.busy_reads = u32::MAX;
        let mut waits = 0;
        assert_eq!(
            program(&mut generator, choose(100_000_000).unwrap(), true, || {
                waits += 1;
                waits < 10
            }),
            Err(StillBusy)
        );
        assert_eq!(
            generator.writes,
            [PASSWORD_BITS | 1],
            "only the stop request"
        );
        assert_eq!(stop(&mut generator, || false), Err(StillBusy));
    }

    /// Memory standing in for the manager's first 0xa8 bytes.
    #[repr(C, align(4))]
    struct Block([u32; 42]);

    #[test]
    fn the_pwm_generator_is_at_0xa0() {
        let mut block = Block([0; 42]);
        {
            // SAFETY: `block` outlives the handle and nothing else touches it.
            let mut registers =
                unsafe { RegisterBlock::new_mmio(block.0.as_mut_ptr().cast::<RegisterBlock>()) };
            let mut generator = PwmGenerator(&mut registers);
            program(&mut generator, choose(100_000_000).unwrap(), true, || true).unwrap();
        }
        assert_eq!(block.0[40], PASSWORD_BITS | 0x10 | 6, "CM_PWMCTL");
        assert_eq!(block.0[41], PASSWORD_BITS | (5 << 12), "CM_PWMDIV");
        assert!(block.0[..40].iter().all(|&word| word == 0));
    }
}

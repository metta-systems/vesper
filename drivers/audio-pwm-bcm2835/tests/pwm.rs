//! Host tests of the PWM register values, FIFO words and test tone.

#[cfg(test)]
mod tests {
    use audio_pwm_bcm2835::{
        Control, DmaConfig, MEMORY_DREQ, PERIOD_BYTES, PERIOD_FRAMES, PERIODS, PWM_CLOCK_HZ,
        PWM_DREQ, PeriodContent, RANGE, RATE_TOLERANCE_PPM, RegisterBlock, SINE, Side, Status,
        Tone, clear_errors, clearing, deviation_ppm, dma_feed, fifo_stereo, frame, has_errors,
        measured_rate_millihertz, period_content, sample_rate_millihertz, word,
    };

    #[test]
    fn control_values_match_the_datasheet() {
        // PWEN1 0, USEF1 5, PWEN2 8, USEF2 13; MSEN1 7 and MSEN2 15 clear (D4).
        assert_eq!(fifo_stereo().raw_value(), 0x2121);
        // CLRF1 is bit 6.
        assert_eq!(clearing().raw_value(), 0x40);
        let all = Control::ZERO
            .with_mark_space1(true)
            .with_mark_space2(true)
            .with_repeat_last1(true)
            .with_repeat_last2(true);
        assert_eq!(all.raw_value(), 0x8484);
    }

    #[test]
    fn status_flags_match_the_datasheet() {
        // WERR1 2, RERR1 3, GAPO1..4 4 to 7, BERR 8.
        assert_eq!(clear_errors().raw_value(), 0x1fc);
        let status = Status::new_with_raw_value(0x3);
        assert!(status.fifo_full() && status.fifo_empty());
        assert!(!has_errors(status));
        assert!(has_errors(Status::new_with_raw_value(0x100)));
        assert!(
            !has_errors(Status::new_with_raw_value(0x600)),
            "STA1 and STA2 only"
        );
        let dma = DmaConfig::ZERO
            .with_enable(true)
            .with_panic_threshold(7)
            .with_request_threshold(7);
        assert_eq!(dma.raw_value(), 0x8000_0707);
    }

    /// Memory standing in for the block's 0x28 bytes.
    #[repr(C, align(4))]
    struct Block([u32; 10]);

    #[test]
    fn registers_sit_at_their_offsets() {
        let mut block = Block([0; 10]);
        {
            // SAFETY: `block` outlives the handle and nothing else touches it.
            let mut registers =
                unsafe { RegisterBlock::new_mmio(block.0.as_mut_ptr().cast::<RegisterBlock>()) };
            registers.write_range1(RANGE);
            registers.write_range2(RANGE + 1);
            registers.write_fifo(0xf1f0);
            registers.write_data1(0xd1);
            registers.write_data2(0xd2);
        }
        assert_eq!(block.0[4], RANGE, "RNG1 at 0x10");
        assert_eq!(block.0[5], 0xd1, "DAT1 at 0x14");
        assert_eq!(block.0[6], 0xf1f0, "FIF1 at 0x18");
        assert_eq!(block.0[8], RANGE + 1, "RNG2 at 0x20");
        assert_eq!(block.0[9], 0xd2, "DAT2 at 0x24");
    }

    #[test]
    fn the_rate_is_44092_hz() {
        assert_eq!(sample_rate_millihertz(PWM_CLOCK_HZ, RANGE), 44_091_710);
    }

    #[test]
    fn words_span_the_range() {
        assert_eq!(word(i16::MIN, RANGE), 0);
        assert_eq!(word(0, RANGE), RANGE / 2);
        assert_eq!(word(i16::MAX, RANGE), RANGE - 1);
        let mut previous = 0;
        for sample in (i16::MIN..=i16::MAX).step_by(97) {
            let value = word(sample, RANGE);
            assert!(value >= previous && value < RANGE);
            previous = value;
        }
    }

    #[test]
    fn frames_put_right_first_and_silence_on_the_muted_side() {
        let silence = RANGE / 2;
        let loud = word(i16::MAX, RANGE);
        assert_eq!(frame(Side::Left, i16::MAX, RANGE), [silence, loud]);
        assert_eq!(frame(Side::Right, i16::MAX, RANGE), [loud, silence]);
        assert_eq!(frame(Side::Both, i16::MAX, RANGE), [loud, loud]);
    }

    #[test]
    fn the_sine_table_is_one_period() {
        assert_eq!(SINE[0], 0);
        assert_eq!(SINE[64], i16::MAX);
        assert_eq!(SINE[128], 0);
        assert_eq!(SINE[192], -i16::MAX);
        for index in 1..128 {
            assert_eq!(SINE[index], SINE[128 - index], "symmetric about the peak");
            assert_eq!(SINE[index], -SINE[128 + index], "odd about the middle");
            assert!(SINE[index] > 0);
        }
        // Within 0.2 % of full scale of the true sine.
        for (index, &value) in (0_u8..=255).zip(SINE.iter()) {
            let exact =
                (f64::from(index) * core::f64::consts::TAU / 256.0).sin() * f64::from(i16::MAX);
            assert!((f64::from(value) - exact).abs() < 0.002 * f64::from(i16::MAX));
        }
    }

    #[test]
    fn the_tone_runs_at_440_hz() {
        let rate = sample_rate_millihertz(PWM_CLOCK_HZ, RANGE);
        let tone = Tone::new(440, rate, 0);
        // Cycles per second: step / 2^32 per sample, `rate` / 1000 samples.
        let millihertz = (u64::from(tone.step()) * rate) >> 32;
        assert!(millihertz.abs_diff(440_000) < 2, "{millihertz} mHz");
        let mut tone = Tone::new(440, rate, 2);
        let peak = (0..44_092).map(|_| tone.next_sample()).max().unwrap();
        assert!(
            peak > i16::MAX / 4 - 300 && peak <= i16::MAX / 4,
            "a quarter of full scale"
        );
    }

    #[test]
    fn the_dma_ring_is_four_pages_fed_by_dreq_5() {
        assert_eq!(PWM_DREQ, 5);
        assert_eq!(MEMORY_DREQ, 0);
        assert_eq!(PERIOD_FRAMES, 512);
        assert_eq!(PERIODS, 4);
        assert_eq!(PERIOD_BYTES, 4096);
        // §9.6 DMAC: ENAB 31, PANIC 15:8 = 7, DREQ 7:0 = 7.
        assert_eq!(dma_feed().raw_value(), 0x8000_0707);
    }

    #[test]
    fn the_ring_test_ramps_up_plays_ramps_down_then_goes_quiet() {
        let contents: Vec<_> = (0..6).map(|index| period_content(index, 3)).collect();
        assert_eq!(
            contents,
            [
                PeriodContent::RampUp,
                PeriodContent::Tone,
                PeriodContent::Tone,
                PeriodContent::Tone,
                PeriodContent::RampDown,
                PeriodContent::Off,
            ]
        );
    }

    #[test]
    fn the_measured_rate_is_checked_to_a_thousandth() {
        let expected = sample_rate_millihertz(PWM_CLOCK_HZ, RANGE);
        assert_eq!(expected, 44_091_710, "100 MHz / 2268");
        // 430 periods of 512 frames in exactly the time they should take.
        let frames = 430 * 512;
        let elapsed_ns = frames * 1_000_000_000_000 / expected;
        let measured = measured_rate_millihertz(frames, elapsed_ns);
        assert!(deviation_ppm(measured, expected) < 2);
        // A PLLD of 498 MHz instead of 500 shows up as 4000 ppm.
        let slow = expected * 498 / 500;
        assert_eq!(deviation_ppm(slow, expected), 4000);
        assert!(deviation_ppm(slow, expected) > RATE_TOLERANCE_PPM);
        assert_eq!(RATE_TOLERANCE_PPM, 1000, "0.1 %");
    }
}

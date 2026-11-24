//! Host tests of the GPIO function select values.

#[cfg(test)]
mod tests {
    use {
        gpio::gpio_v1::{PinFunction, PinPull},
        gpio_bcm2835::{
            Chip, FunctionSelect, RegisterBlock, apply_pull, level, place, select, set_level,
        },
    };

    /// Words of the block's registers: 0xf4 bytes.
    const BLOCK_WORDS: usize = 61;

    /// Word indices of `GPSET0`, `GPCLR0` and `GPLEV0` (offsets 0x1c, 0x28 and
    /// 0x34, BCM2711 ARM Peripherals, section 5.2).
    const SET_WORD: usize = 0x1c / 4;
    const CLEAR_WORD: usize = 0x28 / 4;
    const LEVEL_WORD: usize = 0x34 / 4;

    #[test]
    fn a_level_is_one_write_to_the_set_or_clear_bank() {
        let mut block = Block([0; BLOCK_WORDS]);
        // GPIO 23 and 24 read high, 25 low.
        block.0[LEVEL_WORD] = (1 << 23) | (1 << 24);
        {
            // SAFETY: `block` outlives the handle and nothing else touches it.
            let mut registers =
                unsafe { RegisterBlock::new_mmio(block.0.as_mut_ptr().cast::<RegisterBlock>()) };
            // The panel's backlight on (GPIO 23), reset low (GPIO 24), and a pin
            // of the second bank (GPIO 41) high.
            assert_eq!(set_level(&mut registers, Chip::Bcm2711, 23, true), Some(()));
            assert_eq!(
                set_level(&mut registers, Chip::Bcm2711, 24, false),
                Some(())
            );
            assert_eq!(set_level(&mut registers, Chip::Bcm2711, 41, true), Some(()));
            assert_eq!(set_level(&mut registers, Chip::Bcm2711, 58, true), None);
            assert_eq!(set_level(&mut registers, Chip::Bcm2835, 54, true), None);
            assert_eq!(level(&mut registers, Chip::Bcm2711, 23), Some(true));
            assert_eq!(level(&mut registers, Chip::Bcm2711, 25), Some(false));
            assert_eq!(level(&mut registers, Chip::Bcm2711, 58), None);
        }
        // Each write names one pin; the last write to a bank is what the
        // memory holds.
        assert_eq!(block.0[SET_WORD], 1 << 23);
        assert_eq!(block.0[SET_WORD + 1], 1 << (41 - 32));
        assert_eq!(block.0[CLEAR_WORD], 1 << 24);
        assert_eq!(block.0[CLEAR_WORD + 1], 0);
    }

    /// Memory standing in for the block's registers.
    #[repr(C, align(4))]
    struct Block([u32; BLOCK_WORDS]);

    /// Word index of `GPIO_PUP_PDN_CNTRL_REG0` (offset 0xe4).
    const PULL_RESISTOR_CONTROL_WORD: usize = 0xe4 / 4;

    /// `GPPUD`, `GPPUDCLK0` and `GPPUDCLK1` at each wait of a pull change on
    /// `pin`, and after it.
    fn pull_steps(pin: u32, pull: PinPull) -> Option<Vec<[u32; 3]>> {
        let mut block = Block([0; BLOCK_WORDS]);
        // SAFETY: `block` outlives the handle and nothing else touches it.
        let mut registers =
            unsafe { RegisterBlock::new_mmio(block.0.as_mut_ptr().cast::<RegisterBlock>()) };
        let mut steps = Vec::new();
        let state = |registers: &mut gpio_bcm2835::MmioRegisterBlock<'_>| {
            [
                registers.read_pull_control().raw_value(),
                registers.read_pull_clock(0).unwrap(),
                registers.read_pull_clock(1).unwrap(),
            ]
        };
        apply_pull(&mut registers, Chip::Bcm2835, pin, pull, |registers| {
            steps.push(state(registers));
        })?;
        steps.push(state(&mut registers));
        Some(steps)
    }

    /// The four `GPIO_PUP_PDN_CNTRL_REGn` after the BCM2711 pull changes
    /// `pulls`, starting from `initial`, and how many waits ran.
    fn bcm2711_pulls(initial: [u32; 4], pulls: &[(u32, PinPull)]) -> Option<([u32; 4], usize)> {
        let mut block = Block([0; BLOCK_WORDS]);
        block.0[PULL_RESISTOR_CONTROL_WORD..PULL_RESISTOR_CONTROL_WORD + 4]
            .copy_from_slice(&initial);
        let mut waits = 0;
        {
            // SAFETY: `block` outlives the handle and nothing else touches it.
            let mut registers =
                unsafe { RegisterBlock::new_mmio(block.0.as_mut_ptr().cast::<RegisterBlock>()) };
            for &(pin, pull) in pulls {
                apply_pull(&mut registers, Chip::Bcm2711, pin, pull, |_| waits += 1)?;
            }
        }
        let mut controls = [0; 4];
        controls
            .copy_from_slice(&block.0[PULL_RESISTOR_CONTROL_WORD..PULL_RESISTOR_CONTROL_WORD + 4]);
        Some((controls, waits))
    }

    #[test]
    fn a_bcm2711_pull_is_its_pin_field_at_once() {
        // Table 90: two bits per pin, sixteen pins per register, 01 up, 10
        // down. GPIO 40 and 41 are fields 8 and 9 of REG2.
        assert_eq!(
            bcm2711_pulls([0; 4], &[(40, PinPull::Up), (41, PinPull::Down)]),
            Some(([0, 0, 0b10_01 << 16, 0], 0))
        );
        // Pin 15 is the top field of REG0, pin 57 the last pin of REG3.
        assert_eq!(
            bcm2711_pulls([0; 4], &[(15, PinPull::Down), (57, PinPull::Up)]),
            Some(([0b10 << 30, 0, 0, 0b01 << 18], 0))
        );
        // Off clears the field and keeps its neighbours (the reset value is
        // a pull-down on most pins: 0b10 in every field).
        let reset = 0xaaaa_aaaa;
        assert_eq!(
            bcm2711_pulls([reset; 4], &[(14, PinPull::Off)]),
            Some(([reset & !(0b11 << 28), reset, reset, reset], 0))
        );
        assert_eq!(bcm2711_pulls([0; 4], &[(58, PinPull::Up)]), None);
    }

    #[test]
    fn the_chips_differ_in_their_pins() {
        assert_eq!(Chip::from_compatible_index(0), Some(Chip::Bcm2835));
        assert_eq!(Chip::from_compatible_index(1), Some(Chip::Bcm2711));
        assert_eq!(Chip::from_compatible_index(2), None);
        assert_eq!(place(Chip::Bcm2835, 53), Some((5, 3)));
        assert_eq!(place(Chip::Bcm2835, 54), None);
        assert_eq!(place(Chip::Bcm2711, 57), Some((5, 7)));
        assert_eq!(place(Chip::Bcm2711, 58), None);
    }

    #[test]
    fn a_pull_is_latched_by_the_pin_clock_then_both_clear() {
        assert_eq!(
            pull_steps(35, PinPull::Up),
            Some(vec![[0b10, 0, 0], [0b10, 0, 1 << 3], [0, 0, 0]])
        );
        assert_eq!(
            pull_steps(4, PinPull::Down),
            Some(vec![[0b01, 0, 0], [0b01, 1 << 4, 0], [0, 0, 0]])
        );
        assert_eq!(
            pull_steps(53, PinPull::Off),
            Some(vec![[0, 0, 0], [0, 0, 1 << 21], [0, 0, 0]])
        );
        assert_eq!(pull_steps(Chip::Bcm2835.pins(), PinPull::Up), None);
    }

    #[test]
    fn device_tree_pull_codes_name_the_same_pulls() {
        for code in 0..3 {
            let pull = PinPull::from_code(code).expect("a pull code");
            assert_eq!(u32::from(pull.code()), code);
        }
        assert_eq!(PinPull::from_code(2), Some(PinPull::Up));
        assert_eq!(PinPull::from_code(3), None);
    }

    #[test]
    fn pins_map_to_their_register_and_slot() {
        let place = |pin| place(Chip::Bcm2835, pin);
        assert_eq!(place(0), Some((0, 0)));
        assert_eq!(place(9), Some((0, 9)));
        assert_eq!(place(10), Some((1, 0)));
        assert_eq!(place(48), Some((4, 8)));
        assert_eq!(place(53), Some((5, 3)));
        assert_eq!(place(Chip::Bcm2835.pins()), None);
        assert_eq!(place(u32::MAX), None);
    }

    #[test]
    fn function_codes_follow_the_manual() {
        let code = |function| select(FunctionSelect::ZERO, 0, function).raw_value();
        assert_eq!(code(PinFunction::Input), 0b000);
        assert_eq!(code(PinFunction::Output), 0b001);
        assert_eq!(code(PinFunction::Alt0), 0b100);
        assert_eq!(code(PinFunction::Alt1), 0b101);
        assert_eq!(code(PinFunction::Alt2), 0b110);
        assert_eq!(code(PinFunction::Alt3), 0b111);
        assert_eq!(code(PinFunction::Alt4), 0b011);
        assert_eq!(code(PinFunction::Alt5), 0b010);
    }

    #[test]
    fn device_tree_codes_name_the_same_functions() {
        for code in 0..8 {
            let function = PinFunction::from_select_code(code).expect("a function select code");
            assert_eq!(u32::from(function.select_code()), code);
        }
        // The SDHOST pin group's `brcm,function = <4>`.
        assert_eq!(PinFunction::from_select_code(4), Some(PinFunction::Alt0));
        assert_eq!(PinFunction::from_select_code(8), None);
        assert_eq!(PinFunction::from_select_code(u32::MAX), None);
    }

    #[test]
    fn selecting_one_pin_keeps_the_others() {
        // GPIO 48 and 49 are the last two pins of GPFSEL4 (bits 24 to 29).
        let all_outputs = FunctionSelect::new_with_raw_value(0x0924_9249);
        let switched = select(all_outputs, 8, PinFunction::Alt0);
        assert_eq!(switched.raw_value(), 0x0c24_9249);
        let switched = select(switched, 9, PinFunction::Alt0);
        assert_eq!(switched.raw_value(), 0x2424_9249);
    }
}

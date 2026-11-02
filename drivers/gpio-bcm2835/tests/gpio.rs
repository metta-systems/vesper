//! Host tests of the GPIO function select values.

#[cfg(test)]
mod tests {
    use {
        gpio::gpio_v1::{PinFunction, PinPull},
        gpio_bcm2835::{FunctionSelect, PINS, RegisterBlock, apply_pull, place, select},
    };

    /// Memory standing in for the block's registers: 0xa0 bytes.
    #[repr(C, align(4))]
    struct Block([u32; 40]);

    /// `GPPUD`, `GPPUDCLK0` and `GPPUDCLK1` at each wait of a pull change on
    /// `pin`, and after it.
    fn pull_steps(pin: u32, pull: PinPull) -> Option<Vec<[u32; 3]>> {
        let mut block = Block([0; 40]);
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
        apply_pull(&mut registers, pin, pull, |registers| {
            steps.push(state(registers));
        })?;
        steps.push(state(&mut registers));
        Some(steps)
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
        assert_eq!(pull_steps(PINS, PinPull::Up), None);
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
        assert_eq!(place(0), Some((0, 0)));
        assert_eq!(place(9), Some((0, 9)));
        assert_eq!(place(10), Some((1, 0)));
        assert_eq!(place(48), Some((4, 8)));
        assert_eq!(place(53), Some((5, 3)));
        assert_eq!(place(PINS), None);
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

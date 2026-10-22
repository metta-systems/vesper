#![no_std]

//! The GPIO block of the BCM2835 (`brcm,bcm2835-gpio`): the function select
//! registers that connect each pin to an input, an output or one of its
//! alternate functions.
//!
//! Resources: BCM2835 ARM Peripherals, section 6.1 (`GPFSEL0` to `GPFSEL5`:
//! three bits per pin, ten pins per register) and table 6-31 (the alternate
//! functions of each pin).

use {
    arbitrary_int::{u2, u3},
    bitbybit::bitfield,
    derive_mmio::Mmio,
    gpio::gpio_v1::{PinFunction, PinPull},
    libmmio::assert_register_offsets,
};

/// Pins of the block.
pub const PINS: u32 = 54;

/// Pins of one function select register.
const PINS_PER_REGISTER: u32 = 10;

/// One function select register (`GPFSELn`).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct FunctionSelect {
    /// The function code of each of the register's ten pins
    #[bits(0..=2, rw, stride = 3)]
    function: [u3; 10],
}

/// Pull-up/down control (`GPPUD`): the pull a pin takes when its clock bit
/// is set.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct PullControl {
    /// The pull, as a `brcm,pull` code
    #[bits(0..=1, rw)]
    pull: u2,
}

/// The block's function select and pull registers.
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
pub struct RegisterBlock {
    function_select: [FunctionSelect; 6],
    _reserved: [u32; 31],
    pull_control: PullControl,
    /// `GPPUDCLK0` and `GPPUDCLK1`: one bit per pin, 32 pins each
    pull_clock: [u32; 2],
}

assert_register_offsets!(RegisterBlock, 0xa0, {
    function_select: 0x00,
    pull_control: 0x94,
    pull_clock: 0x98,
});

/// Pins of one pull clock register.
const PINS_PER_CLOCK: u32 = 32;

/// The function select register holding `pin` and the pin's place in it, or
/// `None` for a pin the block does not have.
#[must_use]
pub const fn place(pin: u32) -> Option<(usize, usize)> {
    if pin < PINS {
        Some((
            (pin / PINS_PER_REGISTER) as usize,
            (pin % PINS_PER_REGISTER) as usize,
        ))
    } else {
        None
    }
}

/// Give `pin` the pull `pull`, or `None` for a pin the block does not have.
///
/// The control register holds the pull while the pin's clock bit latches it
/// into the pin, then both are cleared (BCM2835 ARM Peripherals, section 6.1,
/// `GPPUDCLKn`). `wait` runs between the steps and must last at least 150
/// cycles; Linux waits 1 µs.
pub fn apply_pull(
    registers: &mut MmioRegisterBlock<'_>,
    pin: u32,
    pull: PinPull,
    mut wait: impl FnMut(&mut MmioRegisterBlock<'_>),
) -> Option<()> {
    if pin >= PINS {
        return None;
    }
    let bank = (pin / PINS_PER_CLOCK) as usize;
    let bit = 1 << (pin % PINS_PER_CLOCK);
    registers.write_pull_control(PullControl::ZERO.with_pull(u2::new(pull.code())));
    wait(registers);
    registers.write_pull_clock(bank, bit).ok()?;
    wait(registers);
    registers.write_pull_control(PullControl::ZERO);
    registers.write_pull_clock(bank, 0).ok()
}

/// `current` with the pin at `slot` switched to `function`.
#[must_use]
pub fn select(current: FunctionSelect, slot: usize, function: PinFunction) -> FunctionSelect {
    current.with_function(slot, u3::new(function.select_code()))
}

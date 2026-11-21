#![no_std]

//! The GPIO block of the BCM2835 (`brcm,bcm2835-gpio`) and the BCM2711
//! (`brcm,bcm2711-gpio`): the function select registers that connect each pin
//! to an input, an output or one of its alternate functions, and the pull
//! resistors.
//!
//! Resources: BCM2835 ARM Peripherals, section 6.1 (`GPFSEL0` to `GPFSEL5`:
//! three bits per pin, ten pins per register; `GPPUD` and `GPPUDCLKn`) and
//! table 6-31 (the alternate functions of each pin); BCM2711 ARM Peripherals,
//! section 5.2 (the same function select registers for 58 pins, and
//! `GPIO_PUP_PDN_CNTRL_REG0` to `3` in place of `GPPUD`: two bits per pin,
//! sixteen pins per register, table 90) and section 5.3 (the alternate
//! functions).

use {
    arbitrary_int::{u2, u3},
    bitbybit::{bitenum, bitfield},
    derive_mmio::Mmio,
    gpio::gpio_v1::{PinFunction, PinPull},
    libmmio::assert_register_offsets,
};

/// Which block the node is: they differ in their pin count and pull registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chip {
    /// The BCM2835 to BCM2837 (Raspberry Pi 1 to 3): 54 pins, pulls latched
    /// through `GPPUD` and `GPPUDCLKn`
    Bcm2835,
    /// The BCM2711 (Raspberry Pi 4): 58 pins, a pull field per pin in
    /// `GPIO_PUP_PDN_CNTRL_REGn`
    Bcm2711,
}

impl Chip {
    /// The block a driver's matched compatible string names, by its position
    /// in `driver_compatible!("brcm,bcm2835-gpio", "brcm,bcm2711-gpio")`.
    #[must_use]
    pub const fn from_compatible_index(index: u64) -> Option<Self> {
        match index {
            0 => Some(Self::Bcm2835),
            1 => Some(Self::Bcm2711),
            _ => None,
        }
    }

    /// Pins of the block.
    #[must_use]
    pub const fn pins(self) -> u32 {
        match self {
            Self::Bcm2835 => 54,
            Self::Bcm2711 => 58,
        }
    }
}

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

/// The pull of one pin in `GPIO_PUP_PDN_CNTRL_REGn` (BCM2711 ARM
/// Peripherals, table 90); 0b11 is reserved.
#[bitenum(u2, exhaustive = false)]
#[derive(Debug, PartialEq, Eq)]
pub enum PullResistor {
    None = 0b00,
    Up = 0b01,
    Down = 0b10,
}

impl PullResistor {
    /// The resistor of `pull`.
    #[must_use]
    pub const fn of(pull: PinPull) -> Self {
        match pull {
            PinPull::Off => Self::None,
            PinPull::Up => Self::Up,
            PinPull::Down => Self::Down,
        }
    }
}

/// `GPIO_PUP_PDN_CNTRL_REGn` (BCM2711): the pull of sixteen pins, two bits
/// each, pin 16n + i at bits 2i and 2i + 1.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct PullResistorControl {
    #[bits(0..=1, rw, stride = 2)]
    pull: [Option<PullResistor>; 16],
}

/// The block's function select and pull registers: the BCM2835's
/// `GPPUD`/`GPPUDCLKn` and the BCM2711's `GPIO_PUP_PDN_CNTRL_REGn`; each chip
/// uses its own.
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
pub struct RegisterBlock {
    function_select: [FunctionSelect; 6],
    _reserved: [u32; 31],
    pull_control: PullControl,
    /// `GPPUDCLK0` and `GPPUDCLK1`: one bit per pin, 32 pins each
    pull_clock: [u32; 2],
    _reserved_to_pull_resistors: [u32; 17],
    pull_resistor_control: [PullResistorControl; 4],
}

assert_register_offsets!(RegisterBlock, 0xf4, {
    function_select: 0x00,
    pull_control: 0x94,
    pull_clock: 0x98,
    pull_resistor_control: 0xe4,
});

/// Pins of one pull clock register.
const PINS_PER_CLOCK: u32 = 32;

/// Pins of one `GPIO_PUP_PDN_CNTRL_REGn`.
const PINS_PER_PULL_RESISTOR_CONTROL: u32 = 16;

/// The function select register holding `pin` and the pin's place in it, or
/// `None` for a pin `chip`'s block does not have.
#[must_use]
pub const fn place(chip: Chip, pin: u32) -> Option<(usize, usize)> {
    if pin < chip.pins() {
        Some((
            (pin / PINS_PER_REGISTER) as usize,
            (pin % PINS_PER_REGISTER) as usize,
        ))
    } else {
        None
    }
}

/// Give `pin` the pull `pull`, or `None` for a pin `chip`'s block does not
/// have.
///
/// On the BCM2835 the control register holds the pull while the pin's clock
/// bit latches it into the pin, then both are cleared (BCM2835 ARM
/// Peripherals, section 6.1, `GPPUDCLKn`); `wait` runs between the steps and
/// must last at least 150 cycles (Linux waits 1 µs). On the BCM2711 the pin's
/// field of `GPIO_PUP_PDN_CNTRL_REGn` is the pull itself, written at once
/// (BCM2711 ARM Peripherals, table 90), and `wait` does not run.
pub fn apply_pull(
    registers: &mut MmioRegisterBlock<'_>,
    chip: Chip,
    pin: u32,
    pull: PinPull,
    mut wait: impl FnMut(&mut MmioRegisterBlock<'_>),
) -> Option<()> {
    if pin >= chip.pins() {
        return None;
    }
    if chip == Chip::Bcm2711 {
        let register = (pin / PINS_PER_PULL_RESISTOR_CONTROL) as usize;
        let field = (pin % PINS_PER_PULL_RESISTOR_CONTROL) as usize;
        let current = registers.read_pull_resistor_control(register).ok()?;
        return registers
            .write_pull_resistor_control(register, current.with_pull(field, PullResistor::of(pull)))
            .ok();
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

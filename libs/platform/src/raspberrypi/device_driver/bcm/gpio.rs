/*
 * SPDX-License-Identifier: MIT OR BlueOak-1.0.0
 * Copyright (c) 2018-2019 Andre Richter <andre.o.richter@gmail.com>
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 * Original code distributed under MIT, additional changes are under BlueOak-1.0.0
 */

use {
    crate::device_driver::IRQNumber,
    bitbybit::{bitenum, bitfield},
    core::marker::PhantomData,
    derive_mmio::Mmio,
    libaddress::{Address, Virtual},
    liblocking::{IRQSafeNullLock, interface::Mutex},
    libmmio::assert_register_offsets,
};

// Descriptions taken from
// https://github.com/raspberrypi/documentation/files/1888662/BCM2837-ARM-Peripherals.-.Revised.-.V2-1.pdf
// and the BCM2711 ARM Peripherals manual.

/// Generates `pub enums` with no variants for each `ident` passed in.
macro states($($name:ident),*) {
$(pub enum $name {  })*
}

// Possible states for a GPIO pin.
states! {
    Uninitialized, Input, Output, Alt
}

/// An alternative GPIO function.
#[bitenum(u3, exhaustive = true)]
#[derive(Debug, PartialEq, Eq)]
pub enum Function {
    Input = 0b000,
    Output = 0b001,
    Alt0 = 0b100,
    Alt1 = 0b101,
    Alt2 = 0b110,
    Alt3 = 0b111,
    Alt4 = 0b011,
    Alt5 = 0b010,
}

/// Pull up/down resistor setup.
#[derive(PartialEq, Eq, Copy, Clone)]
pub enum PullUpDown {
    None,
    Up,
    Down,
}

/// `GPFSELn`: the function of ten consecutive pins.
#[bitfield(u32, default = 0, debug)]
pub struct FunctionSelect {
    #[bits(0..=2, rw, stride = 3)]
    function: [Function; 10],
}

/// `GPSETn` / `GPCLRn` / `GPLEVn` / `GPPUDCLKn`: one bit per pin of a 32-pin bank.
#[bitfield(u32, default = 0, debug)]
pub struct Pins {
    #[bit(0, rw)]
    pin: [bool; 32],
}

/// The pull a GPPUD write applies (BCM2837).
#[cfg(board_rpi3)]
#[bitenum(u2, exhaustive = false)]
#[derive(Debug, PartialEq, Eq)]
pub enum PullControl {
    Off = 0b00,
    PullDown = 0b01,
    PullUp = 0b10,
}

/// `GPPUD` (BCM2837): the pull applied to the pins clocked by `GPPUDCLKn`.
#[cfg(board_rpi3)]
#[bitfield(u32, default = 0, debug)]
pub struct PullUpDownEnable {
    #[bits(0..=1, rw)]
    control: Option<PullControl>,
}

/// The pull of a pin (BCM2711).
#[cfg(board_rpi4)]
#[bitenum(u2, exhaustive = false)]
#[derive(Debug, PartialEq, Eq)]
pub enum PullResistor {
    None = 0b00,
    Up = 0b01,
    Down = 0b10,
}

/// `GPIO_PUP_PDN_CNTRL_REGn` (BCM2711): the pull of sixteen pins.
#[cfg(board_rpi4)]
#[bitfield(u32, default = 0, debug)]
pub struct PullUpDownControl {
    #[bits(0..=1, rw, stride = 2)]
    pull: [Option<PullResistor>; 16],
}

/// The GPIO register block.
#[cfg(board_rpi3)]
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
pub struct GpioRegisterBlock {
    function_select: [FunctionSelect; 6],
    _reserved1: u32,
    #[mmio(Write)]
    set_pin: [Pins; 2],
    _reserved2: u32,
    #[mmio(Write)]
    clear_pin: [Pins; 2],
    _reserved3: u32,
    #[mmio(PureRead)]
    pin_level: [Pins; 2],
    _events: [u32; 22],
    pull_up_down: PullUpDownEnable,
    pull_up_down_clock: [Pins; 2],
}

#[cfg(board_rpi3)]
assert_register_offsets!(GpioRegisterBlock, 0xa0, {
    function_select: 0x00,
    set_pin: 0x1c,
    clear_pin: 0x28,
    pin_level: 0x34,
    pull_up_down: 0x94,
    pull_up_down_clock: 0x98,
});

/// The GPIO register block.
#[cfg(board_rpi4)]
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
pub struct GpioRegisterBlock {
    function_select: [FunctionSelect; 6],
    _reserved1: u32,
    #[mmio(Write)]
    set_pin: [Pins; 2],
    _reserved2: u32,
    #[mmio(Write)]
    clear_pin: [Pins; 2],
    _reserved3: u32,
    #[mmio(PureRead)]
    pin_level: [Pins; 2],
    _events: [u32; 42],
    pull_up_down_control: [PullUpDownControl; 4],
}

#[cfg(board_rpi4)]
assert_register_offsets!(GpioRegisterBlock, 0xf4, {
    function_select: 0x00,
    set_pin: 0x1c,
    clear_pin: 0x28,
    pin_level: 0x34,
    pull_up_down_control: 0xe4,
});

/// Bank and position of `pin` in registers holding `per_register` pins.
const fn locate(pin: usize, per_register: usize) -> (usize, usize) {
    (pin / per_register, pin % per_register)
}

struct GPIOInner {
    registers: MmioGpioRegisterBlock<'static>,
}

/// Public interface to the GPIO MMIO area
pub struct GPIO {
    inner: IRQSafeNullLock<GPIOInner>,
}

// Pins are at most 53 by construction (`Pin::new`), so every bank index below
// is in range: the array accessors' `OutOfBoundsError` cannot happen, and a
// write that reports one is ignored.

impl GPIOInner {
    /// # Safety
    ///
    /// `mmio_base_addr` maps the GPIO block as device memory for the rest of
    /// the program, and nothing else accesses it.
    pub const unsafe fn new(mmio_base_addr: Address<Virtual>) -> Self {
        Self {
            // SAFETY: the caller's contract.
            registers: unsafe { GpioRegisterBlock::new_mmio(mmio_base_addr.as_mut_ptr()) },
        }
    }

    #[cfg(board_rpi3)]
    pub fn power_off(&mut self) {
        use core::time::Duration;

        // The Linux 2837 GPIO driver waits 1 µs between the steps.
        const DELAY: Duration = Duration::from_micros(1);

        // power off gpio pins (but not VCC pins)
        for bank in 0..5 {
            let _in_range = self
                .registers
                .write_function_select(bank, FunctionSelect::ZERO);
        }

        self.registers
            .write_pull_up_down(PullUpDownEnable::ZERO.with_control(PullControl::Off));

        libtime::time::time_manager().spin_for(DELAY);

        for bank in 0..self.registers.len_pull_up_down_clock() {
            let _in_range = self
                .registers
                .write_pull_up_down_clock(bank, Pins::new_with_raw_value(u32::MAX));
        }

        libtime::time::time_manager().spin_for(DELAY);

        // flush GPIO setup
        for bank in 0..self.registers.len_pull_up_down_clock() {
            let _in_range = self.registers.write_pull_up_down_clock(bank, Pins::ZERO);
        }
    }

    #[cfg(board_rpi4)]
    pub fn power_off(&mut self) {
        todo!()
    }

    #[cfg(board_rpi3)]
    pub fn set_pull_up_down(&mut self, pin: usize, pull: PullUpDown) {
        use core::time::Duration;

        // The Linux 2837 GPIO driver waits 1 µs between the steps.
        const DELAY: Duration = Duration::from_micros(1);

        let (bank, bit) = locate(pin, 32);

        self.registers
            .write_pull_up_down(PullUpDownEnable::ZERO.with_control(match pull {
                PullUpDown::None => PullControl::Off,
                PullUpDown::Up => PullControl::PullUp,
                PullUpDown::Down => PullControl::PullDown,
            }));

        libtime::time::time_manager().spin_for(DELAY);

        let _in_range = self
            .registers
            .write_pull_up_down_clock(bank, Pins::ZERO.with_pin(bit, true));

        libtime::time::time_manager().spin_for(DELAY);

        self.registers
            .write_pull_up_down(PullUpDownEnable::ZERO.with_control(PullControl::Off));
        let _in_range = self.registers.write_pull_up_down_clock(bank, Pins::ZERO);
    }

    #[cfg(board_rpi4)]
    pub fn set_pull_up_down(&mut self, pin: usize, pull: PullUpDown) {
        let (bank, index) = locate(pin, 16);
        let resistor = match pull {
            PullUpDown::None => PullResistor::None,
            PullUpDown::Up => PullResistor::Up,
            PullUpDown::Down => PullResistor::Down,
        };
        let _in_range = self
            .registers
            .modify_pull_up_down_control(bank, |control| control.with_pull(index, resistor));
    }

    pub fn set_function(&mut self, pin: usize, function: Function) {
        let (bank, index) = locate(pin, 10);
        let _in_range = self
            .registers
            .modify_function_select(bank, |select| select.with_function(index, function));
    }

    pub fn set_pin(&mut self, pin: usize) {
        let (bank, bit) = locate(pin, 32);
        // GPSET is write-1-to-set: zero bits leave the other pins alone.
        let _in_range = self
            .registers
            .write_set_pin(bank, Pins::ZERO.with_pin(bit, true));
    }

    pub fn clear_pin(&mut self, pin: usize) {
        let (bank, bit) = locate(pin, 32);
        let _in_range = self
            .registers
            .write_clear_pin(bank, Pins::ZERO.with_pin(bit, true));
    }

    pub fn get_level(&self, pin: usize) -> Level {
        let (bank, bit) = locate(pin, 32);
        self.registers
            .read_pin_level(bank)
            .is_ok_and(|level| level.pin(bit))
    }
}
impl GPIO {
    pub const COMPATIBLE: &'static str = "BCM GPIO";

    /// # Safety
    ///
    /// Unsafe, duh!
    pub const unsafe fn new(mmio_base_addr: Address<Virtual>) -> Self {
        Self {
            inner: IRQSafeNullLock::new(
                // SAFETY: Unsafe!
                unsafe { GPIOInner::new(mmio_base_addr) },
            ),
        }
    }

    pub fn get_pin(&self, pin: usize) -> Pin<'_, Uninitialized> {
        // SAFETY: Not safe yet
        unsafe { Pin::new(pin, &self.inner) } // todo: expose only inner to avoid unlocked access
    }

    pub fn power_off(&self) {
        self.inner.lock(GPIOInner::power_off);
    }
}

/// A GPIO pin in state `State`.
///
/// The `State` generic always corresponds to an un-instantiable type that is
/// used solely to mark and track the state of a given GPIO pin. A `Pin`
/// structure starts in the `Uninitialized` state and must be transitioned into
/// one of `Input`, `Output`, or `Alt` via the `into_input`, `into_output`, and
/// `into_alt` methods before it can be used.
#[allow(clippy::struct_field_names)]
pub struct Pin<'outer, State> {
    pin: usize,
    inner: &'outer IRQSafeNullLock<GPIOInner>,
    _state: PhantomData<State>,
}

impl<'outer, State> Pin<'outer, State> {
    /// Transitions `self` to state `NewState`, consuming `self` and returning a new
    /// `Pin` instance in state `NewState`. This method should _never_ be exposed to
    /// the public!
    #[inline(always)]
    fn transition<NewState>(self) -> Pin<'outer, NewState> {
        Pin {
            pin: self.pin,
            inner: self.inner,
            _state: PhantomData,
        }
    }

    pub fn set_pull_up_down(&self, pull: PullUpDown) {
        self.inner
            .lock(|inner| inner.set_pull_up_down(self.pin, pull));
    }
}

impl<'outer> Pin<'outer, Uninitialized> {
    /// Returns a new GPIO `Pin` structure for pin number `pin`.
    ///
    /// # Panics
    ///
    /// Panics if `pin` > `53`.
    unsafe fn new(
        pin: usize,
        inner: &'outer IRQSafeNullLock<GPIOInner>,
    ) -> Pin<'outer, Uninitialized> {
        assert!(
            pin <= 53,
            "gpio::Pin::new(): pin {pin} exceeds maximum of 53"
        );

        Pin {
            inner,
            pin,
            _state: PhantomData,
        }
    }

    /// Enables the alternative function `function` for `self`. Consumes self
    /// and returns a `Pin` structure in the `Alt` state.
    pub fn into_alt(self, function: Function) -> Pin<'outer, Alt> {
        self.inner
            .lock(|inner| inner.set_function(self.pin, function));
        self.transition()
    }

    /// Sets this pin to be an _output_ pin. Consumes self and returns a `Pin`
    /// structure in the `Output` state.
    pub fn into_output(self) -> Pin<'outer, Output> {
        self.into_alt(Function::Output).transition()
    }

    /// Sets this pin to be an _input_ pin. Consumes self and returns a `Pin`
    /// structure in the `Input` state.
    pub fn into_input(self) -> Pin<'outer, Input> {
        self.into_alt(Function::Input).transition()
    }
}

impl Pin<'_, Output> {
    /// Sets (turns on) this pin.
    pub fn set(&mut self) {
        self.inner.lock(|inner| inner.set_pin(self.pin));
    }

    /// Clears (turns off) this pin.
    pub fn clear(&mut self) {
        self.inner.lock(|inner| inner.clear_pin(self.pin));
    }
}

pub type Level = bool;

impl Pin<'_, Input> {
    /// Reads the pin's value. Returns `true` if the level is high and `false`
    /// if the level is low.
    pub fn level(&self) -> Level {
        self.inner.lock(|inner| inner.get_level(self.pin))
    }
}

//--------------------------------------------------------------------------------------------------
// OS Interface Code
//--------------------------------------------------------------------------------------------------

impl libdriver::drivers::interface::DeviceDriver for GPIO {
    type IRQNumberType = IRQNumber;

    fn compatible(&self) -> &'static str {
        Self::COMPATIBLE
    }
}

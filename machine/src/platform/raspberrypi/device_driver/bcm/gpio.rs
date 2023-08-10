/*
 * SPDX-License-Identifier: MIT OR BlueOak-1.0.0
 * Copyright (c) 2018-2019 Andre Richter <andre.o.richter@gmail.com>
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 * Original code distributed under MIT, additional changes are under BlueOak-1.0.0
 */

use {
    crate::{
        memory::{Address, Virtual},
        platform::{
            device_driver::{common::MMIODerefWrapper, IRQNumber},
            BcmHost,
        },
        synchronization::{interface::Mutex, IRQSafeNullLock},
        time,
    },
    core::{marker::PhantomData, time::Duration},
    tock_registers::{
        fields::FieldValue,
        interfaces::{ReadWriteable, Readable, Writeable},
        register_structs,
        registers::{ReadOnly, ReadWrite, WriteOnly},
    },
};

// Descriptions taken from
// https://github.com/raspberrypi/documentation/files/1888662/BCM2837-ARM-Peripherals.-.Revised.-.V2-1.pdf

/// Generates `pub enums` with no variants for each `ident` passed in.
macro states($($name:ident),*) {
$(pub enum $name {  })*
}

// Possible states for a GPIO pin.
states! {
    Uninitialized, Input, Output, Alt
}

#[cfg(feature = "rpi3")]
register_structs! {
    /// The offsets for each register.
    /// From <https://wiki.osdev.org/Raspberry_Pi_Bare_Bones> and
    /// <https://github.com/raspberrypi/documentation/files/1888662/BCM2837-ARM-Peripherals.-.Revised.-.V2-1.pdf>
    #[allow(non_snake_case)]
    RegisterBlock {
        (0x00 => pub FunctionSelect: [ReadWrite<u32>; 6]), // function select
        (0x18 => __reserved_1),
        (0x1c => pub SetPin: [WriteOnly<u32>; 2]), // set output pin
        (0x24 => __reserved_2),
        (0x28 => pub ClearPin: [WriteOnly<u32>; 2]), // clear output pin
        (0x30 => __reserved_3),
        (0x34 => pub PinLevel: [ReadOnly<u32>; 2]), // get input pin level
        (0x3c => __reserved_4),
        // Everything below is unused atm!
        // (0x40 => pub EDS: [ReadWrite<u32>; 2]),
        // (0x48 => __reserved_5),
        // (0x4c => pub REN: [ReadWrite<u32>; 2]),
        // (0x54 => __reserved_6),
        // (0x58 => pub FEN: [ReadWrite<u32>; 2]),
        // (0x60 => __reserved_7),
        // (0x64 => pub HEN: [ReadWrite<u32>; 2]),
        // (0x6c => __reserved_8),
        // (0x70 => pub LEN: [ReadWrite<u32>; 2]),
        // (0x78 => __reserved_9),
        // (0x7c => pub AREN: [ReadWrite<u32>; 2]),
        // (0x84 => __reserved_10),
        // (0x88 => pub AFEN: [ReadWrite<u32>; 2]),
        // (0x90 => __reserved_11),
        (0x94 => pub PullUpDown: ReadWrite<u32>),
        (0x98 => pub PullUpDownEnableClock: [ReadWrite<u32>; 2]),
        (0xa0 => @END),
    }
}

#[cfg(feature = "rpi4")]
register_structs! {
    /// The offsets for each register.
    /// From <https://wiki.osdev.org/Raspberry_Pi_Bare_Bones> and
    /// <https://github.com/raspberrypi/documentation/files/1888662/BCM2837-ARM-Peripherals.-.Revised.-.V2-1.pdf>
    #[allow(non_snake_case)]
    RegisterBlock {
        (0x00 => pub FunctionSelect: [ReadWrite<u32>; 6]), // function select
        (0x18 => __reserved_1),
        (0x1c => pub SetPin: [WriteOnly<u32>; 2]), // set output pin
        (0x24 => __reserved_2),
        (0x28 => pub ClearPin: [WriteOnly<u32>; 2]), // clear output pin
        (0x30 => __reserved_3),
        (0x34 => pub PinLevel: [ReadOnly<u32>; 2]), // get input pin level
        (0x3c => __reserved_4),
        (0xe4 => PullUpDownControl: [ReadWrite<u32>; 4]),
        (0xf4 => @END),
    }
}

// Hide RegisterBlock from public api.
type Registers = MMIODerefWrapper<RegisterBlock>;

struct GPIOInner {
    registers: Registers,
}

/// Public interface to the GPIO MMIO area
pub struct GPIO {
    inner: IRQSafeNullLock<GPIOInner>,
}

impl GPIOInner {
    pub const unsafe fn new(mmio_base_addr: Address<Virtual>) -> Self {
        Self {
            registers: Registers::new(mmio_base_addr),
        }
    }

    #[cfg(feature = "rpi3")]
    pub fn power_off(&self) {
        // power off gpio pins (but not VCC pins)
        for bank in 0..5 {
            self.registers.FunctionSelect[bank].set(0);
        }

        self.registers.PullUpDown.set(0);

        // The Linux 2837 GPIO driver waits 1 µs between the steps.
        const DELAY: Duration = Duration::from_micros(1);

        time::time_manager().spin_for(DELAY);

        self.registers.PullUpDownEnableClock[0].set(0xffff_ffff);
        self.registers.PullUpDownEnableClock[1].set(0xffff_ffff);

        time::time_manager().spin_for(DELAY);

        // flush GPIO setup
        self.registers.PullUpDownEnableClock[0].set(0);
        self.registers.PullUpDownEnableClock[1].set(0);
    }

    #[cfg(feature = "rpi4")]
    pub fn power_off(&self) {
        todo!()
    }

    #[cfg(feature = "rpi3")]
    pub fn set_pull_up_down(&self, pin: usize, pull: PullUpDown) {
        let bank = pin / 32;
        let off = pin % 32;

        self.registers.PullUpDown.set(0);

        // The Linux 2837 GPIO driver waits 1 µs between the steps.
        const DELAY: Duration = Duration::from_micros(1);

        time::time_manager().spin_for(DELAY);

        self.registers.PullUpDownEnableClock[bank].modify(FieldValue::<u32, ()>::new(
            0b1,
            off,
            (pull == PullUpDown::Up).into(),
        ));

        time::time_manager().spin_for(DELAY);

        self.registers.PullUpDown.set(0);
        self.registers.PullUpDownEnableClock[bank].set(0);
    }

    #[cfg(feature = "rpi4")]
    pub fn set_pull_up_down(&self, pin: usize, pull: PullUpDown) {
        let bank = pin / 16;
        let off = pin % 16;

        self.registers.PullUpDownControl[bank].modify(FieldValue::<u32, ()>::new(
            0b11,
            off * 2,
            pull.into(),
        ));
    }

    pub fn to_alt(&self, pin: usize, function: Function) {
        let bank = pin / 10;
        let off = pin % 10;

        self.registers.FunctionSelect[bank].modify(FieldValue::<u32, ()>::new(
            0b111,
            off * 3,
            function.into(),
        ));
    }

    pub fn set_pin(&mut self, pin: usize) {
        // Guarantees: pin number is between [0; 53] by construction.
        let bank = pin / 32;
        let shift = pin % 32;
        self.registers.SetPin[bank].set(1 << shift);
    }

    pub fn clear_pin(&mut self, pin: usize) {
        // Guarantees: pin number is between [0; 53] by construction.
        let bank = pin / 32;
        let shift = pin % 32;
        self.registers.ClearPin[bank].set(1 << shift);
    }

    pub fn get_level(&self, pin: usize) -> Level {
        // Guarantees: pin number is between [0; 53] by construction.
        let bank = pin / 32;
        let off = pin % 32;
        self.registers.PinLevel[bank].matches_all(FieldValue::<u32, ()>::new(1, off, 1))
    }
}

impl GPIO {
    pub const COMPATIBLE: &'static str = "BCM GPIO";

    /// # Safety
    ///
    /// Unsafe, duh!
    pub const unsafe fn new(mmio_base_addr: Address<Virtual>) -> Self {
        Self {
            inner: IRQSafeNullLock::new(GPIOInner::new(mmio_base_addr)),
        }
    }

    pub fn get_pin(&self, pin: usize) -> Pin<Uninitialized> {
        unsafe { Pin::new(pin, &self.inner) } // todo: expose only inner to avoid unlocked access
    }

    pub fn power_off(&self) {
        self.inner.lock(|inner| inner.power_off());
    }
}

/// An alternative GPIO function.
#[repr(u8)]
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

impl ::core::convert::From<Function> for u32 {
    fn from(f: Function) -> Self {
        f as u32
    }
}

/// Pull up/down resistor setup.
#[repr(u8)]
#[derive(PartialEq, Eq)]
pub enum PullUpDown {
    None = 0b00,
    Up = 0b01,
    Down = 0b10,
}

impl ::core::convert::From<PullUpDown> for u32 {
    fn from(p: PullUpDown) -> Self {
        p as u32
    }
}

/// A GPIO pin in state `State`.
///
/// The `State` generic always corresponds to an un-instantiable type that is
/// used solely to mark and track the state of a given GPIO pin. A `Pin`
/// structure starts in the `Uninitialized` state and must be transitioned into
/// one of `Input`, `Output`, or `Alt` via the `into_input`, `into_output`, and
/// `into_alt` methods before it can be used.
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
            .lock(|inner| inner.set_pull_up_down(self.pin, pull))
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
        if pin > 53 {
            panic!("gpio::Pin::new(): pin {pin} exceeds maximum of 53");
        }

        Pin {
            inner,
            pin,
            _state: PhantomData,
        }
    }

    /// Enables the alternative function `function` for `self`. Consumes self
    /// and returns a `Pin` structure in the `Alt` state.
    pub fn into_alt(self, function: Function) -> Pin<'outer, Alt> {
        self.inner.lock(|inner| inner.to_alt(self.pin, function));
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

impl<'outer> Pin<'outer, Output> {
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

impl<'outer> Pin<'outer, Input> {
    /// Reads the pin's value. Returns `true` if the level is high and `false`
    /// if the level is low.
    pub fn level(&self) -> Level {
        self.inner.lock(|inner| inner.get_level(self.pin))
    }
}

//--------------------------------------------------------------------------------------------------
// OS Interface Code
//--------------------------------------------------------------------------------------------------

impl crate::drivers::interface::DeviceDriver for GPIO {
    type IRQNumberType = IRQNumber;

    fn compatible(&self) -> &'static str {
        Self::COMPATIBLE
    }
}

//--------------------------------------------------------------------------------------------------
// Testing
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test_case]
    fn test_pin_transitions() {
        let mut reg = [0u32; 40];
        let mmio_base_addr = Address::<Virtual>::new(&mut reg as *mut _ as usize);
        let gpio = unsafe { GPIO::new(mmio_base_addr) };

        let _out = gpio.get_pin(1).into_output();
        assert_eq!(reg[0], 0b001_000);
        let _inp = gpio.get_pin(12).into_input();
        assert_eq!(reg[1], 0b000_000_000);
        let _alt = gpio.get_pin(35).into_alt(Function::Alt1);
        assert_eq!(reg[3], 0b101_000_000_000_000_000);
    }

    #[test_case]
    fn test_pin_outputs() {
        let mut reg = [0u32; 40];
        let mmio_base_addr = Address::<Virtual>::new(&mut reg as *mut _ as usize);
        let gpio = unsafe { GPIO::new(mmio_base_addr) };

        let pin = gpio.get_pin(1);
        let mut out = pin.into_output();
        out.set();
        assert_eq!(reg[7], 0b10); // SET pin 1 = 1 << 1
        out.clear();
        assert_eq!(reg[10], 0b10); // CLR pin 1 = 1 << 1

        let pin = gpio.get_pin(35);
        let mut out = pin.into_output();
        out.set();
        assert_eq!(reg[8], 0b1000); // SET pin 35 = 1 << (35 - 32)
        out.clear();
        assert_eq!(reg[11], 0b1000); // CLR pin 35 = 1 << (35 - 32)
    }

    #[test_case]
    fn test_pin_inputs() {
        let mut reg = [0u32; 40];
        let mmio_base_addr = Address::<Virtual>::new(&mut reg as *mut _ as usize);
        let gpio = unsafe { GPIO::new(mmio_base_addr) };

        let pin = gpio.get_pin(1);
        let inp = pin.into_input();
        assert_eq!(inp.level(), false);

        reg[13] = 0b10;

        assert_eq!(inp.level(), true);

        let pin = gpio.get_pin(35);
        let inp = pin.into_input();
        assert_eq!(inp.level(), false);

        reg[14] = 0b1000;

        assert_eq!(inp.level(), true);
    }
}

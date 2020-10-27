/*
 * SPDX-License-Identifier: MIT OR BlueOak-1.0.0
 * Copyright (c) 2018-2019 Andre Richter <andre.o.richter@gmail.com>
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 * Original code distributed under MIT, additional changes are under BlueOak-1.0.0
 */

use {
    super::BcmHost,
    crate::arch::loop_delay,
    core::{marker::PhantomData, ops},
    register::{
        mmio::{ReadOnly, ReadWrite, WriteOnly},
        register_bitfields, FieldValue,
    },
};

// Descriptions taken from
// https://github.com/raspberrypi/documentation/files/1888662/BCM2837-ARM-Peripherals.-.Revised.-.V2-1.pdf
register_bitfields! {
    u32,

    /// GPIO Pull-up/down Clock Register 0
    PUDCLK0 [
        /// Pin 15
        PUDCLK15 OFFSET(15) NUMBITS(1) [
            NoEffect = 0,
            AssertClock = 1
        ],

        /// Pin 14
        PUDCLK14 OFFSET(14) NUMBITS(1) [
            NoEffect = 0,
            AssertClock = 1
        ]
    ]
}

/// Generates `pub enums` with no variants for each `ident` passed in.
macro states($($name:ident),*) {
$(pub enum $name {  })*
}

// Possible states for a GPIO pin.
states! {
    Uninitialized, Input, Output, Alt
}

/// A wrapper type that prevents reads or writes to its value.
///
/// This type implements no methods. It is meant to make the inner type
/// inaccessible to prevent accidental reads or writes.
#[repr(C)]
pub struct Reserved<T>(T);

/// The offsets for reach register.
/// From https://wiki.osdev.org/Raspberry_Pi_Bare_Bones and
/// https://github.com/raspberrypi/documentation/files/1888662/BCM2837-ARM-Peripherals.-.Revised.-.V2-1.pdf
#[allow(non_snake_case)]
#[repr(C)]
pub struct RegisterBlock {
    pub FSEL: [ReadWrite<u32>; 6], // 0x00-0x14 function select
    __reserved_0: Reserved<u32>,   // 0x18
    pub SET: [WriteOnly<u32>; 2],  // 0x1c-0x20 set output pin
    __reserved_1: Reserved<u32>,   // 0x24
    pub CLR: [WriteOnly<u32>; 2],  // 0x28-0x2c clear output pin
    __reserved_2: Reserved<u32>,   // 0x30
    pub LEV: [ReadOnly<u32>; 2],   // 0x34-0x38 get input pin level
    __reserved_3: Reserved<u32>,   // 0x3C
    pub EDS: [ReadWrite<u32>; 2],  // 0x40-0x44
    __reserved_4: Reserved<u32>,   // 0x48
    pub REN: [ReadWrite<u32>; 2],  // 0x4c-0x50
    __reserved_5: Reserved<u32>,   // 0x54
    pub FEN: [ReadWrite<u32>; 2],  // 0x58-0x5c
    __reserved_6: Reserved<u32>,   // 0x60
    pub HEN: [ReadWrite<u32>; 2],  // 0x64-0x68
    __reserved_7: Reserved<u32>,   // 0x6c
    pub LEN: [ReadWrite<u32>; 2],  // 0x70-0x74
    __reserved_8: Reserved<u32>,   // 0x78
    pub AREN: [ReadWrite<u32>; 2], // 0x7c-0x80
    __reserved_9: Reserved<u32>,   // 0x84
    pub AFEN: [ReadWrite<u32>; 2], // 0x88-0x8c
    __reserved_10: Reserved<u32>,  // 0x90
    pub PUD: ReadWrite<u32>,       // 0x94      pull up down
    pub PUDCLK: [ReadWrite<u32, PUDCLK0::Register>; 2], // 0x98-0x9C -- @todo remove this register
}

/// Public interface to the GPIO MMIO area
pub struct GPIO {
    base_addr: usize,
}

/// Deref to RegisterBlock
///
/// Allows writing
/// ```
/// self.GPPUD.read()
/// ```
/// instead of something along the lines of
/// ```
/// unsafe { (*GPIO::ptr()).GPPUD.read() }
/// ```
impl ops::Deref for GPIO {
    type Target = RegisterBlock;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr() }
    }
}

impl Default for GPIO {
    fn default() -> GPIO {
        // Default RPi3 GPIO base address
        const GPIO_BASE: usize = BcmHost::get_peripheral_address() + 0x20_0000;
        GPIO::new(GPIO_BASE)
    }
}

impl GPIO {
    pub fn new(base_addr: usize) -> GPIO {
        GPIO { base_addr }
    }

    /// Returns a pointer to the register block
    fn ptr(&self) -> *const RegisterBlock {
        self.base_addr as *const _
    }

    pub fn get_pin(&self, pin: usize) -> Pin<Uninitialized> {
        Pin::new(pin, self.base_addr)
    }
}

pub fn enable_uart_pins(gpio: &GPIO) {
    gpio.PUD.set(0);

    loop_delay(150);

    // enable pins 14 and 15
    gpio.PUDCLK[0].write(PUDCLK0::PUDCLK14::AssertClock + PUDCLK0::PUDCLK15::AssertClock);

    loop_delay(150);

    gpio.PUDCLK[0].set(0);
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

/// A GPIO pin in state `State`.
///
/// The `State` generic always corresponds to an un-instantiable type that is
/// used solely to mark and track the state of a given GPIO pin. A `Pin`
/// structure starts in the `Uninitialized` state and must be transitioned into
/// one of `Input`, `Output`, or `Alt` via the `into_input`, `into_output`, and
/// `into_alt` methods before it can be used.
pub struct Pin<State> {
    pin: usize,
    base_addr: usize,
    _state: PhantomData<State>,
}

impl<State> Pin<State> {
    /// Transitions `self` to state `NewState`, consuming `self` and returning a new
    /// `Pin` instance in state `NewState`. This method should _never_ be exposed to
    /// the public!
    #[inline(always)]
    fn transition<NewState>(self) -> Pin<NewState> {
        Pin {
            pin: self.pin,
            base_addr: self.base_addr,
            _state: PhantomData,
        }
    }

    /// Returns a pointer to the register block
    #[inline(always)]
    fn ptr(&self) -> *const RegisterBlock {
        self.base_addr as *const _
    }
}

/// Deref to Pin's Registers
///
/// Allows writing
/// ```
/// self.PUD.read()
/// ```
/// instead of something along the lines of
/// ```
/// unsafe { (*Pin::ptr()).PUD.read() }
/// ```
impl<State> ops::Deref for Pin<State> {
    type Target = RegisterBlock;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr() }
    }
}

impl Pin<Uninitialized> {
    /// Returns a new GPIO `Pin` structure for pin number `pin`.
    ///
    /// # Panics
    ///
    /// Panics if `pin` > `53`.
    fn new(pin: usize, base_addr: usize) -> Pin<Uninitialized> {
        if pin > 53 {
            panic!("gpio::Pin::new(): pin {} exceeds maximum of 53", pin);
        }

        Pin {
            base_addr,
            pin,
            _state: PhantomData,
        }
    }

    /// Enables the alternative function `function` for `self`. Consumes self
    /// and returns a `Pin` structure in the `Alt` state.
    pub fn into_alt(self, function: Function) -> Pin<Alt> {
        let bank = self.pin / 10;
        let off = self.pin % 10;
        self.FSEL[bank].modify(FieldValue::<u32, ()>::new(0b111, off * 3, function.into()));
        self.transition()
    }

    /// Sets this pin to be an _output_ pin. Consumes self and returns a `Pin`
    /// structure in the `Output` state.
    pub fn into_output(self) -> Pin<Output> {
        self.into_alt(Function::Output).transition()
    }

    /// Sets this pin to be an _input_ pin. Consumes self and returns a `Pin`
    /// structure in the `Input` state.
    pub fn into_input(self) -> Pin<Input> {
        self.into_alt(Function::Input).transition()
    }
}

impl Pin<Output> {
    /// Sets (turns on) this pin.
    pub fn set(&mut self) {
        // Guarantees: pin number is between [0; 53] by construction.
        let bank = self.pin / 32;
        let shift = self.pin % 32;
        self.SET[bank].set(1 << shift);
    }

    /// Clears (turns off) this pin.
    pub fn clear(&mut self) {
        // Guarantees: pin number is between [0; 53] by construction.
        let bank = self.pin / 32;
        let shift = self.pin % 32;
        self.CLR[bank].set(1 << shift);
    }
}

pub type Level = bool;

impl Pin<Input> {
    /// Reads the pin's value. Returns `true` if the level is high and `false`
    /// if the level is low.
    pub fn level(&self) -> Level {
        // Guarantees: pin number is between [0; 53] by construction.
        let bank = self.pin / 32;
        let off = self.pin % 32;
        self.LEV[bank].matches_all(FieldValue::<u32, ()>::new(1, off, 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test_case]
    fn test_pin_transitions() {
        let mut reg = [0u32; 40];
        let gpio = GPIO::new(&mut reg as *mut _ as usize);

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
        let gpio = GPIO::new(&mut reg as *mut _ as usize);

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
        let gpio = GPIO::new(&mut reg as *mut _ as usize);

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

/*
 * SPDX-License-Identifier: MIT OR BlueOak-1.0.0
 * Copyright (c) 2018-2019 Andre Richter <andre.o.richter@gmail.com>
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 * Original code distributed under MIT, additional changes are under BlueOak-1.0.0
 */

//! The GPIO block, as far as chainboot needs it: routing the PL011 to the header
//! pins 14 (TXD0) and 15 (RXD0) with their pull-ups on.
//!
//! <https://github.com/raspberrypi/documentation/files/1888662/BCM2837-ARM-Peripherals.-.Revised.-.V2-1.pdf>
//! and the BCM2711 ARM Peripherals manual.

use {
    bitbybit::{bitenum, bitfield},
    derive_mmio::Mmio,
    libmmio::assert_register_offsets,
};

/// The function a pin is switched to.
#[bitenum(u3, exhaustive = true)]
#[derive(Debug, PartialEq, Eq)]
enum Function {
    Input = 0b000,
    Output = 0b001,
    Alt0 = 0b100,
    Alt1 = 0b101,
    Alt2 = 0b110,
    Alt3 = 0b111,
    Alt4 = 0b011,
    Alt5 = 0b010,
}

/// `GPFSELn`: the function of ten consecutive pins.
#[bitfield(u32, default = 0, debug)]
struct FunctionSelect {
    #[bits(0..=2, rw, stride = 3)]
    function: [Function; 10],
}

/// `GPSETn`, `GPCLRn` and `GPPUDCLKn`: one bit per pin of a 32-pin bank.
#[bitfield(u32, default = 0, debug)]
struct Pins {
    #[bit(0, rw)]
    pin: [bool; 32],
}

/// The pull a `GPPUD` write applies (BCM2837).
#[cfg(board_rpi3)]
#[bitenum(u2, exhaustive = false)]
#[derive(Debug, PartialEq, Eq)]
enum PullControl {
    Off = 0b00,
    PullDown = 0b01,
    PullUp = 0b10,
}

/// `GPPUD` (BCM2837): the pull applied to the pins clocked by `GPPUDCLKn`.
#[cfg(board_rpi3)]
#[bitfield(u32, default = 0, debug)]
struct PullUpDownEnable {
    #[bits(0..=1, rw)]
    control: Option<PullControl>,
}

/// The pull of a pin (BCM2711).
#[cfg(board_rpi4)]
#[bitenum(u2, exhaustive = false)]
#[derive(Debug, PartialEq, Eq)]
enum PullResistor {
    None = 0b00,
    Up = 0b01,
    Down = 0b10,
}

/// `GPIO_PUP_PDN_CNTRL_REGn` (BCM2711): the pull of sixteen pins.
#[cfg(board_rpi4)]
#[bitfield(u32, default = 0, debug)]
struct PullUpDownControl {
    #[bits(0..=1, rw, stride = 2)]
    pull: [Option<PullResistor>; 16],
}

#[cfg(board_rpi3)]
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
struct RegisterBlock {
    function_select: [FunctionSelect; 6],
    _reserved1: [u32; 1],
    #[mmio(Write)]
    set_pin: [Pins; 2],
    _reserved2: [u32; 1],
    #[mmio(Write)]
    clear_pin: [Pins; 2],
    _reserved3: [u32; 25],
    pull_up_down: PullUpDownEnable,
    pull_up_down_clock: [Pins; 2],
}

#[cfg(board_rpi3)]
assert_register_offsets!(RegisterBlock, 0xa0, {
    function_select: 0x00,
    set_pin: 0x1c,
    clear_pin: 0x28,
    pull_up_down: 0x94,
    pull_up_down_clock: 0x98,
});

#[cfg(board_rpi4)]
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
struct RegisterBlock {
    function_select: [FunctionSelect; 6],
    _reserved1: [u32; 1],
    #[mmio(Write)]
    set_pin: [Pins; 2],
    _reserved2: [u32; 1],
    #[mmio(Write)]
    clear_pin: [Pins; 2],
    _reserved3: [u32; 45],
    pull_up_down_control: [PullUpDownControl; 4],
}

#[cfg(board_rpi4)]
assert_register_offsets!(RegisterBlock, 0xf4, {
    function_select: 0x00,
    set_pin: 0x1c,
    clear_pin: 0x28,
    pull_up_down_control: 0xe4,
});

/// The pins of the PL011, with `Alt0` the function that connects them to it.
const UART_PINS: [usize; 2] = [14, 15];
const UART_FUNCTION: Function = Function::Alt0;

/// Bank and position of `pin` in registers holding `per_register` pins.
const fn locate(pin: usize, per_register: usize) -> (usize, usize) {
    (pin / per_register, pin % per_register)
}

/// Connect the PL011's TXD0 and RXD0 to header pins 14 and 15 and pull both up.
///
/// # Safety
///
/// `base` is the address of the GPIO registers, accessible for as long as the
/// program runs, and nothing else uses them.
pub unsafe fn route_uart(base: usize) {
    // SAFETY: the caller's contract.
    let mut registers = unsafe { RegisterBlock::new_mmio(base as *mut RegisterBlock) };
    for pin in UART_PINS {
        let (bank, index) = locate(pin, 10);
        let _in_range = registers
            .modify_function_select(bank, |select| select.with_function(index, UART_FUNCTION));
        pull_up(&mut registers, pin);
    }
}

/// Make `pin` an output.
///
/// # Safety
///
/// `base` is the address of the GPIO registers, accessible for as long as the
/// program runs, and nothing else uses them; `pin` is a pin the program owns.
pub unsafe fn make_output(base: usize, pin: usize) {
    // SAFETY: the caller's contract.
    let mut registers = unsafe { RegisterBlock::new_mmio(base as *mut RegisterBlock) };
    let (bank, index) = locate(pin, 10);
    let _in_range = registers
        .modify_function_select(bank, |select| select.with_function(index, Function::Output));
}

/// Drive the output `pin` high or low. The set and clear registers act on the
/// pins whose bits are written and leave the others alone.
///
/// # Safety
///
/// As for [`make_output`].
pub unsafe fn write_pin(base: usize, pin: usize, high: bool) {
    // SAFETY: the caller's contract.
    let mut registers = unsafe { RegisterBlock::new_mmio(base as *mut RegisterBlock) };
    let (bank, bit) = locate(pin, 32);
    let pins = Pins::ZERO.with_pin(bit, true);
    let _in_range = if high {
        registers.write_set_pin(bank, pins)
    } else {
        registers.write_clear_pin(bank, pins)
    };
}

/// Pull `pin` up (BCM2837): select the pull in `GPPUD`, clock it into the pin,
/// then take both away again, waiting a microsecond between the steps as the
/// Linux driver does.
#[cfg(board_rpi3)]
fn pull_up(registers: &mut MmioRegisterBlock<'_>, pin: usize) {
    use core::time::Duration;

    const DELAY: Duration = Duration::from_micros(1);

    let (bank, bit) = locate(pin, 32);

    registers.write_pull_up_down(PullUpDownEnable::ZERO.with_control(PullControl::PullUp));
    libtime::time::time_manager().spin_for(DELAY);

    let _in_range = registers.write_pull_up_down_clock(bank, Pins::ZERO.with_pin(bit, true));
    libtime::time::time_manager().spin_for(DELAY);

    registers.write_pull_up_down(PullUpDownEnable::ZERO.with_control(PullControl::Off));
    let _in_range = registers.write_pull_up_down_clock(bank, Pins::ZERO);
}

/// Pull `pin` up (BCM2711): its two bits in the control register of its bank.
#[cfg(board_rpi4)]
fn pull_up(registers: &mut MmioRegisterBlock<'_>, pin: usize) {
    let (bank, index) = locate(pin, 16);
    let _in_range = registers
        .modify_pull_up_down_control(bank, |control| control.with_pull(index, PullResistor::Up));
}

/*
 * SPDX-License-Identifier: MIT OR BlueOak-1.0.0
 * Copyright (c) 2018-2019 Andre Richter <andre.o.richter@gmail.com>
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 * Original code distributed under MIT, additional changes are under BlueOak-1.0.0
 *
 * http://infocenter.arm.com/help/topic/com.arm.doc.ddi0183g/DDI0183G_uart_pl011_r1p5_trm.pdf
 */

//! The PL011 UART chainboot talks to its host over: polled, 115200 8N1, with the
//! FIFOs on and every interrupt masked.

use {
    arbitrary_int::{u6, u11},
    bitbybit::{bitenum, bitfield},
    core::{
        fmt,
        sync::atomic::{AtomicBool, Ordering},
    },
    derive_mmio::Mmio,
    libconsole::{SerialOps, console::interface},
    liblocking::{IRQSafeNullLock, interface::Mutex},
    libmmio::assert_register_offsets,
    libprimitives::cpu::loop_while,
};

const BAUD_RATE: u32 = 115_200;

/// How many times the UART's flags are looked at before the UART is given up on.
/// A byte takes 87 us to send, a few hundred looks; a million is some tenths of a
/// second.
const SPIN_LIMIT: u32 = 1_000_000;

/// The transmitter did not drain in time, so it is not running (a UART whose
/// clock is stopped never sends). Chainboot then carries on without a console, so
/// that the LED can still say it is alive, instead of waiting for ever.
static TRANSMITTER_DEAD: AtomicBool = AtomicBool::new(false);

/// Look at `busy` until it stops holding; `false` when it still held after
/// [`SPIN_LIMIT`] looks.
fn spin_while(busy: impl Fn() -> bool) -> bool {
    (0..SPIN_LIMIT).any(|_look| {
        let done = !busy();
        if !done {
            core::hint::spin_loop();
        }
        done
    })
}

/// Data Register.
#[bitfield(u32, default = 0, debug)]
struct Data {
    /// Received or transmitted byte
    #[bits(0..=7, rw)]
    byte: u8,
}

/// Flag Register
#[bitfield(u32, default = 0, debug)]
struct Flags {
    /// The UART is transmitting: set from the first byte in the transmit FIFO
    /// until the last stop bit has left the shift register.
    #[bit(3, r)]
    busy: bool,

    /// The receive FIFO (the holding register when FIFOs are off) is empty.
    #[bit(4, r)]
    receive_fifo_empty: bool,

    /// The transmit FIFO (the holding register when FIFOs are off) is full.
    #[bit(5, r)]
    transmit_fifo_full: bool,
}

/// Integer baud rate divisor.
#[bitfield(u32, default = 0, debug)]
struct IntegerBaudRate {
    #[bits(0..=15, rw)]
    divisor: u16,
}

/// Fractional baud rate divisor.
#[bitfield(u32, default = 0, debug)]
struct FractionalBaudRate {
    #[bits(0..=5, rw)]
    divisor: u6,
}

/// Word length: the number of data bits in a frame.
#[bitenum(u2, exhaustive = true)]
#[derive(Debug)]
enum WordLength {
    Five = 0b00,
    Six = 0b01,
    Seven = 0b10,
    Eight = 0b11,
}

/// Line Control register.
#[bitfield(u32, default = 0, debug)]
struct LineControl {
    #[bit(1, rw)]
    parity: bool,

    #[bit(3, rw)]
    two_stop_bits: bool,

    #[bit(4, rw)]
    fifos: bool,

    #[bits(5..=6, rw)]
    word_length: WordLength,
}

/// Control Register.
#[bitfield(u32, default = 0, debug)]
struct Control {
    #[bit(0, rw)]
    uart_enable: bool,

    #[bit(8, rw)]
    transmit_enable: bool,

    #[bit(9, rw)]
    receive_enable: bool,
}

/// Interrupt sources as the mask and clear registers lay them out: all eleven.
#[bitfield(u32, default = 0, debug)]
struct InterruptSources {
    #[bits(0..=10, rw)]
    all: u11,
}

/// DMA Control Register.
#[bitfield(u32, default = 0, debug)]
struct DmaControl {
    #[bit(0, rw)]
    receive_dma_enable: bool,

    #[bit(1, rw)]
    transmit_dma_enable: bool,
}

// https://developer.arm.com/documentation/ddi0183/g/programmers-model/summary-of-registers?lang=en
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
struct RegisterBlock {
    #[mmio(Read, Write)]
    data: Data,
    _reserved1: [u32; 5],
    #[mmio(PureRead)]
    flag: Flags,
    _reserved2: [u32; 2],
    #[mmio(Write)]
    integer_baud_rate: IntegerBaudRate,
    #[mmio(Write)]
    fractional_baud_rate: FractionalBaudRate,
    line_control: LineControl,
    #[mmio(Write)]
    control: Control,
    _reserved3: [u32; 1],
    #[mmio(Write)]
    interrupt_mask: InterruptSources,
    _reserved4: [u32; 2],
    #[mmio(Write)]
    interrupt_clear: InterruptSources,
    #[mmio(Write)]
    dma_control: DmaControl,
    _reserved5: [u32; 1005],
}

assert_register_offsets!(RegisterBlock, 0x1000, {
    data: 0x00,
    flag: 0x18,
    integer_baud_rate: 0x24,
    fractional_baud_rate: 0x28,
    line_control: 0x2c,
    control: 0x30,
    interrupt_mask: 0x38,
    interrupt_clear: 0x44,
    dma_control: 0x48,
});

/// Integer and fractional divisors for `baud_rate` with a UART clock of
/// `clock` Hz: the clock over sixteen times the rate, as a 16.6 fixed point
/// number, which is `64 * clock / (16 * rate)`, rounded to the nearest.
#[allow(
    clippy::cast_possible_truncation,
    reason = "the integer part is checked against the field's width, the fraction is masked to six bits"
)]
const fn divisors(clock: u32, baud_rate: u32) -> Option<(u16, u8)> {
    let value = (8 * clock as u64 / baud_rate as u64).div_ceil(2);
    let integer = value >> 6;
    if integer == 0 || integer > u16::MAX as u64 {
        return None;
    }
    Some((integer as u16, (value & 0x3f) as u8))
}

// The 4 MHz clock at 115200 baud: 2 + 11/64, a rate 0.08% slow. The 48 MHz
// default: 26 + 3/64, 0.015% slow.
const _: () = assert!(matches!(divisors(4_000_000, BAUD_RATE), Some((2, 11))));
const _: () = assert!(matches!(divisors(48_000_000, BAUD_RATE), Some((26, 3))));

/// A PL011 UART.
pub struct Pl011 {
    registers: IRQSafeNullLock<MmioRegisterBlock<'static>>,
}

impl Pl011 {
    /// The PL011 whose registers start at `base`.
    ///
    /// # Safety
    ///
    /// `base` is the address of a PL011's registers, accessible for as long
    /// as the program runs, and nothing else uses the UART.
    pub const unsafe fn new(base: usize) -> Self {
        Self {
            registers: IRQSafeNullLock::new(
                // SAFETY: the caller's contract.
                unsafe { RegisterBlock::new_mmio(base as *mut RegisterBlock) },
            ),
        }
    }

    /// Set the UART up, running at `clock` Hz: 115200 8N1 with FIFOs, every
    /// interrupt masked and no DMA, then turn it on. The GPIO pins have to be
    /// routed to it already.
    ///
    /// # Errors
    ///
    /// If the clock and the rate give no divisors the registers hold.
    pub fn init(&self, clock: u32) -> Result<(), &'static str> {
        let (integer, fraction) =
            divisors(clock, BAUD_RATE).ok_or("PL011 baud rate divisor out of range")?;
        let fraction =
            u6::try_new(fraction).map_err(|_error| "PL011 fractional divisor too wide")?;
        self.registers.lock(|registers| {
            // Turn the UART off and let the character in flight finish.
            registers.write_control(Control::ZERO);
            let _settled = spin_while(|| registers.read_flag().busy());
            registers.modify_line_control(|line| line.with_fifos(false));
            registers.write_interrupt_clear(InterruptSources::ZERO.with_all(u11::new(0x7ff)));

            // The divisors take effect on the write to the line control register.
            registers.write_integer_baud_rate(IntegerBaudRate::ZERO.with_divisor(integer));
            registers.write_fractional_baud_rate(FractionalBaudRate::ZERO.with_divisor(fraction));
            registers.write_line_control(
                LineControl::ZERO
                    .with_word_length(WordLength::Eight)
                    .with_fifos(true)
                    .with_parity(false)
                    .with_two_stop_bits(false),
            );

            // Chainboot polls: no interrupts, no DMA.
            registers.write_interrupt_mask(InterruptSources::ZERO);
            registers.write_dma_control(
                DmaControl::ZERO
                    .with_receive_dma_enable(false)
                    .with_transmit_dma_enable(false),
            );

            registers.write_control(
                Control::ZERO
                    .with_uart_enable(true)
                    .with_transmit_enable(true)
                    .with_receive_enable(true),
            );
        });
        Ok(())
    }
}

impl SerialOps for Pl011 {
    fn read_byte(&self) -> u8 {
        self.registers.lock(|registers| {
            loop_while(|| registers.read_flag().receive_fifo_empty());
            registers.read_data().byte()
        })
    }

    fn read_byte_nonblocking(&self) -> Option<u8> {
        self.registers.lock(|registers| {
            (!registers.read_flag().receive_fifo_empty()).then(|| registers.read_data().byte())
        })
    }

    fn write_byte(&self, byte: u8) {
        if TRANSMITTER_DEAD.load(Ordering::Relaxed) {
            return;
        }
        self.registers.lock(|registers| {
            if spin_while(|| registers.read_flag().transmit_fifo_full()) {
                registers.write_data(Data::ZERO.with_byte(byte));
            } else {
                TRANSMITTER_DEAD.store(true, Ordering::Relaxed);
            }
        });
    }

    /// Wait until every character has been put on the line.
    fn flush(&self) {
        if TRANSMITTER_DEAD.load(Ordering::Relaxed) {
            return;
        }
        self.registers.lock(|registers| {
            if !spin_while(|| registers.read_flag().busy()) {
                TRANSMITTER_DEAD.store(true, Ordering::Relaxed);
            }
        });
    }

    /// Consume input until the receive FIFO is empty, or has been read through
    /// four times over: a line held low keeps filling it with break characters.
    fn clear_rx(&self) {
        /// Characters the receive FIFO holds.
        const FIFO_DEPTH: u32 = 16;

        self.registers.lock(|registers| {
            for _ in 0..FIFO_DEPTH * 4 {
                if registers.read_flag().receive_fifo_empty() {
                    break;
                }
                let _discarded = registers.read_data();
            }
        });
    }
}

impl interface::ConsoleOps for Pl011 {}

impl interface::Write for Pl011 {
    fn write_fmt(&self, args: fmt::Arguments) -> fmt::Result {
        /// The UART as text to format into.
        struct Text<'uart>(&'uart Pl011);

        impl fmt::Write for Text<'_> {
            fn write_str(&mut self, text: &str) -> fmt::Result {
                interface::ConsoleOps::write_string(self.0, text);
                Ok(())
            }
        }

        fmt::Write::write_fmt(&mut Text(self), args)
    }
}

impl interface::All for Pl011 {}

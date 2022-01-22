/*
 * SPDX-License-Identifier: MIT OR BlueOak-1.0.0
 * Copyright (c) 2018-2019 Andre Richter <andre.o.richter@gmail.com>
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 * Original code distributed under MIT, additional changes are under BlueOak-1.0.0
 */

#[cfg(not(feature = "noserial"))]
use tock_registers::interfaces::{Readable, Writeable};
use {
    super::{gpio, BcmHost},
    crate::{
        devices::{ConsoleOps, SerialOps},
        platform::MMIODerefWrapper,
    },
    cfg_if::cfg_if,
    core::{convert::From, fmt},
    tock_registers::{
        interfaces::ReadWriteable,
        register_bitfields, register_structs,
        registers::{ReadOnly, ReadWrite, WriteOnly},
    },
};

// Auxiliary mini UART registers
//
// Descriptions taken from
// https://github.com/raspberrypi/documentation/files/1888662/BCM2837-ARM-Peripherals.-.Revised.-.V2-1.pdf
register_bitfields! {
    u32,

    /// Auxiliary enables
    AUX_ENABLES [
        /// If set the mini UART is enabled. The UART will immediately
        /// start receiving data, especially if the UART1_RX line is
        /// low.
        /// If clear the mini UART is disabled. That also disables any
        /// mini UART register access
        MINI_UART_ENABLE OFFSET(0) NUMBITS(1) []
    ],

    /// Mini Uart Interrupt Identify
    AUX_MU_IIR [
        /// Writing with bit 1 set will clear the receive FIFO
        /// Writing with bit 2 set will clear the transmit FIFO
        FIFO_CLEAR OFFSET(1) NUMBITS(2) [
            Rx = 0b01,
            Tx = 0b10,
            All = 0b11
        ]
    ],

    /// Mini Uart Line Control
    AUX_MU_LCR [
        /// Mode the UART works in
        DATA_SIZE OFFSET(0) NUMBITS(2) [
            SevenBit = 0b00,
            EightBit = 0b11
        ]
    ],

    /// Mini Uart Line Status
    AUX_MU_LSR [
        /// This bit is set if the transmit FIFO is empty and the transmitter is
        /// idle. (Finished shifting out the last bit).
        TX_IDLE    OFFSET(6) NUMBITS(1) [],

        /// This bit is set if the transmit FIFO can accept at least
        /// one byte.
        TX_EMPTY   OFFSET(5) NUMBITS(1) [],

        /// This bit is set if the receive FIFO holds at least 1
        /// symbol.
        DATA_READY OFFSET(0) NUMBITS(1) []
    ],

    /// Mini Uart Extra Control
    AUX_MU_CNTL [
        /// If this bit is set the mini UART transmitter is enabled.
        /// If this bit is clear the mini UART transmitter is disabled.
        TX_EN OFFSET(1) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ],

        /// If this bit is set the mini UART receiver is enabled.
        /// If this bit is clear the mini UART receiver is disabled.
        RX_EN OFFSET(0) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ]
    ],

    /// Mini Uart Status
    AUX_MU_STAT [
        TX_DONE OFFSET(9) NUMBITS(1) [
            No = 0,
            Yes = 1
        ],

        /// This bit is set if the transmit FIFO can accept at least
        /// one byte.
        SPACE_AVAILABLE OFFSET(1) NUMBITS(1) [
            No = 0,
            Yes = 1
        ],

        /// This bit is set if the receive FIFO holds at least 1
        /// symbol.
        SYMBOL_AVAILABLE OFFSET(0) NUMBITS(1) [
            No = 0,
            Yes = 1
        ]
    ],

    /// Mini Uart Baud rate
    AUX_MU_BAUD [
        /// Mini UART baud rate counter
        RATE OFFSET(0) NUMBITS(16) []
    ]
}

register_structs! {
    #[allow(non_snake_case)]
    RegisterBlock {
        // 0x00 - AUX_IRQ?
        (0x00 => __reserved_1),
        (0x04 => AUX_ENABLES: ReadWrite<u32, AUX_ENABLES::Register>),
        (0x08 => __reserved_2),
        (0x40 => AUX_MU_IO: ReadWrite<u32>),//Mini Uart I/O Data
        (0x44 => AUX_MU_IER: WriteOnly<u32>),//Mini Uart Interrupt Enable
        (0x48 => AUX_MU_IIR: WriteOnly<u32, AUX_MU_IIR::Register>),
        (0x4c => AUX_MU_LCR: WriteOnly<u32, AUX_MU_LCR::Register>),
        (0x50 => AUX_MU_MCR: WriteOnly<u32>),
        (0x54 => AUX_MU_LSR: ReadOnly<u32, AUX_MU_LSR::Register>),
        // 0x58 - AUX_MU_MSR
        // 0x5c - AUX_MU_SCRATCH
        (0x58 => __reserved_3),
        (0x60 => AUX_MU_CNTL: WriteOnly<u32, AUX_MU_CNTL::Register>),
        (0x64 => AUX_MU_STAT: ReadOnly<u32, AUX_MU_STAT::Register>),
        (0x68 => AUX_MU_BAUD: WriteOnly<u32, AUX_MU_BAUD::Register>),
        (0x6c => @END),
    }
}

type Registers = MMIODerefWrapper<RegisterBlock>;

pub struct MiniUart {
    registers: Registers,
}

pub struct PreparedMiniUart(MiniUart);

/// Divisor values for common baud rates
pub enum Rate {
    Baud115200 = 270,
}

impl From<Rate> for u32 {
    fn from(r: Rate) -> Self {
        r as u32
    }
}

// [temporary] Used in mmu.rs to set up local paging
pub const UART1_START: usize = 0x21_5000;

impl Default for MiniUart {
    fn default() -> Self {
        const UART1_BASE: usize = BcmHost::get_peripheral_address() + UART1_START;
        unsafe { MiniUart::new(UART1_BASE) }
    }
}

impl MiniUart {
    /// # Safety
    ///
    /// Unsafe, duh!
    pub const unsafe fn new(base_addr: usize) -> MiniUart {
        MiniUart {
            registers: Registers::new(base_addr),
        }
    }
}

impl MiniUart {
    cfg_if! {
        if #[cfg(not(feature = "noserial"))] {
            /// Set baud rate and characteristics (115200 8N1) and map to GPIO
            pub fn prepare(self, gpio: &gpio::GPIO) -> PreparedMiniUart {
                // GPIO pins should be set up first before enabling the UART

                // Pin 14
                const MINI_UART_TXD: gpio::Function = gpio::Function::Alt5;
                // Pin 15
                const MINI_UART_RXD: gpio::Function = gpio::Function::Alt5;

                // map UART1 to GPIO pins
                gpio.get_pin(14).into_alt(MINI_UART_TXD).set_pull_up_down(gpio::PullUpDown::Up);
                gpio.get_pin(15).into_alt(MINI_UART_RXD).set_pull_up_down(gpio::PullUpDown::Up);

                // initialize UART
                self.registers.AUX_ENABLES.modify(AUX_ENABLES::MINI_UART_ENABLE::SET);
                self.registers.AUX_MU_IER.set(0);
                self.registers.AUX_MU_CNTL.set(0);
                self.registers.AUX_MU_LCR.write(AUX_MU_LCR::DATA_SIZE::EightBit);
                self.registers.AUX_MU_MCR.set(0);
                self.registers.AUX_MU_IER.set(0);
                self.registers.AUX_MU_BAUD
                    .write(AUX_MU_BAUD::RATE.val(Rate::Baud115200.into()));

                // Clear FIFOs before using the device
                self.registers.AUX_MU_IIR.write(AUX_MU_IIR::FIFO_CLEAR::All);

                self.registers.AUX_MU_CNTL
                    .write(AUX_MU_CNTL::RX_EN::Enabled + AUX_MU_CNTL::TX_EN::Enabled);

                PreparedMiniUart(self)
            }
        } else {
            pub fn prepare(self, _gpio: &gpio::GPIO) -> PreparedMiniUart {
                PreparedMiniUart(self)
            }
        }
    }
}

impl Drop for PreparedMiniUart {
    fn drop(&mut self) {
        self.0
            .registers
            .AUX_ENABLES
            .modify(AUX_ENABLES::MINI_UART_ENABLE::CLEAR);
        // @todo disable gpio.PUD ?
    }
}

impl SerialOps for PreparedMiniUart {
    cfg_if! {
        if #[cfg(not(feature = "noserial"))] {
            /// Receive a byte without console translation
            fn read_byte(&self) -> u8 {
                // wait until something is in the buffer
                crate::arch::loop_until(|| self.0.registers.AUX_MU_STAT.is_set(AUX_MU_STAT::SYMBOL_AVAILABLE));

                // read it and return
                self.0.registers.AUX_MU_IO.get() as u8
            }

            fn write_byte(&self, b: u8) {
                // wait until we can send
                crate::arch::loop_until(|| self.0.registers.AUX_MU_STAT.is_set(AUX_MU_STAT::SPACE_AVAILABLE));

                // write the character to the buffer
                self.0.registers.AUX_MU_IO.set(b as u32);
            }

            /// Wait until the TX FIFO is empty, aka all characters have been put on the
            /// line.
            fn flush(&self) {
                crate::arch::loop_until(|| self.0.registers.AUX_MU_STAT.is_set(AUX_MU_STAT::TX_DONE));
            }

            /// Consume input until RX FIFO is empty, aka all pending characters have been
            /// consumed.
            fn clear_rx(&self) {
                crate::arch::loop_while(|| {
                    let pending = self.0.registers.AUX_MU_STAT.is_set(AUX_MU_STAT::SYMBOL_AVAILABLE);
                    if pending { self.read_byte(); }
                    pending
                });
            }
        } else {
            fn read_byte(&self) -> u8 { 0 }
            fn write_byte(&self, _byte: u8) {}
            fn flush(&self) {}
            fn clear_rx(&self) {}
        }
    }
}

impl ConsoleOps for PreparedMiniUart {
    cfg_if! {
        if #[cfg(not(feature = "noserial"))] {
            /// Send a character
            fn write_char(&self, c: char) {
                self.write_byte(c as u8);
            }

            /// Display a string
            fn write_string(&self, string: &str) {
                for c in string.chars() {
                    // convert newline to carriage return + newline
                    if c == '\n' {
                        self.write_char('\r')
                    }

                    self.write_char(c);
                }
            }

            /// Receive a character
            fn read_char(&self) -> char {
                let mut ret = self.read_byte() as char;

                // convert carriage return to newline -- this doesn't work well for reading binaries...
                if ret == '\r' {
                    ret = '\n'
                }

                ret
            }
        } else {
            fn write_char(&self, _c: char) {}
            fn write_string(&self, _string: &str) {}
            fn read_char(&self) -> char {
                '\n'
            }
        }
    }
}

impl fmt::Write for PreparedMiniUart {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

/*
 * SPDX-License-Identifier: MIT OR BlueOak-1.0.0
 * Copyright (c) 2018-2019 Andre Richter <andre.o.richter@gmail.com>
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 * Original code distributed under MIT, additional changes are under BlueOak-1.0.0
 */

use {
    super::{gpio, BcmHost},
    crate::devices::ConsoleOps,
    cfg_if::cfg_if,
    core::{convert::From, fmt, ops},
    tock_registers::{
        interfaces::{ReadWriteable, Readable, Writeable},
        register_bitfields,
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

    /// Mini Uart Baudrate
    AUX_MU_BAUD [
        /// Mini UART baudrate counter
        RATE OFFSET(0) NUMBITS(16) []
    ]
}

#[allow(non_snake_case)]
#[repr(C)]
pub struct RegisterBlock {
    __reserved_0: u32,                                  // 0x00 - AUX_IRQ?
    AUX_ENABLES: ReadWrite<u32, AUX_ENABLES::Register>, // 0x04
    __reserved_1: [u32; 14],                            // 0x08
    AUX_MU_IO: ReadWrite<u32>,                          // 0x40 - Mini Uart I/O Data
    AUX_MU_IER: WriteOnly<u32>,                         // 0x44 - Mini Uart Interrupt Enable
    AUX_MU_IIR: WriteOnly<u32, AUX_MU_IIR::Register>,   // 0x48
    AUX_MU_LCR: WriteOnly<u32, AUX_MU_LCR::Register>,   // 0x4C
    AUX_MU_MCR: WriteOnly<u32>,                         // 0x50
    AUX_MU_LSR: ReadOnly<u32, AUX_MU_LSR::Register>,    // 0x54
    __reserved_2: [u32; 2],                             // 0x58 - AUX_MU_MSR, AUX_MU_SCRATCH
    AUX_MU_CNTL: WriteOnly<u32, AUX_MU_CNTL::Register>, // 0x60
    __reserved_3: u32,                                  // 0x64 - AUX_MU_STAT
    AUX_MU_BAUD: WriteOnly<u32, AUX_MU_BAUD::Register>, // 0x68
}

pub struct MiniUart {
    base_addr: usize,
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

/// Deref to RegisterBlock
///
/// Allows writing
/// ```
/// self.MU_IER.read()
/// ```
/// instead of something along the lines of
/// ```
/// unsafe { (*MiniUart::ptr()).MU_IER.read() }
/// ```
impl ops::Deref for MiniUart {
    type Target = RegisterBlock;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr() }
    }
}

// [temporary] Used in mmu.rs to set up local paging
pub const UART1_BASE: usize = BcmHost::get_peripheral_address() + 0x21_5000;

impl Default for MiniUart {
    fn default() -> MiniUart {
        MiniUart::new(UART1_BASE)
    }
}

impl MiniUart {
    pub fn new(base_addr: usize) -> MiniUart {
        MiniUart { base_addr }
    }

    /// Returns a pointer to the register block
    fn ptr(&self) -> *const RegisterBlock {
        self.base_addr as *const _
    }
}

impl MiniUart {
    cfg_if! {
        if #[cfg(not(feature = "noserial"))] {
            /// Set baud rate and characteristics (115200 8N1) and map to GPIO
            pub fn prepare(self, gpio: &gpio::GPIO) -> PreparedMiniUart {
                // initialize UART
                self.AUX_ENABLES.modify(AUX_ENABLES::MINI_UART_ENABLE::SET);
                self.AUX_MU_IER.set(0);
                self.AUX_MU_CNTL.set(0);
                self.AUX_MU_LCR.write(AUX_MU_LCR::DATA_SIZE::EightBit);
                self.AUX_MU_MCR.set(0);
                self.AUX_MU_IER.set(0);
                self.AUX_MU_IIR.write(AUX_MU_IIR::FIFO_CLEAR::All);
                self.AUX_MU_BAUD
                    .write(AUX_MU_BAUD::RATE.val(Rate::Baud115200.into()));

                // Pin 14
                const MINI_UART_TXD: gpio::Function = gpio::Function::Alt5;
                // Pin 15
                const MINI_UART_RXD: gpio::Function = gpio::Function::Alt5;

                // map UART1 to GPIO pins
                gpio.get_pin(14).into_alt(MINI_UART_TXD);
                gpio.get_pin(15).into_alt(MINI_UART_RXD);

                gpio::enable_uart_pins(gpio);

                self.AUX_MU_CNTL
                    .write(AUX_MU_CNTL::RX_EN::Enabled + AUX_MU_CNTL::TX_EN::Enabled);

                // Clear FIFOs before using the device
                self.AUX_MU_IIR.write(AUX_MU_IIR::FIFO_CLEAR::All);

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
            .AUX_ENABLES
            .modify(AUX_ENABLES::MINI_UART_ENABLE::CLEAR);
        // @todo disable gpio.PUD ?
    }
}

impl ConsoleOps for PreparedMiniUart {
    cfg_if! {
        if #[cfg(not(feature = "noserial"))] {
            /// Send a character
            fn putc(&self, c: char) {
                // wait until we can send
                crate::arch::loop_until(|| self.0.AUX_MU_LSR.is_set(AUX_MU_LSR::TX_EMPTY));

                // write the character to the buffer
                self.0.AUX_MU_IO.set(c as u32);
            }

            /// Display a string
            fn puts(&self, string: &str) {
                for c in string.chars() {
                    // convert newline to carriage return + newline
                    if c == '\n' {
                        self.putc('\r')
                    }

                    self.putc(c);
                }
            }

            /// Receive a character
            fn getc(&self) -> char {
                // wait until something is in the buffer
                crate::arch::loop_until(|| self.0.AUX_MU_LSR.is_set(AUX_MU_LSR::DATA_READY));

                // read it and return
                let mut ret = self.0.AUX_MU_IO.get() as u8 as char;

                // convert carriage return to newline
                if ret == '\r' {
                    ret = '\n'
                }

                ret
            }

            /// Wait until the TX FIFO is empty, aka all characters have been put on the
            /// line.
            fn flush(&self) {
                crate::arch::loop_until(|| self.0.AUX_MU_LSR.is_set(AUX_MU_LSR::TX_IDLE));
            }
        } else {
            fn putc(&self, _c: char) {}
            fn puts(&self, _string: &str) {}
            fn getc(&self) -> char {
                '\n'
            }
            fn flush(&self) {}
        }
    }
}

impl fmt::Write for PreparedMiniUart {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.puts(s);
        Ok(())
    }
}

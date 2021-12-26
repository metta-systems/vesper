/*
 * SPDX-License-Identifier: MIT OR BlueOak-1.0.0
 * Copyright (c) 2018-2019 Andre Richter <andre.o.richter@gmail.com>
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 * Original code distributed under MIT, additional changes are under BlueOak-1.0.0
 *
 * http://infocenter.arm.com/help/topic/com.arm.doc.ddi0183g/DDI0183G_uart_pl011_r1p5_trm.pdf
 * https://docs.rs/embedded-serial/0.5.0/embedded_serial/
 */

use {
    super::{
        gpio,
        mailbox::{self, MailboxOps},
        BcmHost,
    },
    crate::{
        arch::loop_until,
        devices::{ConsoleOps, SerialOps},
        platform::MMIODerefWrapper,
    },
    snafu::Snafu,
    tock_registers::{
        interfaces::{Readable, Writeable},
        register_bitfields, register_structs,
        registers::{ReadOnly, ReadWrite, WriteOnly},
    },
};

// PL011 UART registers.
//
// Descriptions taken from
// https://github.com/raspberrypi/documentation/files/1888662/BCM2837-ARM-Peripherals.-.Revised.-.V2-1.pdf
register_bitfields! {
    u32,

    /// Flag Register
    FR [
        /// Transmit FIFO full. The meaning of this bit depends on the
        /// state of the FEN bit in the UARTLCR_ LCRH Register. If the
        /// FIFO is disabled, this bit is set when the transmit
        /// holding register is full. If the FIFO is enabled, the TXFF
        /// bit is set when the transmit FIFO is full.
        TXFF OFFSET(5) NUMBITS(1) [],

        /// Receive FIFO empty. The meaning of this bit depends on the
        /// state of the FEN bit in the UARTLCR_H Register. If the
        /// FIFO is disabled, this bit is set when the receive holding
        /// register is empty. If the FIFO is enabled, the RXFE bit is
        /// set when the receive FIFO is empty.
        RXFE OFFSET(4) NUMBITS(1) []
    ],

    /// Integer Baud rate divisor
    IBRD [
        /// Integer Baud rate divisor
        IBRD OFFSET(0) NUMBITS(16) []
    ],

    /// Fractional Baud rate divisor
    FBRD [
        /// Fractional Baud rate divisor
        FBRD OFFSET(0) NUMBITS(6) []
    ],

    /// Line Control register
    LCRH [
        /// Word length. These bits indicate the number of data bits
        /// transmitted or received in a frame.
        WLEN OFFSET(5) NUMBITS(2) [
            FiveBit = 0b00,
            SixBit = 0b01,
            SevenBit = 0b10,
            EightBit = 0b11
        ]
    ],

    /// Control Register
    CR [
        /// Receive enable. If this bit is set to 1, the receive
        /// section of the UART is enabled. Data reception occurs for
        /// UART signals. When the UART is disabled in the middle of
        /// reception, it completes the current character before
        /// stopping.
        RXE    OFFSET(9) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ],

        /// Transmit enable. If this bit is set to 1, the transmit
        /// section of the UART is enabled. Data transmission occurs
        /// for UART signals. When the UART is disabled in the middle
        /// of transmission, it completes the current character before
        /// stopping.
        TXE    OFFSET(8) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ],

        /// UART enable
        UARTEN OFFSET(0) NUMBITS(1) [
            /// If the UART is disabled in the middle of transmission
            /// or reception, it completes the current character
            /// before stopping.
            Disabled = 0,
            Enabled = 1
        ]
    ],

    /// Interupt Clear Register
    ICR [
        /// Meta field for all pending interrupts
        ALL OFFSET(0) NUMBITS(11) []
    ]
}

register_structs! {
    #[allow(non_snake_case)]
    RegisterBlock {
        (0x00 => DR: ReadWrite<u32>),
        (0x04 => __reserved_1), // (UART0_RSRECR=0x04)
        (0x18 => FR: ReadOnly<u32, FR::Register>),
        (0x1c => __reserved_2),
        (0x20 => ILPR: u32),
        (0x24 => IBRD: WriteOnly<u32, IBRD::Register>),
        (0x28 => FBRD: WriteOnly<u32, FBRD::Register>),
        (0x2c => LCRH: WriteOnly<u32, LCRH::Register>),
        (0x30 => CR: WriteOnly<u32, CR::Register>),
        (0x34 => IFLS: u32),
        (0x38 => IMSC: u32),
        (0x3c => RIS: u32),
        (0x40 => MIS: u32),
        (0x44 => ICR: WriteOnly<u32, ICR::Register>),
        (0x48 => DMACR: u32),
        (0x4c => __reserved_3),
        (0x80 => ITCR: u32),
        (0x84 => ITIP: u32),
        (0x88 => ITOP: u32),
        (0x8c => TDR: u32),
        (0x90 => @END),
    }
}

#[derive(Debug, Snafu)]
pub enum PL011UartError {
    #[snafu(display("PL011 UART setup failed in mailbox operation"))]
    MailboxError,
}

pub type Result<T> = ::core::result::Result<T, PL011UartError>;

type Registers = MMIODerefWrapper<RegisterBlock>;

pub struct PL011Uart {
    registers: Registers,
}

pub struct PreparedPL011Uart(PL011Uart);

/// Divisor values for common baud rates
pub enum Rate {
    Baud115200 = 2,
}

impl From<Rate> for u32 {
    fn from(r: Rate) -> Self {
        r as u32
    }
}

pub const UART0_START: usize = 0x20_1000;

impl Default for PL011Uart {
    fn default() -> Self {
        const UART0_BASE: usize = BcmHost::get_peripheral_address() + UART0_START;
        unsafe { PL011Uart::new(UART0_BASE) }
    }
}

impl PL011Uart {
    /// # Safety
    ///
    /// Unsafe, duh!
    pub const unsafe fn new(base_addr: usize) -> PL011Uart {
        PL011Uart {
            registers: Registers::new(base_addr),
        }
    }

    /// Set baud rate and characteristics (115200 8N1) and map to GPIO
    pub fn prepare(
        self,
        mut mbox: mailbox::Mailbox,
        gpio: &gpio::GPIO,
    ) -> Result<PreparedPL011Uart> {
        // turn off UART0
        self.registers.CR.set(0);

        // set up clock for consistent divisor values
        let index = mbox.request();
        let index = mbox.set_clock_rate(index, mailbox::clock::UART, 4_000_000 /* 4Mhz */);
        let mbox = mbox.end(index);

        if mbox.call(mailbox::channel::PropertyTagsArmToVc).is_err() {
            return Err(PL011UartError::MailboxError); // Abort if UART clocks couldn't be set
        };

        // Pin 14
        const UART_TXD: gpio::Function = gpio::Function::Alt0;
        // Pin 15
        const UART_RXD: gpio::Function = gpio::Function::Alt0;

        // map UART0 to GPIO pins
        gpio.get_pin(14).into_alt(UART_TXD);
        gpio.get_pin(15).into_alt(UART_RXD);

        gpio.enable_uart_pins();

        self.registers.ICR.write(ICR::ALL::CLEAR);
        // @todo Configure divisors more sanely
        self.registers
            .IBRD
            .write(IBRD::IBRD.val(Rate::Baud115200.into()));
        self.registers.FBRD.write(FBRD::FBRD.val(0xB)); // Results in 115200 baud
        self.registers.LCRH.write(LCRH::WLEN::EightBit); // 8N1

        self.registers
            .CR
            .write(CR::UARTEN::Enabled + CR::TXE::Enabled + CR::RXE::Enabled);

        Ok(PreparedPL011Uart(self))
    }
}

impl Drop for PreparedPL011Uart {
    fn drop(&mut self) {
        self.0
            .registers
            .CR
            .write(CR::UARTEN::Disabled + CR::TXE::Disabled + CR::RXE::Disabled);
    }
}

impl SerialOps for PreparedPL011Uart {
    fn write_byte(&self, b: u8) {
        // wait until we can send
        loop_until(|| !self.0.registers.FR.is_set(FR::TXFF));

        // write the character to the buffer
        self.0.registers.DR.set(b as u32);
    }

    fn read_byte(&self) -> u8 {
        // wait until something is in the buffer
        loop_until(|| !self.0.registers.FR.is_set(FR::RXFE));

        // read it and return
        self.0.registers.DR.get() as u8
    }
}

impl ConsoleOps for PreparedPL011Uart {
    /// Send a character
    fn write_char(&self, c: char) {
        self.write_byte(c as u8)
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

        // convert carriage return to newline
        if ret == '\r' {
            ret = '\n'
        }

        ret
    }
}

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
    crate::{arch::loop_until, devices::ConsoleOps},
    core::ops,
    register::{mmio::*, register_bitfields},
    snafu::Snafu,
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

#[allow(non_snake_case)]
#[repr(C)]
pub struct RegisterBlock {
    DR: ReadWrite<u32>,                   // 0x00
    __reserved_0: [u32; 5],               // 0x04 (UART0_RSRECR=0x04)
    FR: ReadOnly<u32, FR::Register>,      // 0x18
    __reserved_1: [u32; 1],               // 0x1c
    ILPR: u32,                            // 0x20
    IBRD: WriteOnly<u32, IBRD::Register>, // 0x24
    FBRD: WriteOnly<u32, FBRD::Register>, // 0x28
    LCRH: WriteOnly<u32, LCRH::Register>, // 0x2C
    CR: WriteOnly<u32, CR::Register>,     // 0x30
    IFLS: u32,                            // 0x34
    IMSC: u32,                            // 0x38
    RIS: u32,                             // 0x3C
    MIS: u32,                             // 0x40
    ICR: WriteOnly<u32, ICR::Register>,   // 0x44
    DMACR: u32,                           // 0x48
    __reserved_2: [u32; 14],              // 0x4c-0x7c
    ITCR: u32,                            // 0x80
    ITIP: u32,                            // 0x84
    ITOP: u32,                            // 0x88
    TDR: u32,                             // 0x8C
}

#[derive(Debug, Snafu)]
pub enum PL011UartError {
    #[snafu(display("PL011 UART setup failed in mailbox operation"))]
    MailboxError,
}
pub type Result<T> = ::core::result::Result<T, PL011UartError>;

pub struct PL011Uart {
    base_addr: usize,
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

impl ops::Deref for PL011Uart {
    type Target = RegisterBlock;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr() }
    }
}

impl ops::Deref for PreparedPL011Uart {
    type Target = RegisterBlock;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.0.ptr() }
    }
}

impl Default for PL011Uart {
    fn default() -> Self {
        const UART0_BASE: usize = BcmHost::get_peripheral_address() + 0x20_1000;
        PL011Uart::new(UART0_BASE)
    }
}

impl PL011Uart {
    pub fn new(base_addr: usize) -> PL011Uart {
        PL011Uart { base_addr }
    }

    /// Returns a pointer to the register block
    fn ptr(&self) -> *const RegisterBlock {
        self.base_addr as *const _
    }

    /// Set baud rate and characteristics (115200 8N1) and map to GPIO
    pub fn prepare(
        self,
        mut mbox: mailbox::Mailbox,
        gpio: &gpio::GPIO,
    ) -> Result<PreparedPL011Uart> {
        // turn off UART0
        self.CR.set(0);

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

        gpio::enable_uart_pins(gpio);

        self.ICR.write(ICR::ALL::CLEAR);
        // @todo Configure divisors more sanely
        self.IBRD.write(IBRD::IBRD.val(Rate::Baud115200.into()));
        self.FBRD.write(FBRD::FBRD.val(0xB)); // Results in 115200 baud
        self.LCRH.write(LCRH::WLEN::EightBit); // 8N1

        self.CR
            .write(CR::UARTEN::Enabled + CR::TXE::Enabled + CR::RXE::Enabled);

        Ok(PreparedPL011Uart(self))
    }
}

impl Drop for PreparedPL011Uart {
    fn drop(&mut self) {
        self.CR
            .write(CR::UARTEN::Disabled + CR::TXE::Disabled + CR::RXE::Disabled);
    }
}

impl ConsoleOps for PreparedPL011Uart {
    /// Send a character
    fn putc(&self, c: char) {
        // wait until we can send
        loop_until(|| !self.FR.is_set(FR::TXFF));

        // write the character to the buffer
        self.DR.set(c as u32);
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
        loop_until(|| !self.FR.is_set(FR::RXFE));

        // read it and return
        let mut ret = self.DR.get() as u8 as char;

        // convert carriage return to newline
        if ret == '\r' {
            ret = '\n'
        }

        ret
    }
}

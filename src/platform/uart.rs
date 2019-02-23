/*
 * MIT License
 *
 * Copyright (c) 2018-2019 Andre Richter <andre.o.richter@gmail.com>
 * Copyright (c) 2019 Berkus Decker <berkus+github@metta.systems>
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */

use super::mailbox;
use crate::arch::{loop_delay, loop_until};
use crate::devices::ConsoleOps;
use crate::platform::{gpio, rpi3::BcmHost};
use core::{convert::TryFrom, ops};
use register::{mmio::*, register_bitfields};

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

pub enum PL011UartError {
    MailboxError,
}
pub type Result<T> = ::core::result::Result<T, PL011UartError>;

pub struct PL011Uart {
    base_addr: usize,
}

impl ops::Deref for PL011Uart {
    type Target = RegisterBlock;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr() }
    }
}

impl PL011Uart {
    pub fn new_default() -> PL011Uart {
        const UART0_BASE: u32 = BcmHost::get_peripheral_address() + 0x20_1000;
        PL011Uart {
            base_addr: usize::try_from(UART0_BASE).unwrap(),
        }
    }

    pub fn new(base_addr: usize) -> PL011Uart {
        PL011Uart { base_addr }
    }

    /// Returns a pointer to the register block
    fn ptr(&self) -> *const RegisterBlock {
        self.base_addr as *const _
    }

    /// Set baud rate and characteristics (115200 8N1) and map to GPIO
    pub fn init(&self, mbox: &mut mailbox::Mailbox, gpio: &gpio::GPIO) -> Result<()> {
        // turn off UART0
        self.CR.set(0);

        // set up clock for consistent divisor values
        mbox.buffer[0] = 9 * 4;
        mbox.buffer[1] = mailbox::REQUEST;
        mbox.buffer[2] = mailbox::tag::SetClockRate;
        mbox.buffer[3] = 12;
        mbox.buffer[4] = 8;
        mbox.buffer[5] = mailbox::clock::UART; // UART clock
        mbox.buffer[6] = 4_000_000; // 4Mhz
        mbox.buffer[7] = 0; // skip turbo setting
        mbox.buffer[8] = mailbox::tag::End;

        if mbox.call(mailbox::channel::PropertyTagsArmToVc).is_err() {
            return Err(PL011UartError::MailboxError); // Abort if UART clocks couldn't be set
        };

        // map UART0 to GPIO pins
        gpio.GPFSEL1
            .modify(gpio::GPFSEL1::FSEL14::TXD0 + gpio::GPFSEL1::FSEL15::RXD0);

        gpio.GPPUD.set(0); // enable pins 14 and 15
        loop_delay(150);

        gpio.GPPUDCLK0.modify(
            gpio::GPPUDCLK0::PUDCLK14::AssertClock + gpio::GPPUDCLK0::PUDCLK15::AssertClock,
        );
        loop_delay(150);

        gpio.GPPUDCLK0.set(0);

        self.ICR.write(ICR::ALL::CLEAR);
        self.IBRD.write(IBRD::IBRD.val(2)); // Results in 115200 baud
        self.FBRD.write(FBRD::FBRD.val(0xB));
        self.LCRH.write(LCRH::WLEN::EightBit); // 8N1

        self.CR
            .write(CR::UARTEN::Enabled + CR::TXE::Enabled + CR::RXE::Enabled);

        Ok(())
    }
}

impl Drop for PL011Uart {
    fn drop(&mut self) {
        self.CR
            .write(CR::UARTEN::Disabled + CR::TXE::Disabled + CR::RXE::Disabled);
    }
}

impl ConsoleOps for PL011Uart {
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
            // convert newline to carrige return + newline
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

        // convert carrige return to newline
        if ret == '\r' {
            ret = '\n'
        }

        ret
    }
}

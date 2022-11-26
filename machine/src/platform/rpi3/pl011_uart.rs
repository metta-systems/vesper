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
    super::{gpio, BcmHost},
    crate::{
        arch::loop_while,
        devices::{ConsoleOps, SerialOps},
        platform::MMIODerefWrapper,
        qemu::QemuConsole,
    },
    snafu::Snafu,
    tock_registers::{
        interfaces::{ReadWriteable, Readable, Writeable},
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
        /// Transmit FIFO empty. The meaning of this bit depends on the
        /// state of the FEN bit in the Line Control Register, If the
        /// FIFO is disabled, this bit is set when the transmit holding
        /// register is empty. If the FIFO is enabled, the TXFE bit is
        /// set when the transmit FIFO is empty. This bit does not indicate
        /// if there is data in the transmit shift register.
        TXFE OFFSET(7) NUMBITS(1) [],

        /// Receive FIFO full. The meaning of this bit depends on the
        /// state of the FEN bit in the LCRH Register. If the FIFO is
        /// disabled, this bit is set when the receive holding register
        /// is full. If the FIFO is enabled, the RXFF bit is set when
        /// the receive FIFO is full.
        RXFF OFFSET(6) NUMBITS(1) [],

        /// Transmit FIFO full. The meaning of this bit depends on the
        /// state of the FEN bit in the LCRH Register. If the
        /// FIFO is disabled, this bit is set when the transmit
        /// holding register is full. If the FIFO is enabled, the TXFF
        /// bit is set when the transmit FIFO is full.
        TXFF OFFSET(5) NUMBITS(1) [],

        /// Receive FIFO empty. The meaning of this bit depends on the
        /// state of the FEN bit in the LCRH Register. If the
        /// FIFO is disabled, this bit is set when the receive holding
        /// register is empty. If the FIFO is enabled, the RXFE bit is
        /// set when the receive FIFO is empty.
        RXFE OFFSET(4) NUMBITS(1) [],

        /// UART busy. If this bit is set to 1, the UART is busy
        /// transmitting data. This bit remains set until the complete
        /// byte, including all the stop bits, has been sent from the
        /// shift register. This bit is set as soon as the transmit FIFO
        /// becomes non-empty, regardless of whether the UART is enabled or not.
        BUSY OFFSET(3) NUMBITS(1) []
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
        Parity OFFSET(1) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ],

        /// Use 2 stop bits
        Stop2 OFFSET(3) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ],

        Fifo OFFSET(4) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ],

        /// Word length. These bits indicate the number of data bits
        /// transmitted or received in a frame.
        WordLength OFFSET(5) NUMBITS(2) [
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
    ],

    /// Interupt Mask Set/Clear Register
    IMSC [
        /// Meta field for all interrupts
        ALL OFFSET(0) NUMBITS(11) []
    ],

    /// DMA Control Register
    DMACR [
        // RX DMA enabled
        RXDMAE OFFSET(0) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ],

        // TX DMA enabled
        TXDMAE OFFSET(0) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ],
    ]
}

// https://developer.arm.com/documentation/ddi0183/g/programmers-model/summary-of-registers?lang=en
register_structs! {
    #[allow(non_snake_case)]
    RegisterBlock {
        (0x00 => Data: ReadWrite<u32>), // DR
        (0x04 => Status: ReadWrite<u32>), // RSR/ECR
        (0x08 => __reserved_1),
        (0x18 => Flag: ReadOnly<u32, FR::Register>),
        (0x1c => __reserved_2),
        (0x24 => IntegerBaudRate: ReadWrite<u32, IBRD::Register>),
        (0x28 => FractionalBaudRate: ReadWrite<u32, FBRD::Register>),
        (0x2c => LineControl: ReadWrite<u32, LCRH::Register>),
        (0x30 => Control: ReadWrite<u32, CR::Register>),
        (0x34 => InterruptFifoLevelSelect: ReadWrite<u32>),
        (0x38 => InterruptMaskSetClear: ReadWrite<u32, IMSC::Register>),
        (0x3c => RawInterruptStatus: ReadOnly<u32>),
        (0x40 => MaskedInterruptStatus: ReadOnly<u32>),
        (0x44 => InterruptClear: WriteOnly<u32, ICR::Register>),
        (0x48 => DmaControl: ReadWrite<u32, DMACR::Register>),
        (0x4c => __reserved_3),
        (0x1000 => @END),
    }
}

#[derive(Debug, Snafu)]
pub enum PL011UartError {
    #[snafu(display("PL011 UART setup failed in mailbox operation"))]
    MailboxError,
    #[snafu(display(
        "PL011 UART setup failed due to integer baud rate divisor out of range ({})",
        ibrd
    ))]
    InvalidIntegerDivisor { ibrd: u32 },
    #[snafu(display(
        "PL011 UART setup failed due to fractional baud rate divisor out of range ({})",
        fbrd
    ))]
    InvalidFractionalDivisor { fbrd: u32 },
}

pub type Result<T> = ::core::result::Result<T, PL011UartError>;

type Registers = MMIODerefWrapper<RegisterBlock>;

pub struct PL011Uart {
    registers: Registers,
}

pub struct PreparedPL011Uart(PL011Uart);

pub struct RateDivisors {
    integer_baud_rate_divisor: u32,
    fractional_baud_rate_divisor: u32,
}

impl RateDivisors {
    // Set integer & fractional part of baud rate.
    // Integer = clock/(16 * Baud)
    // e.g. 3000000 / (16 * 115200) = 1.627 = ~1.
    // Fraction = (Fractional part * 64) + 0.5
    // e.g. (.627 * 64) + 0.5 = 40.6 = ~40.
    //
    // Use integer-only calculation based on [this page](https://krinkinmu.github.io/2020/11/29/PL011.html)
    // Calculate 64 * clock / (16 * rate) = 4 * clock / rate, then extract 6 lowest bits for fractional part
    // and the next 16 bits for integer part.
    pub fn from_clock_and_rate(clock: u64, baud_rate: u32) -> Result<RateDivisors> {
        let value = 4 * clock / baud_rate as u64;
        let i = ((value >> 6) & 0xffff) as u32;
        let f = (value & 0x3f) as u32;
        // TODO: check for integer overflow, i.e. any bits set above the 0x3fffff mask.
        // FIXME: can't happen due to calculation above
        if i > 65535 {
            return Err(PL011UartError::InvalidIntegerDivisor { ibrd: i });
        }
        // FIXME: can't happen due to calculation above
        if f > 63 {
            return Err(PL011UartError::InvalidFractionalDivisor { fbrd: f });
        }
        Ok(RateDivisors {
            integer_baud_rate_divisor: i,
            fractional_baud_rate_divisor: f,
        })
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
    pub fn prepare(self, gpio: &gpio::GPIO) -> Result<PreparedPL011Uart> {
        // Turn off UART
        self.registers.Control.set(0);

        // Wait for any ongoing transmissions to complete
        self.flush_internal();

        // Flush TX FIFO
        self.registers.LineControl.modify(LCRH::Fifo::Disabled);

        // set up clock for consistent divisor values
        const CLOCK: u32 = 4_000_000; // 4Mhz
        const BAUD_RATE: u32 = 115_200;

        //====================================================================================
        use super::mailbox::{self, Mailbox, MailboxOps};
        let mut mailbox = Mailbox::<9>::default();
        let index = mailbox.request();
        let index = mailbox.set_clock_rate(index, mailbox::clock::UART, CLOCK);
        let mailbox = mailbox.end(index);

        if mailbox.call(mailbox::channel::PropertyTagsArmToVc).is_err() {
            let con = QemuConsole {};
            con.write_string("mailbox call failed!");
            return Err(PL011UartError::MailboxError); // Abort if UART clocks couldn't be set
        };
        //====================================================================================

        // Pin 14
        const UART_TXD: gpio::Function = gpio::Function::Alt0;
        // Pin 15
        const UART_RXD: gpio::Function = gpio::Function::Alt0;

        // Map UART0 to GPIO pins and enable pull-ups
        gpio.get_pin(14)
            .into_alt(UART_TXD)
            .set_pull_up_down(gpio::PullUpDown::Up);
        gpio.get_pin(15)
            .into_alt(UART_RXD)
            .set_pull_up_down(gpio::PullUpDown::Up);

        // Clear pending interrupts
        self.registers.InterruptClear.write(ICR::ALL::SET);

        // From the PL011 Technical Reference Manual:
        //
        // The LCR_H, IBRD, and FBRD registers form the single 30-bit wide LCR Register that is
        // updated on a single write strobe generated by a LCR_H write. So, to internally update the
        // contents of IBRD or FBRD, a LCR_H write must always be performed at the end.
        //
        // Set the baud rate divisors, 8N1 and FIFO enabled.
        let divisors = RateDivisors::from_clock_and_rate(CLOCK.into(), BAUD_RATE)?;
        self.registers
            .IntegerBaudRate
            .write(IBRD::IBRD.val(divisors.integer_baud_rate_divisor & 0xffff));
        self.registers
            .FractionalBaudRate
            .write(FBRD::FBRD.val(divisors.fractional_baud_rate_divisor & 0b11_1111));
        self.registers.LineControl.write(
            LCRH::WordLength::EightBit
                + LCRH::Fifo::Enabled
                + LCRH::Parity::Disabled
                + LCRH::Stop2::Disabled,
        );

        // Mask all interrupts by setting corresponding bits to 1
        self.registers.InterruptMaskSetClear.write(IMSC::ALL::SET);

        // Disable DMA
        self.registers
            .DmaControl
            .write(DMACR::RXDMAE::Disabled + DMACR::TXDMAE::Disabled);

        // Turn on UART
        self.registers
            .Control
            .write(CR::UARTEN::Enabled + CR::TXE::Enabled + CR::RXE::Enabled);

        Ok(PreparedPL011Uart(self))
    }

    fn flush_internal(&self) {
        loop_while(|| self.registers.Flag.is_set(FR::BUSY));
    }
}

impl Drop for PreparedPL011Uart {
    fn drop(&mut self) {
        self.0.registers.Control.set(0);
    }
}

impl SerialOps for PreparedPL011Uart {
    fn read_byte(&self) -> u8 {
        // wait until something is in the buffer
        loop_while(|| self.0.registers.Flag.is_set(FR::RXFE));

        // read it and return
        self.0.registers.Data.get() as u8
    }

    fn write_byte(&self, b: u8) {
        // wait until we can send
        loop_while(|| self.0.registers.Flag.is_set(FR::TXFF));

        // write the character to the buffer
        self.0.registers.Data.set(b as u32);
    }

    /// Wait until the TX FIFO is empty, aka all characters have been put on the
    /// line.
    fn flush(&self) {
        self.0.flush_internal();
    }

    /// Consume input until RX FIFO is empty, aka all pending characters have been
    /// consumed.
    fn clear_rx(&self) {
        loop_while(|| {
            let pending = !self.0.registers.Flag.is_set(FR::RXFE);
            if pending {
                self.read_byte();
            }
            pending
        });
    }
}

// @todo Seems like a blanket implementation of ConsoleOps is in order..
impl ConsoleOps for PreparedPL011Uart {
    /// Send a character
    fn write_char(&self, c: char) {
        self.write_byte(c as u8) // @fixme write all chars of a unicode scalar value here!!
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test_case]
    fn test_divisors() {
        const CLOCK: u64 = 3_000_000;
        const BAUD_RATE: u32 = 115_200;

        let divisors = RateDivisors::from_clock_and_rate(CLOCK, BAUD_RATE);
        assert_eq!(divisors.integer_baud_rate_divisor, 1);
        assert_eq!(divisors.fractional_baud_rate_divisor, 40);
    }
}

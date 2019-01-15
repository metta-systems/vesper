use arch::*;
use core::ops;
use platform::{gpio, rpi3::PERIPHERAL_BASE};
use register::mmio::*;

// The base address for UART.
const UART0_BASE: u32 = PERIPHERAL_BASE + 0x20_1000;

// The offsets for reach register for the UART.
const UART0_DR: u32 = UART0_BASE + 0x00;
const UART0_RSRECR: u32 = UART0_BASE + 0x04;
const UART0_FR: u32 = UART0_BASE + 0x18;
const UART0_ILPR: u32 = UART0_BASE + 0x20;
const UART0_IBRD: u32 = UART0_BASE + 0x24;
const UART0_FBRD: u32 = UART0_BASE + 0x28;
const UART0_LCRH: u32 = UART0_BASE + 0x2C;
const UART0_CR: u32 = UART0_BASE + 0x30;
const UART0_IFLS: u32 = UART0_BASE + 0x34;
const UART0_IMSC: u32 = UART0_BASE + 0x38;
const UART0_RIS: u32 = UART0_BASE + 0x3C;
const UART0_MIS: u32 = UART0_BASE + 0x40;
const UART0_ICR: u32 = UART0_BASE + 0x44;
const UART0_DMACR: u32 = UART0_BASE + 0x48;
const UART0_ITCR: u32 = UART0_BASE + 0x80;
const UART0_ITIP: u32 = UART0_BASE + 0x84;
const UART0_ITOP: u32 = UART0_BASE + 0x88;
const UART0_TDR: u32 = UART0_BASE + 0x8C;

// Mini UART
const UART1_BASE: u32 = PERIPHERAL_BASE + 0x21_5000;

#[allow(non_snake_case)]
#[repr(C)]
pub struct RegisterBlock {
    __reserved_0: u32,                                  // 0x00 - AUX_IRQ
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

pub struct MiniUart;

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
        unsafe { &*Self::ptr() }
    }
}

impl MiniUart {
    pub fn new() -> MiniUart {
        MiniUart
    }

    /// Returns a pointer to the register block
    fn ptr() -> *const RegisterBlock {
        UART1_BASE as *const _
    }

    ///Set baud rate and characteristics (115200 8N1) and map to GPIO
    pub fn init(&self) {
        // initialize UART
        self.AUX_ENABLES.modify(AUX_ENABLES::MINI_UART_ENABLE::SET);
        self.AUX_MU_IER.set(0);
        self.AUX_MU_CNTL.set(0);
        self.AUX_MU_LCR.write(AUX_MU_LCR::DATA_SIZE::EightBit);
        self.AUX_MU_MCR.set(0);
        self.AUX_MU_IER.set(0);
        self.AUX_MU_IIR.write(AUX_MU_IIR::FIFO_CLEAR::All);
        self.AUX_MU_BAUD.write(AUX_MU_BAUD::RATE.val(270)); // 115200 baud

        // map UART1 to GPIO pins
        unsafe {
            (*gpio::GPFSEL1).modify(gpio::GPFSEL1::FSEL14::TXD1 + gpio::GPFSEL1::FSEL15::RXD1);

            (*gpio::GPPUD).set(0); // enable pins 14 and 15
            loop_delay(150);

            (*gpio::GPPUDCLK0).write(
                gpio::GPPUDCLK0::PUDCLK14::AssertClock + gpio::GPPUDCLK0::PUDCLK15::AssertClock,
            );
            loop_delay(150);

            (*gpio::GPPUDCLK0).set(0);
        }

        self.AUX_MU_CNTL
            .write(AUX_MU_CNTL::RX_EN::Enabled + AUX_MU_CNTL::TX_EN::Enabled);
    }

    /// Send a character
    pub fn send(&self, c: char) {
        // wait until we can send
        loop_until(|| self.AUX_MU_LSR.is_set(AUX_MU_LSR::TX_EMPTY));

        // write the character to the buffer
        self.AUX_MU_IO.set(c as u32);
    }

    /// Receive a character
    pub fn getc(&self) -> char {
        // wait until something is in the buffer
        loop_until(|| self.AUX_MU_LSR.is_set(AUX_MU_LSR::DATA_READY));

        // read it and return
        let ret = self.AUX_MU_IO.get() as u8 as char;

        // convert carriage return to newline
        if ret == '\r' {
            '\n'
        } else {
            ret
        }
    }

    /// Display a string
    pub fn puts(&self, string: &str) {
        for c in string.chars() {
            // convert newline to carriage return + newline
            if c == '\n' {
                self.send('\r')
            }

            self.send(c);
        }
    }
}

impl core::fmt::Write for MiniUart {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.puts(s);
        Ok(())
    }
}

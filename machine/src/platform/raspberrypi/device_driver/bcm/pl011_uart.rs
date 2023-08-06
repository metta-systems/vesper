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
    crate::{
        console::interface,
        cpu::loop_while,
        devices::serial::SerialOps,
        exception,
        platform::{
            device_driver::{common::MMIODerefWrapper, gpio, IRQNumber},
            mailbox::{self, Mailbox, MailboxOps},
            BcmHost,
        },
        synchronization::{interface::Mutex, IRQSafeNullLock},
    },
    core::fmt::{self, Arguments},
    snafu::Snafu,
    tock_registers::{
        interfaces::{ReadWriteable, Readable, Writeable},
        register_bitfields, register_structs,
        registers::{ReadOnly, ReadWrite, WriteOnly},
    },
};

//--------------------------------------------------------------------------------------------------
// Private Definitions
//--------------------------------------------------------------------------------------------------

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
        BAUD_DIVINT OFFSET(0) NUMBITS(16) []
    ],

    /// Fractional Baud rate divisor
    FBRD [
        /// Fractional Baud rate divisor
        BAUD_DIVFRAC OFFSET(0) NUMBITS(6) []
    ],

    /// Line Control register
    LCR_H [
        /// Word length. These bits indicate the number of data bits
        /// transmitted or received in a frame.
        WordLength OFFSET(5) NUMBITS(2) [
            FiveBit = 0b00,
            SixBit = 0b01,
            SevenBit = 0b10,
            EightBit = 0b11
        ],

        Fifos OFFSET(4) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ],

        /// Use 2 stop bits
        Stop2 OFFSET(3) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ],

        Parity OFFSET(1) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ],
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

    /// Interrupt FIFO Level Select Register.
    IFLS [
        /// Receive interrupt FIFO level select.
        /// The trigger points for the receive interrupt are as follows.
        RXIFLSEL OFFSET(3) NUMBITS(5) [
            OneEigth = 0b000,
            OneQuarter = 0b001,
            OneHalf = 0b010,
            ThreeQuarters = 0b011,
            SevenEights = 0b100
        ]
    ],

    /// Interrupt Mask Set/Clear Register.
    IMSC [
        /// Receive timeout interrupt mask. A read returns the current mask for the UARTRTINTR
        /// interrupt.
        ///
        /// - On a write of 1, the mask of the UARTRTINTR interrupt is set.
        /// - A write of 0 clears the mask.
        RTIM OFFSET(6) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ],

        /// Receive interrupt mask. A read returns the current mask for the UARTRXINTR interrupt.
        ///
        /// - On a write of 1, the mask of the UARTRXINTR interrupt is set.
        /// - A write of 0 clears the mask.
        RXIM OFFSET(4) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ]
    ],

    /// Masked Interrupt Status Register.
    MIS [
        /// Receive timeout masked interrupt status. Returns the masked interrupt state of the
        /// UARTRTINTR interrupt.
        RTMIS OFFSET(6) NUMBITS(1) [],

        /// Receive masked interrupt status. Returns the masked interrupt state of the UARTRXINTR
        /// interrupt.
        RXMIS OFFSET(4) NUMBITS(1) []
    ],

    /// Interrupt Clear Register
    ICR [
        /// Meta field for all pending interrupts
        /// On a write of 1, the corresponding interrupt is cleared. A write of 0 has no effect.
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
        (0x24 => IntegerBaudRate: WriteOnly<u32, IBRD::Register>),
        (0x28 => FractionalBaudRate: WriteOnly<u32, FBRD::Register>),
        (0x2c => LineControl: ReadWrite<u32, LCR_H::Register>), // @todo write-only?
        (0x30 => Control: WriteOnly<u32, CR::Register>),
        (0x34 => InterruptFifoLevelSelect: ReadWrite<u32, IFLS::Register>),
        (0x38 => InterruptMaskSetClear: ReadWrite<u32, IMSC::Register>),
        (0x3c => RawInterruptStatus: ReadOnly<u32>),
        (0x40 => MaskedInterruptStatus: ReadOnly<u32, MIS::Register>),
        (0x44 => InterruptClear: WriteOnly<u32, ICR::Register>),
        (0x48 => DmaControl: WriteOnly<u32, DMACR::Register>),
        (0x4c => __reserved_3),
        (0x1000 => @END),
    }
}

// #[derive(Debug, Snafu)]
// pub enum PL011UartError {
//     #[snafu(display("PL011 UART setup failed in mailbox operation"))]
//     MailboxError,
//     #[snafu(display(
//         "PL011 UART setup failed due to integer baud rate divisor out of range ({})",
//         ibrd
//     ))]
//     InvalidIntegerDivisor { ibrd: u32 },
//     #[snafu(display(
//         "PL011 UART setup failed due to fractional baud rate divisor out of range ({})",
//         fbrd
//     ))]
//     InvalidFractionalDivisor { fbrd: u32 },
// }
//
// pub type Result<T> = ::core::result::Result<T, PL011UartError>;

type Registers = MMIODerefWrapper<RegisterBlock>;

struct PL011UartInner {
    registers: Registers,
}

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

pub struct PL011Uart {
    inner: IRQSafeNullLock<PL011UartInner>,
}

pub struct RateDivisors {
    integer_baud_rate_divisor: u32,
    fractional_baud_rate_divisor: u32,
}

// [temporary] Used in mmu.rs to set up local paging
pub const UART0_BASE: usize = BcmHost::get_peripheral_address() + 0x20_1000;

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

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
    pub fn from_clock_and_rate(clock: u64, baud_rate: u32) -> Result<RateDivisors, &'static str> {
        let value = 4 * clock / baud_rate as u64;
        let i = ((value >> 6) & 0xffff) as u32;
        let f = (value & 0x3f) as u32;
        // TODO: check for integer overflow, i.e. any bits set above the 0x3fffff mask.
        // FIXME: can't happen due to calculation above
        if i > 65535 {
            return Err("PL011 UART setup failed due to integer baud rate divisor out of range");
            // return Err(PL011UartError::InvalidIntegerDivisor { ibrd: i });
        }
        // FIXME: can't happen due to calculation above
        if f > 63 {
            return Err("PL011 UART setup failed due to fractional baud rate divisor out of range");
            // return Err(PL011UartError::InvalidFractionalDivisor { fbrd: f });
        }
        Ok(RateDivisors {
            integer_baud_rate_divisor: i,
            fractional_baud_rate_divisor: f,
        })
    }
}

impl PL011Uart {
    pub const COMPATIBLE: &'static str = "BCM PL011 UART";

    /// Create an instance.
    ///
    /// # Safety
    ///
    /// - The user must ensure to provide a correct MMIO start address.
    pub const unsafe fn new(base_addr: usize) -> Self {
        Self {
            inner: IRQSafeNullLock::new(PL011UartInner::new(base_addr)),
        }
    }

    /// GPIO pins should be set up first before enabling the UART
    pub fn prepare_gpio(gpio: &gpio::GPIO) {
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
    }
}

//--------------------------------------------------------------------------------------------------
// Private Code
//--------------------------------------------------------------------------------------------------

impl PL011UartInner {
    /// Create an instance.
    ///
    /// # Safety
    ///
    /// - The user must ensure to provide a correct MMIO start address.
    pub const unsafe fn new(base_addr: usize) -> Self {
        Self {
            registers: Registers::new(base_addr),
        }
    }

    /// Set baud rate and characteristics (115200 8N1) and map to GPIO
    pub fn prepare(&self) -> core::result::Result<(), &'static str> {
        use tock_registers::interfaces::Writeable;

        // Turn off UART
        self.registers.Control.set(0);

        // Wait for any ongoing transmissions to complete
        self.flush_internal();

        // Flush TX FIFO
        self.registers.LineControl.modify(LCR_H::Fifos::Disabled);

        // Clear pending interrupts
        self.registers.InterruptClear.write(ICR::ALL::SET);

        // set up clock for consistent divisor values
        const CLOCK: u32 = 4_000_000; // 4Mhz
        const BAUD_RATE: u32 = 115_200;

        let mut mailbox = Mailbox::<9>::default();
        let index = mailbox.request();
        let index = mailbox.set_clock_rate(index, mailbox::clock::UART, CLOCK);
        let mailbox = mailbox.end(index);

        if mailbox.call(mailbox::channel::PropertyTagsArmToVc).is_err() {
            return Err("PL011 UART setup failed in mailbox operation");
            // return Err(PL011UartError::MailboxError); // Abort if UART clocks couldn't be set
        };

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
            .write(IBRD::BAUD_DIVINT.val(divisors.integer_baud_rate_divisor & 0xffff));
        self.registers
            .FractionalBaudRate
            .write(FBRD::BAUD_DIVFRAC.val(divisors.fractional_baud_rate_divisor & 0b11_1111));
        self.registers.LineControl.write(
            LCR_H::WordLength::EightBit
                + LCR_H::Fifos::Enabled
                + LCR_H::Parity::Disabled
                + LCR_H::Stop2::Disabled,
        );

        // Set RX FIFO fill level at 1/8.
        self.registers
            .InterruptFifoLevelSelect
            .write(IFLS::RXIFLSEL::OneEigth);

        // Enable RX IRQ + RX timeout IRQ.
        self.registers
            .InterruptMaskSetClear
            .write(IMSC::RXIM::Enabled + IMSC::RTIM::Enabled);

        // Disable DMA
        self.registers
            .DmaControl
            .write(DMACR::RXDMAE::Disabled + DMACR::TXDMAE::Disabled);

        // Turn on UART
        self.registers
            .Control
            .write(CR::UARTEN::Enabled + CR::TXE::Enabled + CR::RXE::Enabled);

        Ok(())
    }

    fn flush_internal(&self) {
        loop_while(|| self.registers.Flag.is_set(FR::BUSY));
    }
}

impl Drop for PL011UartInner {
    fn drop(&mut self) {
        self.registers.Control.set(0);
    }
}

impl SerialOps for PL011UartInner {
    fn read_byte(&self) -> u8 {
        // wait until something is in the buffer
        loop_while(|| self.registers.Flag.is_set(FR::RXFE));

        // read it and return
        self.registers.Data.get() as u8
    }

    fn write_byte(&self, b: u8) {
        // wait until we can send
        loop_while(|| self.registers.Flag.is_set(FR::TXFF));

        // write the character to the buffer
        self.registers.Data.set(b as u32);
    }

    /// Wait until the TX FIFO is empty, aka all characters have been put on the
    /// line.
    fn flush(&self) {
        self.flush_internal();
    }

    /// Consume input until RX FIFO is empty, aka all pending characters have been
    /// consumed.
    fn clear_rx(&self) {
        loop_while(|| {
            let pending = !self.registers.Flag.is_set(FR::RXFE);
            if pending {
                self.read_byte();
            }
            pending
        });
    }
}

impl interface::ConsoleOps for PL011UartInner {}

impl fmt::Write for PL011UartInner {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        use interface::ConsoleOps;
        self.write_string(s);
        Ok(())
    }
}

impl interface::Write for PL011Uart {
    fn write_fmt(&self, args: Arguments) -> fmt::Result {
        self.inner.lock(|inner| fmt::Write::write_fmt(inner, args))
    }
}

//--------------------------------------------------------------------------------------------------
// OS Interface Code
//--------------------------------------------------------------------------------------------------

impl crate::drivers::interface::DeviceDriver for PL011Uart {
    type IRQNumberType = IRQNumber;

    fn compatible(&self) -> &'static str {
        Self::COMPATIBLE
    }

    unsafe fn init(&self) -> core::result::Result<(), &'static str> {
        self.inner.lock(|inner| inner.prepare())
    }

    fn register_and_enable_irq_handler(
        &'static self,
        irq_number: &Self::IRQNumberType,
    ) -> Result<(), &'static str> {
        use exception::asynchronous::{irq_manager, IRQHandlerDescriptor};

        let descriptor = IRQHandlerDescriptor::new(*irq_number, Self::COMPATIBLE, self);

        irq_manager().register_handler(descriptor)?;
        irq_manager().enable(irq_number);

        Ok(())
    }
}

impl SerialOps for PL011Uart {
    fn read_byte(&self) -> u8 {
        self.inner.lock(|inner| inner.read_byte())
    }

    fn write_byte(&self, byte: u8) {
        self.inner.lock(|inner| inner.write_byte(byte))
    }

    fn flush(&self) {
        self.inner.lock(|inner| inner.flush())
    }

    fn clear_rx(&self) {
        self.inner.lock(|inner| inner.clear_rx())
    }
}

impl interface::ConsoleOps for PL011Uart {
    fn write_char(&self, c: char) {
        self.inner.lock(|inner| inner.write_char(c))
    }

    fn write_string(&self, string: &str) {
        self.inner.lock(|inner| inner.write_string(string))
    }

    fn read_char(&self) -> char {
        self.inner.lock(|inner| inner.read_char())
    }
}

impl interface::All for PL011Uart {}

impl exception::asynchronous::interface::IRQHandler for PL011Uart {
    fn handle(&self) -> Result<(), &'static str> {
        use interface::ConsoleOps;

        self.inner.lock(|inner| {
            let pending = inner.registers.MaskedInterruptStatus.extract();

            // Clear all pending IRQs.
            inner.registers.InterruptClear.write(ICR::ALL::SET);

            // Check for any kind of RX interrupt.
            if pending.matches_any(MIS::RXMIS::SET + MIS::RTMIS::SET) {
                // Echo any received characters.
                // while let Some(c) = inner.read_char() {
                //     inner.write_char(c)
                // }
            }
        });

        Ok(())
    }
}

//--------------------------------------------------------------------------------------------------
// Testing
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test_case]
    fn test_divisors() {
        const CLOCK: u64 = 3_000_000;
        const BAUD_RATE: u32 = 115_200;

        let divisors = RateDivisors::from_clock_and_rate(CLOCK, BAUD_RATE);
        assert!(divisors.is_ok());
        let divisors = divisors.unwrap();
        assert_eq!(divisors.integer_baud_rate_divisor, 1);
        assert_eq!(divisors.fractional_baud_rate_divisor, 40);
    }
}

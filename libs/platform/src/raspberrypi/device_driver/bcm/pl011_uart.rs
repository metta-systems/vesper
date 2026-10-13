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
    crate::device_driver::{IRQNumber, gpio},
    arbitrary_int::u6,
    bitbybit::{bitenum, bitfield},
    core::fmt::{self, Arguments},
    derive_mmio::Mmio,
    libaddress::{Address, Virtual},
    libconsole::{SerialOps, console::interface},
    liblocking::{IRQSafeNullLock, interface::Mutex},
    libmmio::assert_register_offsets,
    libprimitives::cpu::loop_while,
    // snafu::Snafu,
};

//--------------------------------------------------------------------------------------------------
// Private Definitions
//--------------------------------------------------------------------------------------------------

// PL011 UART registers.
//
// Descriptions taken from
// https://github.com/raspberrypi/documentation/files/1888662/BCM2837-ARM-Peripherals.-.Revised.-.V2-1.pdf

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
    /// UART busy. If this bit is set to 1, the UART is busy
    /// transmitting data. This bit remains set until the complete
    /// byte, including all the stop bits, has been sent from the
    /// shift register. This bit is set as soon as the transmit FIFO
    /// becomes non-empty, regardless of whether the UART is enabled or not.
    #[bit(3, r)]
    busy: bool,

    /// Receive FIFO empty. The meaning of this bit depends on the
    /// state of the FEN bit in the LCRH Register. If the
    /// FIFO is disabled, this bit is set when the receive holding
    /// register is empty. If the FIFO is enabled, the RXFE bit is
    /// set when the receive FIFO is empty.
    #[bit(4, r)]
    receive_fifo_empty: bool,

    /// Transmit FIFO full. The meaning of this bit depends on the
    /// state of the FEN bit in the LCRH Register. If the
    /// FIFO is disabled, this bit is set when the transmit
    /// holding register is full. If the FIFO is enabled, the TXFF
    /// bit is set when the transmit FIFO is full.
    #[bit(5, r)]
    transmit_fifo_full: bool,

    /// Receive FIFO full. The meaning of this bit depends on the
    /// state of the FEN bit in the LCRH Register. If the FIFO is
    /// disabled, this bit is set when the receive holding register
    /// is full. If the FIFO is enabled, the RXFF bit is set when
    /// the receive FIFO is full.
    #[bit(6, r)]
    receive_fifo_full: bool,

    /// Transmit FIFO empty. The meaning of this bit depends on the
    /// state of the FEN bit in the Line Control Register, If the
    /// FIFO is disabled, this bit is set when the transmit holding
    /// register is empty. If the FIFO is enabled, the TXFE bit is
    /// set when the transmit FIFO is empty. This bit does not indicate
    /// if there is data in the transmit shift register.
    #[bit(7, r)]
    transmit_fifo_empty: bool,
}

/// Integer Baud rate divisor
#[bitfield(u32, default = 0, debug)]
struct IntegerBaudRate {
    /// Integer Baud rate divisor
    #[bits(0..=15, rw)]
    divisor: u16,
}

/// Fractional Baud rate divisor
#[bitfield(u32, default = 0, debug)]
struct FractionalBaudRate {
    /// Fractional Baud rate divisor
    #[bits(0..=5, rw)]
    divisor: u6,
}

/// Word length: the number of data bits transmitted or received in a frame.
#[bitenum(u2, exhaustive = true)]
#[derive(Debug)]
enum WordLength {
    Five = 0b00,
    Six = 0b01,
    Seven = 0b10,
    Eight = 0b11,
}

/// Line Control register
#[bitfield(u32, default = 0, debug)]
struct LineControl {
    #[bit(1, rw)]
    parity: bool,

    /// Use 2 stop bits
    #[bit(3, rw)]
    two_stop_bits: bool,

    #[bit(4, rw)]
    fifos: bool,

    #[bits(5..=6, rw)]
    word_length: WordLength,
}

/// Control Register
#[bitfield(u32, default = 0, debug)]
struct Control {
    /// UART enable. If the UART is disabled in the middle of transmission
    /// or reception, it completes the current character before stopping.
    #[bit(0, rw)]
    uart_enable: bool,

    /// Transmit enable. If this bit is set to 1, the transmit
    /// section of the UART is enabled. Data transmission occurs
    /// for UART signals. When the UART is disabled in the middle
    /// of transmission, it completes the current character before
    /// stopping.
    #[bit(8, rw)]
    transmit_enable: bool,

    /// Receive enable. If this bit is set to 1, the receive
    /// section of the UART is enabled. Data reception occurs for
    /// UART signals. When the UART is disabled in the middle of
    /// reception, it completes the current character before
    /// stopping.
    #[bit(9, rw)]
    receive_enable: bool,
}

/// The trigger points for the receive interrupt.
#[bitenum(u3, exhaustive = false)]
#[derive(Debug)]
enum FifoLevel {
    OneEighth = 0b000,
    OneQuarter = 0b001,
    OneHalf = 0b010,
    ThreeQuarters = 0b011,
    SevenEighths = 0b100,
}

/// Interrupt FIFO Level Select Register.
#[bitfield(u32, default = 0, debug)]
struct InterruptFifoLevelSelect {
    /// Receive interrupt FIFO level select.
    #[bits(3..=5, rw)]
    receive: Option<FifoLevel>,
}

/// Interrupt sources, as the mask set/clear, masked status and clear
/// registers lay them out.
#[bitfield(u32, default = 0, debug)]
struct InterruptSources {
    /// Receive interrupt (UARTRXINTR).
    ///
    /// In the mask register a write of 1 sets the mask of the interrupt and a
    /// write of 0 clears it; a read returns the current mask.
    #[bit(4, rw)]
    receive: bool,

    /// Receive timeout interrupt (UARTRTINTR).
    #[bit(6, rw)]
    receive_timeout: bool,
}

impl InterruptSources {
    /// Every interrupt: on a write of 1 to the clear register, the
    /// corresponding interrupt is cleared. A write of 0 has no effect.
    const ALL: Self = Self::new_with_raw_value(0x7ff);
}

/// DMA Control Register
#[bitfield(u32, default = 0, debug)]
struct DmaControl {
    /// RX DMA enabled
    #[bit(0, rw)]
    receive_dma_enable: bool,

    /// TX DMA enabled
    #[bit(1, rw)]
    transmit_dma_enable: bool,
}

// https://developer.arm.com/documentation/ddi0183/g/programmers-model/summary-of-registers?lang=en
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
struct Pl011RegisterBlock {
    #[mmio(Read, Write)]
    data: Data,
    /// RSR/ECR
    #[mmio(Read, Write)]
    status: u32,
    _reserved1: [u32; 4],
    #[mmio(PureRead)]
    flag: Flags,
    _reserved2: [u32; 2],
    #[mmio(Write)]
    integer_baud_rate: IntegerBaudRate,
    #[mmio(Write)]
    fractional_baud_rate: FractionalBaudRate,
    line_control: LineControl, // @todo write-only?
    #[mmio(Write)]
    control: Control,
    interrupt_fifo_level_select: InterruptFifoLevelSelect,
    interrupt_mask_set_clear: InterruptSources,
    #[mmio(PureRead)]
    raw_interrupt_status: u32,
    #[mmio(PureRead)]
    masked_interrupt_status: InterruptSources,
    #[mmio(Write)]
    interrupt_clear: InterruptSources,
    #[mmio(Write)]
    dma_control: DmaControl,
    _reserved3: [u32; 1005],
}

assert_register_offsets!(Pl011RegisterBlock, 0x1000, {
    data: 0x00,
    status: 0x04,
    flag: 0x18,
    integer_baud_rate: 0x24,
    fractional_baud_rate: 0x28,
    line_control: 0x2c,
    control: 0x30,
    interrupt_fifo_level_select: 0x34,
    interrupt_mask_set_clear: 0x38,
    raw_interrupt_status: 0x3c,
    masked_interrupt_status: 0x40,
    interrupt_clear: 0x44,
    dma_control: 0x48,
});
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

struct PL011UartInner {
    registers: MmioPl011RegisterBlock<'static>,
}

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

pub struct PL011Uart {
    inner: IRQSafeNullLock<PL011UartInner>,
}

pub struct RateDivisors {
    pub integer_baud_rate_divisor: u32,
    pub fractional_baud_rate_divisor: u32,
}

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
        let value = 4 * clock / u64::from(baud_rate);
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
    pub const unsafe fn new(mmio_base_addr: Address<Virtual>) -> Self {
        Self {
            inner: IRQSafeNullLock::new(
                // SAFETY: Not safe.
                unsafe { PL011UartInner::new(mmio_base_addr) },
            ),
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
    pub const unsafe fn new(mmio_base_addr: Address<Virtual>) -> Self {
        Self {
            // SAFETY: the caller's contract.
            registers: unsafe { Pl011RegisterBlock::new_mmio(mmio_base_addr.as_mut_ptr()) },
        }
    }

    /// A handle to the registers for one access.
    fn registers(&self) -> MmioPl011RegisterBlock<'static> {
        // SAFETY: `PL011UartInner` is only reached through the
        // `IRQSafeNullLock` of its `PL011Uart`, which serializes every access,
        // so no two handles are used at the same time.
        unsafe { self.registers.clone() }
    }

    /// Set baud rate and characteristics (115200 8N1) and map to GPIO
    pub fn prepare(&mut self) -> core::result::Result<(), &'static str> {
        // set up clock for consistent divisor values
        const CLOCK: u32 = 4_000_000; // 4Mhz
        const BAUD_RATE: u32 = 115_200;

        // Turn off UART
        self.registers.write_control(Control::ZERO);

        // Wait for any ongoing transmissions to complete
        self.flush_internal();

        // Flush TX FIFO
        self.registers
            .modify_line_control(|line| line.with_fifos(false));

        // Clear pending interrupts
        self.registers.write_interrupt_clear(InterruptSources::ALL);

        // // Should have a MailboxCommand with ref to a command buffer, and access to global MAILBOX
        // // driver to run those commands atomically..
        // let mut mailbox = Mailbox::<9>::default();
        // let index = mailbox.request();
        // let index = mailbox.set_clock_rate(index, mailbox::clock::UART, CLOCK);
        // let mailbox = mailbox.end(index);
        //
        // if mailbox.call(mailbox::channel::PropertyTagsArmToVc).is_err() {
        //     return Err("PL011 UART setup failed in mailbox operation");
        //     // return Err(PL011UartError::MailboxError); // Abort if UART clocks couldn't be set
        // };

        // From the PL011 Technical Reference Manual:
        //
        // The LCR_H, IBRD, and FBRD registers form the single 30-bit wide LCR Register that is
        // updated on a single write strobe generated by a LCR_H write. So, to internally update the
        // contents of IBRD or FBRD, a LCR_H write must always be performed at the end.
        //
        // Set the baud rate divisors, 8N1 and FIFO enabled.
        let divisors = RateDivisors::from_clock_and_rate(CLOCK.into(), BAUD_RATE)?;
        // `from_clock_and_rate` already rejects divisors outside the fields.
        let integer_divisor = u16::try_from(divisors.integer_baud_rate_divisor)
            .ok()
            .ok_or("PL011 UART integer baud rate divisor out of range")?;
        let fractional_divisor = u8::try_from(divisors.fractional_baud_rate_divisor)
            .ok()
            .and_then(|divisor| u6::try_new(divisor).ok())
            .ok_or("PL011 UART fractional baud rate divisor out of range")?;
        self.registers
            .write_integer_baud_rate(IntegerBaudRate::ZERO.with_divisor(integer_divisor));
        self.registers
            .write_fractional_baud_rate(FractionalBaudRate::ZERO.with_divisor(fractional_divisor));
        self.registers.write_line_control(
            LineControl::ZERO
                .with_word_length(WordLength::Eight)
                .with_fifos(true)
                .with_parity(false)
                .with_two_stop_bits(false),
        );

        // Set RX FIFO fill level at 1/8.
        self.registers.write_interrupt_fifo_level_select(
            InterruptFifoLevelSelect::ZERO.with_receive(FifoLevel::OneEighth),
        );

        // Enable RX IRQ + RX timeout IRQ.
        self.registers.write_interrupt_mask_set_clear(
            InterruptSources::ZERO
                .with_receive(true)
                .with_receive_timeout(true),
        );

        // Disable DMA
        self.registers.write_dma_control(
            DmaControl::ZERO
                .with_receive_dma_enable(false)
                .with_transmit_dma_enable(false),
        );

        // Turn on UART
        self.registers.write_control(
            Control::ZERO
                .with_uart_enable(true)
                .with_transmit_enable(true)
                .with_receive_enable(true),
        );

        Ok(())
    }

    fn flush_internal(&self) {
        loop_while(|| self.registers.read_flag().busy());
    }
}

impl Drop for PL011UartInner {
    fn drop(&mut self) {
        self.registers.write_control(Control::ZERO);
    }
}

impl SerialOps for PL011UartInner {
    fn read_byte(&self) -> u8 {
        // wait until something is in the buffer
        loop_while(|| self.registers.read_flag().receive_fifo_empty());

        // read it and return
        self.registers().read_data().byte()
    }

    fn read_byte_nonblocking(&self) -> Option<u8> {
        if self.registers.read_flag().receive_fifo_empty() {
            None
        } else {
            Some(self.registers().read_data().byte())
        }
    }

    fn write_byte(&self, b: u8) {
        // wait until we can send
        loop_while(|| self.registers.read_flag().transmit_fifo_full());

        // write the character to the buffer
        self.registers().write_data(Data::ZERO.with_byte(b));
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
            let pending = !self.registers.read_flag().receive_fifo_empty();
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

impl libdriver::drivers::interface::DeviceDriver for PL011Uart {
    type IRQNumberType = IRQNumber;

    fn compatible(&self) -> &'static str {
        Self::COMPATIBLE
    }

    unsafe fn init(&self) -> core::result::Result<(), &'static str> {
        self.inner.lock(PL011UartInner::prepare)
    }

    fn register_and_enable_irq_handler(
        &'static self,
        irq_number: Self::IRQNumberType,
    ) -> Result<(), &'static str> {
        use {
            crate::exception::asynchronous::irq_manager,
            libexception::asynchronous::IRQHandlerDescriptor,
        };

        let descriptor = IRQHandlerDescriptor::new(irq_number, Self::COMPATIBLE, self);

        irq_manager().register_handler(descriptor)?;
        irq_manager().enable(irq_number);

        Ok(())
    }
}

impl SerialOps for PL011Uart {
    fn read_byte(&self) -> u8 {
        self.inner.lock(|inner| inner.read_byte())
    }

    fn read_byte_nonblocking(&self) -> Option<u8> {
        self.inner.lock(|inner| inner.read_byte_nonblocking())
    }

    fn write_byte(&self, byte: u8) {
        self.inner.lock(|inner| inner.write_byte(byte));
    }

    fn flush(&self) {
        self.inner.lock(|inner| inner.flush());
    }

    fn clear_rx(&self) {
        self.inner.lock(|inner| inner.clear_rx());
    }
}

impl interface::ConsoleOps for PL011Uart {
    fn write_char(&self, c: char) {
        self.inner.lock(|inner| inner.write_char(c));
    }

    fn write_string(&self, string: &str) {
        self.inner.lock(|inner| inner.write_string(string));
    }

    fn read_char(&self) -> char {
        self.inner.lock(|inner| inner.read_char())
    }
}

impl interface::All for PL011Uart {}

impl libexception::asynchronous::interface::IRQHandler for PL011Uart {
    fn handle(&self) -> Result<(), &'static str> {
        self.inner.lock(|inner| {
            let pending = inner.registers.read_masked_interrupt_status();

            // Clear all pending IRQs.
            inner.registers.write_interrupt_clear(InterruptSources::ALL);

            // Check for any kind of RX interrupt.
            if pending.receive() || pending.receive_timeout() {
                // use interface::ConsoleOps;
                // Echo any received characters.
                // while let Some(c) = inner.read_char() {
                //     inner.write_char(c)
                // }
            }
        });

        Ok(())
    }
}

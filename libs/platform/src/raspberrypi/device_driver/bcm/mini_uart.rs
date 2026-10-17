/*
 * SPDX-License-Identifier: MIT OR BlueOak-1.0.0
 * Copyright (c) 2018-2019 Andre Richter <andre.o.richter@gmail.com>
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 * Original code distributed under MIT, additional changes are under BlueOak-1.0.0
 */

use {
    crate::{BcmHost, device_driver::gpio, exception::asynchronous::IRQNumber},
    bitbybit::{bitenum, bitfield},
    core::{
        convert::From,
        fmt::{self, Arguments},
    },
    derive_mmio::Mmio,
    libaddress::{Address, Virtual},
    libconsole::{SerialOps, console::interface},
    liblocking::{IRQSafeNullLock, interface::Mutex},
    libmmio::assert_register_offsets,
};

// Auxiliary mini UART registers
//
// Descriptions taken from
// https://github.com/raspberrypi/documentation/files/1888662/BCM2837-ARM-Peripherals.-.Revised.-.V2-1.pdf

/// Auxiliary enables
#[bitfield(u32, default = 0, debug)]
struct AuxEnables {
    /// If set the mini UART is enabled. The UART will immediately
    /// start receiving data, especially if the `UART1_RX` line is
    /// low.
    /// If clear the mini UART is disabled. That also disables any
    /// mini UART register access
    #[bit(0, rw)]
    mini_uart_enable: bool,
}

/// Mini Uart Interrupt Identify
#[bitfield(u32, default = 0, debug)]
struct InterruptIdentify {
    /// Writing with bit 1 set will clear the receive FIFO
    #[bit(1, w)]
    clear_receive_fifo: bool,
    /// Writing with bit 2 set will clear the transmit FIFO
    #[bit(2, w)]
    clear_transmit_fifo: bool,
}

/// The mode the UART works in.
#[bitenum(u2, exhaustive = false)]
#[derive(Debug)]
enum DataSize {
    SevenBit = 0b00,
    EightBit = 0b11,
}

/// Mini Uart Line Control
#[bitfield(u32, default = 0, debug)]
struct LineControl {
    #[bits(0..=1, rw)]
    data_size: Option<DataSize>,
}

/// Mini Uart Extra Control
#[bitfield(u32, default = 0, debug)]
struct ExtraControl {
    /// If this bit is set the mini UART receiver is enabled.
    /// If this bit is clear the mini UART receiver is disabled.
    #[bit(0, rw)]
    receive_enable: bool,
    /// If this bit is set the mini UART transmitter is enabled.
    /// If this bit is clear the mini UART transmitter is disabled.
    #[bit(1, rw)]
    transmit_enable: bool,
}

/// Mini Uart Status
#[bitfield(u32, default = 0, debug)]
struct Status {
    /// This bit is set if the receive FIFO holds at least 1
    /// symbol.
    #[bit(0, r)]
    symbol_available: bool,
    /// This bit is set if the transmit FIFO can accept at least
    /// one byte.
    #[bit(1, r)]
    space_available: bool,
    #[bit(9, r)]
    transmit_done: bool,
}

/// Mini Uart Baud rate
#[bitfield(u32, default = 0, debug)]
struct BaudRate {
    /// Mini UART baud rate counter
    #[bits(0..=15, rw)]
    rate: u16,
}

#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
struct MiniUartRegisterBlock {
    // 0x00 - AUX_IRQ?
    _reserved1: u32,
    aux_enables: AuxEnables,
    _reserved2: [u32; 14],
    /// Mini Uart I/O Data
    #[mmio(Read, Write)]
    io: u32,
    /// Mini Uart Interrupt Enable
    #[mmio(Write)]
    interrupt_enable: u32,
    #[mmio(Write)]
    interrupt_identify: InterruptIdentify,
    #[mmio(Write)]
    line_control: LineControl,
    /// Mini Uart Modem Control
    #[mmio(Write)]
    modem_control: u32,
    /// Mini Uart Line Status
    #[mmio(PureRead)]
    line_status: u32,
    // 0x58 - AUX_MU_MSR
    // 0x5c - AUX_MU_SCRATCH
    _reserved3: [u32; 2],
    #[mmio(Write)]
    extra_control: ExtraControl,
    #[mmio(PureRead)]
    status: Status,
    #[mmio(Write)]
    baud_rate: BaudRate,
}

assert_register_offsets!(MiniUartRegisterBlock, 0x6c, {
    aux_enables: 0x04,
    io: 0x40,
    interrupt_enable: 0x44,
    interrupt_identify: 0x48,
    line_control: 0x4c,
    modem_control: 0x50,
    line_status: 0x54,
    extra_control: 0x60,
    status: 0x64,
    baud_rate: 0x68,
});

struct MiniUartInner {
    registers: MmioMiniUartRegisterBlock<'static>,
}

pub struct MiniUart {
    inner: IRQSafeNullLock<MiniUartInner>,
}

/// Divisor values for common baud rates
pub enum Rate {
    Baud115200 = 270,
}

impl From<Rate> for u16 {
    fn from(r: Rate) -> Self {
        r as u16
    }
}

// [temporary] Used in mmu.rs to set up local paging
pub const UART1_BASE: usize = BcmHost::get_peripheral_address() + 0x21_5000; // DTB says 0x7e215040

impl libdriver::drivers::interface::DeviceDriver for MiniUart {
    type IRQNumberType = IRQNumber;

    fn compatible(&self) -> &'static str {
        Self::COMPATIBLE
    }

    unsafe fn init(&self) -> Result<(), &'static str> {
        self.inner.lock(MiniUartInner::prepare)
    }
}

impl MiniUart {
    pub const COMPATIBLE: &'static str = "BCM MINI UART";

    /// Create an instance.
    ///
    /// # Safety
    ///
    /// - The user must ensure to provide a correct MMIO start address.
    pub const unsafe fn new(mmio_base_addr: Address<Virtual>) -> Self {
        Self {
            inner: IRQSafeNullLock::new(
                // SAFETY: Not safe.
                unsafe { MiniUartInner::new(mmio_base_addr) },
            ),
        }
    }

    /// GPIO pins should be set up first before enabling the UART
    pub fn prepare_gpio(gpio: &gpio::GPIO) {
        // Pin 14
        const MINI_UART_TXD: gpio::Function = gpio::Function::Alt5;
        // Pin 15
        const MINI_UART_RXD: gpio::Function = gpio::Function::Alt5;

        // map UART1 to GPIO pins
        gpio.get_pin(14)
            .into_alt(MINI_UART_TXD)
            .set_pull_up_down(gpio::PullUpDown::Up);
        gpio.get_pin(15)
            .into_alt(MINI_UART_RXD)
            .set_pull_up_down(gpio::PullUpDown::Up);
    }
}

impl MiniUartInner {
    /// Create an instance.
    ///
    /// # Safety
    ///
    /// - The user must ensure to provide a correct MMIO start address.
    pub const unsafe fn new(mmio_base_addr: Address<Virtual>) -> Self {
        Self {
            // SAFETY: the caller's contract.
            registers: unsafe { MiniUartRegisterBlock::new_mmio(mmio_base_addr.as_mut_ptr()) },
        }
    }

    /// A handle to the registers for one access.
    fn registers(&self) -> MmioMiniUartRegisterBlock<'static> {
        // SAFETY: `MiniUartInner` is only reached through the
        // `IRQSafeNullLock` of its `MiniUart`, which serializes every access,
        // so no two handles are used at the same time.
        unsafe { self.registers.clone() }
    }

    /// Set baud rate and characteristics (115200 8N1) and map to GPIO
    #[allow(clippy::unnecessary_wraps)]
    pub fn prepare(&mut self) -> Result<(), &'static str> {
        // initialize UART
        self.registers
            .modify_aux_enables(|enables| enables.with_mini_uart_enable(true));
        self.registers.write_interrupt_enable(0);
        self.registers.write_extra_control(ExtraControl::ZERO);
        self.registers
            .write_line_control(LineControl::ZERO.with_data_size(DataSize::EightBit));
        self.registers.write_modem_control(0);
        self.registers.write_interrupt_enable(0);
        self.registers
            .write_baud_rate(BaudRate::ZERO.with_rate(Rate::Baud115200.into()));

        // Clear FIFOs before using the device
        self.registers.write_interrupt_identify(
            InterruptIdentify::ZERO
                .with_clear_receive_fifo(true)
                .with_clear_transmit_fifo(true),
        );

        self.registers.write_extra_control(
            ExtraControl::ZERO
                .with_receive_enable(true)
                .with_transmit_enable(true),
        );

        Ok(())
    }

    fn flush_internal(&self) {
        libprimitives::cpu::loop_until(|| self.registers.read_status().transmit_done());
    }
}

impl Drop for MiniUartInner {
    fn drop(&mut self) {
        self.registers
            .modify_aux_enables(|enables| enables.with_mini_uart_enable(false));
        // @todo disable gpio.PUD ?
    }
}

impl SerialOps for MiniUartInner {
    /// Receive a byte without console translation
    fn read_byte(&self) -> u8 {
        // wait until something is in the buffer
        libprimitives::cpu::loop_until(|| self.registers.read_status().symbol_available());

        // read it and return
        (self.registers().read_io() & 0xff) as u8
    }

    fn read_byte_nonblocking(&self) -> Option<u8> {
        self.registers
            .read_status()
            .symbol_available()
            .then(|| (self.registers().read_io() & 0xff) as u8)
    }

    fn write_byte(&self, b: u8) {
        // wait until we can send
        libprimitives::cpu::loop_until(|| self.registers.read_status().space_available());

        // write the character to the buffer
        self.registers().write_io(u32::from(b));
    }

    /// Wait until the TX FIFO is empty, aka all characters have been put on the
    /// line.
    fn flush(&self) {
        self.flush_internal();
    }

    /// Consume input until RX FIFO is empty, aka all pending characters have been
    /// consumed.
    fn clear_rx(&self) {
        libprimitives::cpu::loop_while(|| {
            // libprimitives?
            let pending = self.registers.read_status().symbol_available();
            if pending {
                self.read_byte();
            }
            pending
        });
    }
}

impl interface::ConsoleOps for MiniUartInner {}

impl fmt::Write for MiniUartInner {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        use interface::ConsoleOps;
        self.write_string(s);
        Ok(())
    }
}

impl interface::Write for MiniUart {
    fn write_fmt(&self, args: Arguments) -> fmt::Result {
        self.inner.lock(|inner| fmt::Write::write_fmt(inner, args))
    }
}

impl SerialOps for MiniUart {
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

impl interface::ConsoleOps for MiniUart {
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

impl interface::All for MiniUart {}

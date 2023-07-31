/*
 * SPDX-License-Identifier: MIT OR BlueOak-1.0.0
 * Copyright (c) 2018-2019 Andre Richter <andre.o.richter@gmail.com>
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 * Original code distributed under MIT, additional changes are under BlueOak-1.0.0
 */

#[cfg(not(feature = "noserial"))]
use tock_registers::interfaces::{Readable, Writeable};
use {
    crate::{
        console::interface,
        devices::SerialOps,
        mmio_deref_wrapper::MMIODerefWrapper,
        platform::{device_driver::gpio, BcmHost},
        sync::{interface::Mutex, NullLock},
    },
    cfg_if::cfg_if,
    core::{
        convert::From,
        fmt::{self, Arguments},
    },
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

struct MiniUartInner {
    registers: Registers,
}

pub struct MiniUart {
    inner: NullLock<MiniUartInner>,
}

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
pub const UART1_BASE: usize = BcmHost::get_peripheral_address() + 0x21_5000;

impl crate::drivers::interface::DeviceDriver for MiniUart {
    fn compatible(&self) -> &'static str {
        Self::COMPATIBLE
    }

    unsafe fn init(&self) -> Result<(), &'static str> {
        self.inner.lock(|inner| inner.prepare())
    }
}

impl MiniUart {
    pub const COMPATIBLE: &'static str = "BCM MINI UART";

    /// Create an instance.
    ///
    /// # Safety
    ///
    /// - The user must ensure to provide a correct MMIO start address.
    pub const unsafe fn new(base_addr: usize) -> Self {
        Self {
            inner: NullLock::new(MiniUartInner::new(base_addr)),
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
    pub const unsafe fn new(base_addr: usize) -> Self {
        Self {
            registers: Registers::new(base_addr),
        }
    }

    /// Set baud rate and characteristics (115200 8N1) and map to GPIO
    pub fn prepare(&self) -> Result<(), &'static str> {
        use tock_registers::interfaces::Writeable;
        // initialize UART
        self.registers
            .AUX_ENABLES
            .modify(AUX_ENABLES::MINI_UART_ENABLE::SET);
        self.registers.AUX_MU_IER.set(0);
        self.registers.AUX_MU_CNTL.set(0);
        self.registers
            .AUX_MU_LCR
            .write(AUX_MU_LCR::DATA_SIZE::EightBit);
        self.registers.AUX_MU_MCR.set(0);
        self.registers.AUX_MU_IER.set(0);
        self.registers
            .AUX_MU_BAUD
            .write(AUX_MU_BAUD::RATE.val(Rate::Baud115200.into()));

        // Clear FIFOs before using the device
        self.registers.AUX_MU_IIR.write(AUX_MU_IIR::FIFO_CLEAR::All);

        self.registers
            .AUX_MU_CNTL
            .write(AUX_MU_CNTL::RX_EN::Enabled + AUX_MU_CNTL::TX_EN::Enabled);

        Ok(())
    }

    fn flush_internal(&self) {
        use tock_registers::interfaces::Readable;
        crate::arch::loop_until(|| self.registers.AUX_MU_STAT.is_set(AUX_MU_STAT::TX_DONE));
    }
}

impl Drop for MiniUartInner {
    fn drop(&mut self) {
        self.registers
            .AUX_ENABLES
            .modify(AUX_ENABLES::MINI_UART_ENABLE::CLEAR);
        // @todo disable gpio.PUD ?
    }
}

impl SerialOps for MiniUartInner {
    /// Receive a byte without console translation
    fn read_byte(&self) -> u8 {
        use tock_registers::interfaces::Readable;
        // wait until something is in the buffer
        crate::arch::loop_until(|| {
            self.registers
                .AUX_MU_STAT
                .is_set(AUX_MU_STAT::SYMBOL_AVAILABLE)
        });

        // read it and return
        self.registers.AUX_MU_IO.get() as u8
    }

    fn write_byte(&self, b: u8) {
        use tock_registers::interfaces::{Readable, Writeable};
        // wait until we can send
        crate::arch::loop_until(|| {
            self.registers
                .AUX_MU_STAT
                .is_set(AUX_MU_STAT::SPACE_AVAILABLE)
        });

        // write the character to the buffer
        self.registers.AUX_MU_IO.set(b as u32);
    }

    /// Wait until the TX FIFO is empty, aka all characters have been put on the
    /// line.
    fn flush(&self) {
        self.flush_internal();
    }

    /// Consume input until RX FIFO is empty, aka all pending characters have been
    /// consumed.
    fn clear_rx(&self) {
        use tock_registers::interfaces::Readable;
        crate::arch::loop_while(|| {
            let pending = self
                .registers
                .AUX_MU_STAT
                .is_set(AUX_MU_STAT::SYMBOL_AVAILABLE);
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

impl interface::ConsoleOps for MiniUart {
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

impl interface::All for MiniUart {}

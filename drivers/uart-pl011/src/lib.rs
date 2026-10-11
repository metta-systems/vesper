#![no_std]

//! Register logic of the Arm PL011 UART, serving a serial channel
//! (`libserial-channel`).
//!
//! The UART runs with its FIFOs on and three interrupt sources: receive
//! (the receive FIFO reached its trigger level), receive timeout (bytes wait
//! in a FIFO below the level) and, while the channel has bytes to send,
//! transmit (the transmit FIFO drained to its level). [`Pl011::service`]
//! moves bytes between the FIFOs and the channel's rings once per interrupt
//! or doorbell; the line is level-triggered, so the transmit interrupt is
//! masked whenever nothing waits to be sent.
//!
//! The baud rate and the UART clock stay as the firmware set them: the
//! driver rewrites only the line control, which latches the divisors
//! already in place.
//!
//! Resources: the PL011 Technical Reference Manual (Arm DDI 0183), the
//! BCM2835/BCM2711 ARM Peripherals manuals, QEMU `hw/char/pl011.c`.

use {
    core::sync::atomic::Ordering,
    libserial_channel::SerialChannel,
    tock_registers::{
        interfaces::{ReadWriteable, Readable, Writeable},
        register_bitfields, register_structs,
        registers::{ReadOnly, ReadWrite, WriteOnly},
    },
};

register_bitfields! {
    u32,

    /// Data Register
    DR [
        /// Received or transmitted byte
        DATA OFFSET(0) NUMBITS(8) [],
        /// Framing error
        FE OFFSET(8) NUMBITS(1) [],
        /// Parity error
        PE OFFSET(9) NUMBITS(1) [],
        /// Break error
        BE OFFSET(10) NUMBITS(1) [],
        /// Overrun error: the receive FIFO was full when a byte arrived
        OE OFFSET(11) NUMBITS(1) []
    ],

    /// Flag Register
    FR [
        /// Transmit FIFO empty. With the FIFOs on, set when the transmit
        /// FIFO is empty; the shift register may still be sending.
        TXFE OFFSET(7) NUMBITS(1) [],
        /// Receive FIFO full
        RXFF OFFSET(6) NUMBITS(1) [],
        /// Transmit FIFO full
        TXFF OFFSET(5) NUMBITS(1) [],
        /// Receive FIFO empty
        RXFE OFFSET(4) NUMBITS(1) [],
        /// UART busy: set while a byte, including its stop bits, is being
        /// sent from the shift register, and as soon as the transmit FIFO
        /// is not empty
        BUSY OFFSET(3) NUMBITS(1) []
    ],

    /// Line Control Register
    LCR_H [
        /// Data bits per frame
        WLEN OFFSET(5) NUMBITS(2) [
            FiveBit = 0b00,
            SixBit = 0b01,
            SevenBit = 0b10,
            EightBit = 0b11
        ],
        /// FIFOs on; off makes them one-byte holding registers
        FEN OFFSET(4) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ],
        /// Two stop bits
        STP2 OFFSET(3) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ],
        /// Parity checking and generation
        PEN OFFSET(1) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ]
    ],

    /// Control Register
    CR [
        /// Receive enable
        RXE OFFSET(9) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ],
        /// Transmit enable
        TXE OFFSET(8) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ],
        /// Loopback: the transmitter feeds the receiver internally and
        /// nothing goes out on the line
        LBE OFFSET(7) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ],
        /// UART enable. Disabled in the middle of a byte, the UART finishes
        /// it before stopping.
        UARTEN OFFSET(0) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ]
    ],

    /// Interrupt FIFO Level Select Register
    IFLS [
        /// Receive interrupt trigger level
        RXIFLSEL OFFSET(3) NUMBITS(3) [
            OneEighth = 0b000,
            OneQuarter = 0b001,
            OneHalf = 0b010,
            ThreeQuarters = 0b011,
            SevenEighths = 0b100
        ],
        /// Transmit interrupt trigger level
        TXIFLSEL OFFSET(0) NUMBITS(3) [
            OneEighth = 0b000,
            OneQuarter = 0b001,
            OneHalf = 0b010,
            ThreeQuarters = 0b011,
            SevenEighths = 0b100
        ]
    ],

    /// Interrupt sources, as the mask, raw status, masked status and clear
    /// registers lay them out
    INT [
        /// Overrun error
        OE OFFSET(10) NUMBITS(1) [],
        /// Break error
        BE OFFSET(9) NUMBITS(1) [],
        /// Parity error
        PE OFFSET(8) NUMBITS(1) [],
        /// Framing error
        FE OFFSET(7) NUMBITS(1) [],
        /// Receive timeout
        RT OFFSET(6) NUMBITS(1) [],
        /// Transmit
        TX OFFSET(5) NUMBITS(1) [],
        /// Receive
        RX OFFSET(4) NUMBITS(1) [],
        /// Every source
        ALL OFFSET(0) NUMBITS(11) []
    ],

    /// DMA Control Register
    DMACR [
        /// Transmit DMA
        TXDMAE OFFSET(1) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ],
        /// Receive DMA
        RXDMAE OFFSET(0) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ]
    ],

    /// Peripheral Identification Register 0
    PERIPH_ID0 [
        /// Part number, low byte: 0x11 on every PL011
        PART_NUMBER0 OFFSET(0) NUMBITS(8) []
    ]
}

register_structs! {
    /// The PL011 register block (TRM, "Summary of registers").
    #[allow(non_snake_case)]
    pub RegisterBlock {
        (0x000 => Data: ReadWrite<u32, DR::Register>),
        (0x004 => ReceiveStatus: ReadWrite<u32>),
        (0x008 => _reserved0),
        (0x018 => Flag: ReadOnly<u32, FR::Register>),
        (0x01c => _reserved1),
        (0x024 => IntegerBaudRate: ReadWrite<u32>),
        (0x028 => FractionalBaudRate: ReadWrite<u32>),
        (0x02c => LineControl: ReadWrite<u32, LCR_H::Register>),
        (0x030 => Control: ReadWrite<u32, CR::Register>),
        (0x034 => InterruptFifoLevelSelect: ReadWrite<u32, IFLS::Register>),
        (0x038 => InterruptMaskSetClear: ReadWrite<u32, INT::Register>),
        (0x03c => RawInterruptStatus: ReadOnly<u32, INT::Register>),
        (0x040 => MaskedInterruptStatus: ReadOnly<u32, INT::Register>),
        (0x044 => InterruptClear: WriteOnly<u32, INT::Register>),
        (0x048 => DmaControl: ReadWrite<u32, DMACR::Register>),
        (0x04c => _reserved2),
        (0xfe0 => PeriphId0: ReadOnly<u32, PERIPH_ID0::Register>),
        (0xfe4 => _identification),
        (0x1000 => @END),
    }
}

/// `PART_NUMBER0` of every PL011.
pub const PART_NUMBER0: u32 = 0x11;

/// Most bytes one [`Pl011::service`] pass takes from the receive FIFO: a
/// device that keeps receiving cannot hold the driver in one pass.
pub const RECEIVE_BURST: usize = 32;

/// Most bytes one [`Pl011::service`] pass writes to the transmit FIFO: half
/// the shallowest FIFO, so in loopback the receive FIFO never overflows
/// before the next pass drains it.
pub const TRANSMIT_BURST: usize = 8;

/// What one [`Pl011::service`] pass did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Serviced {
    /// Bytes moved from the receive FIFO to the channel
    pub received: usize,
    /// Bytes moved from the channel to the transmit FIFO
    pub transmitted: usize,
    /// Received bytes dropped because the channel's receive ring was full
    pub dropped: usize,
}

impl Serviced {
    /// Whether the client has something new to look at.
    pub fn moved_bytes(&self) -> bool {
        self.received + self.transmitted > 0
    }
}

/// One PL011.
pub struct Pl011<'registers> {
    registers: &'registers RegisterBlock,
}

impl<'registers> Pl011<'registers> {
    /// The UART whose registers are `registers`.
    pub const fn new(registers: &'registers RegisterBlock) -> Self {
        Self { registers }
    }

    /// Whether the registers identify a PL011.
    pub fn is_pl011(&self) -> bool {
        self.registers.PeriphId0.read(PERIPH_ID0::PART_NUMBER0) == PART_NUMBER0
    }

    /// Bring the UART up for interrupt-driven use: 8N1 with FIFOs, receive
    /// and receive-timeout interrupts unmasked, the transmit interrupt
    /// masked until there is something to send, DMA off, and the
    /// transmitter and receiver on, looped back to each other if
    /// `loopback`.
    pub fn configure(&self, loopback: bool) {
        let registers = self.registers;
        registers.Control.write(CR::UARTEN::Disabled);
        // Let a byte in flight leave before the line settings change.
        while registers.Flag.is_set(FR::BUSY) {
            core::hint::spin_loop();
        }
        // Turning the FIFOs off flushes the transmit FIFO.
        registers.LineControl.modify(LCR_H::FEN::Disabled);
        registers.InterruptMaskSetClear.set(0);
        registers.InterruptClear.write(INT::ALL::SET);
        registers
            .DmaControl
            .write(DMACR::TXDMAE::Disabled + DMACR::RXDMAE::Disabled);
        // Writing LCR_H latches the baud rate divisors in place.
        registers.LineControl.write(
            LCR_H::WLEN::EightBit
                + LCR_H::FEN::Enabled
                + LCR_H::STP2::Disabled
                + LCR_H::PEN::Disabled,
        );
        registers
            .InterruptFifoLevelSelect
            .write(IFLS::RXIFLSEL::OneEighth + IFLS::TXIFLSEL::OneEighth);
        registers
            .InterruptMaskSetClear
            .write(INT::RX::SET + INT::RT::SET);
        registers.Control.write(
            CR::UARTEN::Enabled
                + CR::TXE::Enabled
                + CR::RXE::Enabled
                + if loopback {
                    CR::LBE::Enabled
                } else {
                    CR::LBE::Disabled
                },
        );
    }

    /// Mask every interrupt source and switch the UART off.
    pub fn quiesce(&self) {
        self.registers.InterruptMaskSetClear.set(0);
        self.registers.InterruptClear.write(INT::ALL::SET);
        self.registers.Control.write(CR::UARTEN::Disabled);
    }

    /// Move bytes between the FIFOs and `channel`: clear the pending
    /// interrupts, take up to [`RECEIVE_BURST`] received bytes, give the
    /// transmit FIFO up to [`TRANSMIT_BURST`] bytes, and unmask the transmit
    /// interrupt exactly while bytes remain to be sent.
    ///
    /// The interrupts are cleared first: a byte arriving after its FIFO was
    /// drained raises them again.
    pub fn service(&self, channel: &SerialChannel) -> Serviced {
        let registers = self.registers;
        let mut serviced = Serviced::default();
        registers.InterruptClear.write(INT::ALL::SET);

        while serviced.received + serviced.dropped < RECEIVE_BURST
            && !registers.Flag.is_set(FR::RXFE)
        {
            // DATA is 8 bits wide, so the conversion never fails.
            let byte = u8::try_from(registers.Data.read(DR::DATA)).unwrap_or_default();
            if channel.receive.push(byte) {
                serviced.received += 1;
            } else {
                serviced.dropped += 1;
            }
        }
        if serviced.dropped > 0 {
            channel
                .dropped
                .fetch_add(serviced.dropped as u64, Ordering::Relaxed);
        }

        while serviced.transmitted < TRANSMIT_BURST && !registers.Flag.is_set(FR::TXFF) {
            let Some(byte) = channel.transmit.pop() else {
                break;
            };
            registers.Data.write(DR::DATA.val(u32::from(byte)));
            serviced.transmitted += 1;
        }
        registers
            .InterruptMaskSetClear
            .modify(if channel.transmit.is_empty() {
                INT::TX::CLEAR
            } else {
                INT::TX::SET
            });
        serviced
    }
}

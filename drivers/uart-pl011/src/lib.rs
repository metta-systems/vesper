#![no_std]

//! Register logic of the Arm PL011 UART, serving a serial channel
//! (`libserial-channel`).
//!
//! The UART runs with its FIFOs on and three interrupt sources: receive
//! (the receive FIFO reached its trigger level), receive timeout (bytes wait
//! in a FIFO below the level) and transmit (the transmit FIFO drained
//! through its level). The driver serves it from two Threads:
//!
//! - the interrupt Thread takes the pending sources
//!   ([`Pl011::take_interrupts`]) and moves received bytes into the
//!   client's receive buffers ([`Pl011::receive`] through a [`Receiver`]);
//! - the transmit Thread writes the client's transmit packets to the FIFO
//!   one burst at a time ([`Pl011::transmit`]), waiting for the transmit
//!   interrupt between bursts.
//!
//! The two touch disjoint registers once the UART is configured: the
//! interrupt Thread reads the data and status registers and clears
//! interrupts, the transmit Thread writes the data register.
//!
//! The baud rate and the UART clock stay as the firmware set them: the
//! driver rewrites only the line control, which latches the divisors
//! already in place.
//!
//! Resources: the PL011 Technical Reference Manual (Arm DDI 0183), the
//! BCM2835/BCM2711 ARM Peripherals manuals, QEMU `hw/char/pl011.c`.

use {
    core::sync::atomic::{AtomicU64, Ordering},
    libio_channel::{Consumer, Corrupt, Cursor, DataArea, Event, NoEvent, Packet, Producer},
    libserial_channel::{SLOTS, SerialChannel},
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

/// Most bytes one [`Pl011::receive`] pass takes from the receive FIFO: a
/// device that keeps receiving cannot hold the driver in one pass.
pub const RECEIVE_BURST: usize = 32;

/// Most bytes one [`Pl011::transmit`] burst writes to the transmit FIFO:
/// half the shallowest FIFO, so in loopback the receive FIFO never
/// overflows before the receive side drains it.
pub const TRANSMIT_BURST: usize = 8;

/// The interrupt sources [`Pl011::take_interrupts`] found pending.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Interrupts {
    /// Received bytes wait, or a receive error happened
    pub receive: bool,
    /// The transmit FIFO drained to its trigger level
    pub transmit: bool,
}

/// What one [`Pl011::receive`] pass did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Received {
    /// Bytes stored in the client's receive buffers
    pub stored: usize,
    /// Bytes dropped for want of a receive buffer
    pub dropped: usize,
}

/// The driver's side of a serial channel's `receive` IO channel: it takes
/// the empty buffers the client submits, fills them byte by byte, and
/// completes them back.
pub struct Receiver<'channel, Completed: Event> {
    requests: Consumer<'channel, SLOTS, NoEvent>,
    completions: Producer<'channel, SLOTS, Completed>,
    dropped: &'channel AtomicU64,
    area: DataArea,
    /// The buffer being filled and how far
    filling: Option<(Packet, Cursor)>,
}

impl<'channel, Completed: Event> Receiver<'channel, Completed> {
    /// Serve `channel`'s receive side, advancing `completed` after
    /// completing buffers, which must lie in `area`.
    ///
    /// # Safety
    ///
    /// `area` is mapped read/write for as long as the receiver is used.
    pub unsafe fn new(
        channel: &'channel SerialChannel,
        completed: Completed,
        area: DataArea,
    ) -> Self {
        let (requests, completions) = channel.receive.server(NoEvent, completed);
        Self {
            requests,
            completions,
            dropped: &channel.dropped,
            area,
            filling: None,
        }
    }

    /// Give `packet` back to the client, cut to the bytes stored. A client
    /// with more buffers out than the FIFO holds loses the extra ones.
    fn complete(&mut self, mut packet: Packet, cursor: Cursor) {
        cursor.cut(&mut packet);
        let _lost = self.completions.put(&packet);
    }

    /// Store `byte` in the client's buffers; `false` if none is submitted.
    fn store(&mut self, byte: u8) -> bool {
        loop {
            let (packet, mut cursor) = match self.filling.take() {
                Some(filling) => filling,
                None => match self.requests.get() {
                    Ok(Some(packet)) if self.area.holds(&packet) => (packet, Cursor::START),
                    // A buffer outside the data area comes back empty.
                    Ok(Some(packet)) => {
                        self.complete(packet, Cursor::START);
                        continue;
                    }
                    Ok(None) | Err(Corrupt) => return false,
                },
            };
            let Some(address) = cursor.next_address(&packet) else {
                // A buffer without room comes back as it was.
                self.complete(packet, cursor);
                continue;
            };
            // SAFETY: `area` holds the packet and is mapped read/write
            // (`Self::new`); the client gave the buffer to the driver until
            // it comes back.
            unsafe {
                (address as *mut u8).write_volatile(byte);
            }
            if cursor.is_at_end(&packet) {
                self.complete(packet, cursor);
            } else {
                self.filling = Some((packet, cursor));
            }
            return true;
        }
    }

    /// Complete the buffer being filled, if anything was stored in it.
    fn flush(&mut self) {
        if let Some((packet, cursor)) = self.filling.take() {
            if cursor.passed(&packet) > 0 {
                self.complete(packet, cursor);
            } else {
                self.filling = Some((packet, cursor));
            }
        }
    }

    /// Tell the client about the buffers completed since the last signal.
    pub fn signal(&mut self) -> Result<(), Completed::Error> {
        self.completions.signal()
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

    /// Bring the UART up for interrupt-driven use: 8N1 with FIFOs, the
    /// receive, receive-timeout and transmit interrupts unmasked, DMA off,
    /// and the transmitter and receiver on, looped back to each other if
    /// `loopback`.
    ///
    /// The transmit interrupt rises only as the transmit FIFO drains
    /// through its trigger level, and clearing it keeps it low until the
    /// next time, so it stays unmasked: an idle transmitter raises nothing.
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
            .write(INT::RX::SET + INT::RT::SET + INT::TX::SET);
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

    /// Clear the pending interrupt sources and say which they were. Only
    /// the sources read are cleared, so one rising meanwhile stays pending.
    pub fn take_interrupts(&self) -> Interrupts {
        let pending = self.registers.MaskedInterruptStatus.extract();
        self.registers.InterruptClear.set(pending.get());
        Interrupts {
            receive: pending.matches_any(&[
                INT::RX::SET,
                INT::RT::SET,
                INT::OE::SET,
                INT::BE::SET,
                INT::PE::SET,
                INT::FE::SET,
            ]),
            transmit: pending.is_set(INT::TX),
        }
    }

    /// Move up to [`RECEIVE_BURST`] bytes from the receive FIFO into the
    /// client's buffers through `receiver`, counting the ones without a
    /// buffer as dropped, then complete the buffer being filled: the device
    /// has nothing more for now, so the client gets what arrived.
    pub fn receive<Completed: Event>(&self, receiver: &mut Receiver<'_, Completed>) -> Received {
        let registers = self.registers;
        let mut pass = Received::default();
        while pass.stored + pass.dropped < RECEIVE_BURST && !registers.Flag.is_set(FR::RXFE) {
            // DATA is 8 bits wide, so the conversion never fails.
            let byte = u8::try_from(registers.Data.read(DR::DATA)).unwrap_or_default();
            if receiver.store(byte) {
                pass.stored += 1;
            } else {
                pass.dropped += 1;
            }
        }
        if pass.dropped > 0 {
            receiver
                .dropped
                .fetch_add(pass.dropped as u64, Ordering::Relaxed);
        }
        receiver.flush();
        pass
    }

    /// Write the bytes `next` yields to the transmit FIFO until
    /// [`TRANSMIT_BURST`] were written, the FIFO is full, or `next` runs
    /// dry; how many were written. `next` is only asked for a byte the FIFO
    /// has room for.
    pub fn transmit(&self, mut next: impl FnMut() -> Option<u8>) -> usize {
        let mut written = 0;
        while written < TRANSMIT_BURST && !self.registers.Flag.is_set(FR::TXFF) {
            let Some(byte) = next() else {
                break;
            };
            self.registers.Data.write(DR::DATA.val(u32::from(byte)));
            written += 1;
        }
        written
    }
}

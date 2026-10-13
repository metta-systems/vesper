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
    bitbybit::{bitenum, bitfield},
    core::sync::atomic::{AtomicU64, Ordering},
    derive_mmio::Mmio,
    libio_channel::{Consumer, Corrupt, Cursor, DataArea, Event, NoEvent, Packet, Producer},
    libmmio::assert_register_offsets,
    libserial_channel::{SLOTS, SerialChannel},
};

/// Data Register: reading pops the receive FIFO, writing pushes the
/// transmit FIFO.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Data {
    /// Received or transmitted byte
    #[bits(0..=7, rw)]
    byte: u8,
    /// Framing error
    #[bit(8, r)]
    framing_error: bool,
    /// Parity error
    #[bit(9, r)]
    parity_error: bool,
    /// Break error
    #[bit(10, r)]
    break_error: bool,
    /// Overrun error: the receive FIFO was full when a byte arrived
    #[bit(11, r)]
    overrun_error: bool,
}

/// Flag Register.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Flags {
    /// UART busy: set while a byte, including its stop bits, is being sent
    /// from the shift register, and as soon as the transmit FIFO is not empty
    #[bit(3, r)]
    busy: bool,
    /// Receive FIFO empty
    #[bit(4, r)]
    receive_fifo_empty: bool,
    /// Transmit FIFO full
    #[bit(5, r)]
    transmit_fifo_full: bool,
    /// Receive FIFO full
    #[bit(6, r)]
    receive_fifo_full: bool,
    /// Transmit FIFO empty. With the FIFOs on, set when the transmit FIFO is
    /// empty; the shift register may still be sending.
    #[bit(7, r)]
    transmit_fifo_empty: bool,
}

/// Data bits per frame.
#[bitenum(u2, exhaustive = true)]
#[derive(Debug, PartialEq, Eq)]
pub enum WordLength {
    FiveBits = 0b00,
    SixBits = 0b01,
    SevenBits = 0b10,
    EightBits = 0b11,
}

/// Line Control Register.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct LineControl {
    /// Parity checking and generation
    #[bit(1, rw)]
    parity_enable: bool,
    /// Two stop bits
    #[bit(3, rw)]
    two_stop_bits: bool,
    /// FIFOs on; off makes them one-byte holding registers
    #[bit(4, rw)]
    fifo_enable: bool,
    /// Data bits per frame
    #[bits(5..=6, rw)]
    word_length: WordLength,
}

/// Control Register.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Control {
    /// UART enable. Disabled in the middle of a byte, the UART finishes it
    /// before stopping.
    #[bit(0, rw)]
    uart_enable: bool,
    /// Loopback: the transmitter feeds the receiver internally and nothing
    /// goes out on the line
    #[bit(7, rw)]
    loopback_enable: bool,
    /// Transmit enable
    #[bit(8, rw)]
    transmit_enable: bool,
    /// Receive enable
    #[bit(9, rw)]
    receive_enable: bool,
}

/// FIFO fill level that raises an interrupt.
#[bitenum(u3, exhaustive = false)]
#[derive(Debug, PartialEq, Eq)]
pub enum FifoLevel {
    OneEighth = 0b000,
    OneQuarter = 0b001,
    OneHalf = 0b010,
    ThreeQuarters = 0b011,
    SevenEighths = 0b100,
}

/// Interrupt FIFO Level Select Register.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct InterruptFifoLevelSelect {
    /// Transmit interrupt trigger level
    #[bits(0..=2, rw)]
    transmit: Option<FifoLevel>,
    /// Receive interrupt trigger level
    #[bits(3..=5, rw)]
    receive: Option<FifoLevel>,
}

/// Interrupt sources, as the mask, raw status, masked status and clear
/// registers lay them out.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct InterruptSources {
    /// Receive
    #[bit(4, rw)]
    receive: bool,
    /// Transmit
    #[bit(5, rw)]
    transmit: bool,
    /// Receive timeout
    #[bit(6, rw)]
    receive_timeout: bool,
    /// Framing error
    #[bit(7, rw)]
    framing_error: bool,
    /// Parity error
    #[bit(8, rw)]
    parity_error: bool,
    /// Break error
    #[bit(9, rw)]
    break_error: bool,
    /// Overrun error
    #[bit(10, rw)]
    overrun_error: bool,
}

impl InterruptSources {
    /// Every source, the modem status ones (bits 0-3) included.
    pub const ALL: Self = Self::new_with_raw_value(0x7ff);

    /// Whether a receive source is set: received bytes wait, or a receive
    /// error happened.
    pub const fn any_receive(&self) -> bool {
        self.receive()
            || self.receive_timeout()
            || self.overrun_error()
            || self.break_error()
            || self.parity_error()
            || self.framing_error()
    }
}

/// DMA Control Register.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct DmaControl {
    /// Receive DMA
    #[bit(0, rw)]
    receive_dma_enable: bool,
    /// Transmit DMA
    #[bit(1, rw)]
    transmit_dma_enable: bool,
}

/// Peripheral Identification Register 0.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct PeripheralId0 {
    /// Part number, low byte: 0x11 on every PL011
    #[bits(0..=7, r)]
    part_number0: u8,
}

/// The PL011 register block (TRM, "Summary of registers").
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
pub struct RegisterBlock {
    #[mmio(Read, Write)]
    data: Data,
    #[mmio(Read, Write)]
    receive_status: u32,
    _reserved0: [u32; 4],
    #[mmio(PureRead)]
    flag: Flags,
    _reserved1: [u32; 2],
    integer_baud_rate: u32,
    fractional_baud_rate: u32,
    line_control: LineControl,
    control: Control,
    interrupt_fifo_level_select: InterruptFifoLevelSelect,
    interrupt_mask: InterruptSources,
    #[mmio(PureRead)]
    raw_interrupt_status: InterruptSources,
    #[mmio(PureRead)]
    masked_interrupt_status: InterruptSources,
    #[mmio(Write)]
    interrupt_clear: InterruptSources,
    dma_control: DmaControl,
    _reserved2: [u32; 997],
    #[mmio(PureRead)]
    peripheral_id0: PeripheralId0,
    _identification: [u32; 7],
}

assert_register_offsets!(RegisterBlock, 0x1000, {
    data: 0x000,
    receive_status: 0x004,
    flag: 0x018,
    integer_baud_rate: 0x024,
    fractional_baud_rate: 0x028,
    line_control: 0x02c,
    control: 0x030,
    interrupt_fifo_level_select: 0x034,
    interrupt_mask: 0x038,
    raw_interrupt_status: 0x03c,
    masked_interrupt_status: 0x040,
    interrupt_clear: 0x044,
    dma_control: 0x048,
    peripheral_id0: 0xfe0,
});

/// `PART_NUMBER0` of every PL011.
pub const PART_NUMBER0: u8 = 0x11;

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
    registers: MmioRegisterBlock<'registers>,
}

impl<'registers> Pl011<'registers> {
    /// The UART whose registers are `registers`.
    pub const fn new(registers: MmioRegisterBlock<'registers>) -> Self {
        Self { registers }
    }

    /// Whether the registers identify a PL011.
    pub fn is_pl011(&self) -> bool {
        self.registers.read_peripheral_id0().part_number0() == PART_NUMBER0
    }

    /// Bring the UART up for interrupt-driven use: 8N1 with FIFOs, the
    /// receive, receive-timeout and transmit interrupts unmasked, DMA off,
    /// and the transmitter and receiver on, looped back to each other if
    /// `loopback`.
    ///
    /// The transmit interrupt rises only as the transmit FIFO drains
    /// through its trigger level, and clearing it keeps it low until the
    /// next time, so it stays unmasked: an idle transmitter raises nothing.
    pub fn configure(&mut self, loopback: bool) {
        let registers = &mut self.registers;
        registers.write_control(Control::ZERO);
        // Let a byte in flight leave before the line settings change.
        while registers.read_flag().busy() {
            core::hint::spin_loop();
        }
        // Turning the FIFOs off flushes the transmit FIFO.
        registers.modify_line_control(|line| line.with_fifo_enable(false));
        registers.write_interrupt_mask(InterruptSources::ZERO);
        registers.write_interrupt_clear(InterruptSources::ALL);
        registers.write_dma_control(
            DmaControl::ZERO
                .with_transmit_dma_enable(false)
                .with_receive_dma_enable(false),
        );
        // Writing LCR_H latches the baud rate divisors in place.
        registers.write_line_control(
            LineControl::ZERO
                .with_word_length(WordLength::EightBits)
                .with_fifo_enable(true)
                .with_two_stop_bits(false)
                .with_parity_enable(false),
        );
        registers.write_interrupt_fifo_level_select(
            InterruptFifoLevelSelect::ZERO
                .with_receive(FifoLevel::OneEighth)
                .with_transmit(FifoLevel::OneEighth),
        );
        registers.write_interrupt_mask(
            InterruptSources::ZERO
                .with_receive(true)
                .with_receive_timeout(true)
                .with_transmit(true),
        );
        registers.write_control(
            Control::ZERO
                .with_uart_enable(true)
                .with_transmit_enable(true)
                .with_receive_enable(true)
                .with_loopback_enable(loopback),
        );
    }

    /// Mask every interrupt source and switch the UART off.
    pub fn quiesce(&mut self) {
        self.registers.write_interrupt_mask(InterruptSources::ZERO);
        self.registers.write_interrupt_clear(InterruptSources::ALL);
        self.registers.write_control(Control::ZERO);
    }

    /// Clear the pending interrupt sources and say which they were. Only
    /// the sources read are cleared, so one rising meanwhile stays pending.
    pub fn take_interrupts(&mut self) -> Interrupts {
        let pending = self.registers.read_masked_interrupt_status();
        self.registers.write_interrupt_clear(pending);
        Interrupts {
            receive: pending.any_receive(),
            transmit: pending.transmit(),
        }
    }

    /// Move up to [`RECEIVE_BURST`] bytes from the receive FIFO into the
    /// client's buffers through `receiver`, counting the ones without a
    /// buffer as dropped, then complete the buffer being filled: the device
    /// has nothing more for now, so the client gets what arrived.
    pub fn receive<Completed: Event>(
        &mut self,
        receiver: &mut Receiver<'_, Completed>,
    ) -> Received {
        let mut pass = Received::default();
        while pass.stored + pass.dropped < RECEIVE_BURST
            && !self.registers.read_flag().receive_fifo_empty()
        {
            let byte = self.registers.read_data().byte();
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
    pub fn transmit(&mut self, mut next: impl FnMut() -> Option<u8>) -> usize {
        let mut written = 0;
        while written < TRANSMIT_BURST && !self.registers.read_flag().transmit_fifo_full() {
            let Some(byte) = next() else {
                break;
            };
            self.registers.write_data(Data::ZERO.with_byte(byte));
            written += 1;
        }
        written
    }
}

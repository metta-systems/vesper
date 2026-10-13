//! Host tests of the PL011 register logic against an in-memory register
//! block.
//!
//! Each test plays the hardware through a *device view*: the PL011 register
//! layout with every register readable and writable, so a test can present
//! flags and received data and observe what the driver programmed.

#[cfg(test)]
mod tests {
    use {
        core::{cell::Cell, sync::atomic::Ordering},
        derive_mmio::Mmio,
        libio_channel::{DataArea, Event, IoRec, NoEvent, Packet},
        libserial_channel::SerialChannel,
        uart_pl011::{
            Interrupts, PART_NUMBER0, Pl011, RECEIVE_BURST, Received, Receiver, RegisterBlock,
            TRANSMIT_BURST,
        },
    };

    /// The PL011 register block as the hardware sees it, with plain words so
    /// the tests check the bit positions the driver uses.
    #[derive(Mmio)]
    #[mmio(no_owned)]
    #[repr(C)]
    struct Device {
        data: u32,
        receive_status: u32,
        _reserved0: [u32; 4],
        flag: u32,
        _reserved1: [u32; 2],
        integer_baud_rate: u32,
        fractional_baud_rate: u32,
        line_control: u32,
        control: u32,
        interrupt_fifo_level_select: u32,
        interrupt_mask: u32,
        raw_interrupt_status: u32,
        masked_interrupt_status: u32,
        interrupt_clear: u32,
        dma_control: u32,
        _reserved2: [u32; 997],
        peripheral_id0: u32,
        _identification: [u32; 7],
    }

    const _: () = assert!(size_of::<Device>() == size_of::<RegisterBlock>());

    const FR_RXFE: u32 = 1 << 4;
    const FR_TXFF: u32 = 1 << 5;
    const INT_RX: u32 = 1 << 4;
    const INT_TX: u32 = 1 << 5;
    const INT_RT: u32 = 1 << 6;
    const INT_OE: u32 = 1 << 10;
    const CR_UARTEN: u32 = 1 << 0;
    const CR_LBE: u32 = 1 << 7;
    const CR_TXE: u32 = 1 << 8;
    const CR_RXE: u32 = 1 << 9;
    const LCR_H_FEN: u32 = 1 << 4;
    const LCR_H_WLEN_8: u32 = 0b11 << 5;

    /// A device view and a driver over the same zeroed, leaked block.
    fn device() -> (MmioDevice<'static>, Pl011<'static>) {
        let words = size_of::<Device>().div_ceil(size_of::<u64>());
        let memory: &'static mut [u64] = Box::leak(vec![0_u64; words].into_boxed_slice());
        let block = memory.as_mut_ptr();
        // SAFETY: zeroed, aligned memory of the block's size, leaked for
        // 'static; the tests use the two handles one at a time.
        unsafe {
            (
                Device::new_mmio(block.cast::<Device>()),
                Pl011::new(RegisterBlock::new_mmio(block.cast::<RegisterBlock>())),
            )
        }
    }

    #[test]
    fn a_pl011_identifies_itself() {
        let (mut device, uart) = device();
        assert!(!uart.is_pl011());
        device.write_peripheral_id0(u32::from(PART_NUMBER0));
        assert!(uart.is_pl011());
    }

    #[test]
    fn configure_sets_8n1_with_fifos_and_all_three_interrupts() {
        let (mut device, mut uart) = device();
        device.write_dma_control(0b11);
        device.write_interrupt_mask(0x7ff);
        uart.configure(false);
        assert_eq!(device.read_line_control(), LCR_H_WLEN_8 | LCR_H_FEN);
        assert_eq!(device.read_interrupt_mask(), INT_RX | INT_RT | INT_TX);
        assert_eq!(
            device.read_interrupt_fifo_level_select(),
            0,
            "both trigger levels at 1/8"
        );
        assert_eq!(device.read_dma_control(), 0);
        assert_eq!(
            device.read_interrupt_clear(),
            0x7ff,
            "every pending interrupt cleared"
        );
        assert_eq!(device.read_control(), CR_UARTEN | CR_TXE | CR_RXE);
    }

    #[test]
    fn configure_keeps_the_firmware_baud_rate() {
        let (mut device, mut uart) = device();
        device.write_integer_baud_rate(26);
        device.write_fractional_baud_rate(3);
        uart.configure(false);
        assert_eq!(
            (
                device.read_integer_baud_rate(),
                device.read_fractional_baud_rate()
            ),
            (26, 3)
        );
    }

    #[test]
    fn configure_loops_back_on_request() {
        let (device, mut uart) = device();
        uart.configure(true);
        assert_eq!(device.read_control(), CR_UARTEN | CR_TXE | CR_RXE | CR_LBE);
    }

    #[test]
    fn quiesce_masks_everything_and_switches_off() {
        let (device, mut uart) = device();
        uart.configure(true);
        uart.quiesce();
        assert_eq!(device.read_interrupt_mask(), 0);
        assert_eq!(device.read_control() & CR_UARTEN, 0);
    }

    /// A data area over leaked host memory, so records name real bytes.
    fn data_area(bytes: usize) -> (&'static mut [u8], DataArea) {
        let memory: &'static mut [u8] = Box::leak(vec![0_u8; bytes].into_boxed_slice());
        let area = DataArea {
            base: memory.as_ptr() as u64,
            len: bytes as u64,
        };
        (memory, area)
    }

    fn buffer(value: u64, recs: &[IoRec]) -> Packet {
        Packet::new(value, recs).expect("a well-formed packet")
    }

    /// The receive completion count, single-threaded.
    #[derive(Default)]
    struct Completions(Cell<u64>);

    impl Event for &Completions {
        type Error = ();

        fn advance(&self) -> Result<(), ()> {
            self.0.set(self.0.get() + 1);
            Ok(())
        }

        fn await_at_least(&self, _target: u64) -> Result<u64, ()> {
            unreachable!("the driver never waits for its own completions")
        }
    }

    #[test]
    fn take_interrupts_clears_exactly_the_pending_sources() {
        let (mut device, mut uart) = device();
        device.write_masked_interrupt_status(INT_RX | INT_TX);
        assert_eq!(
            uart.take_interrupts(),
            Interrupts {
                receive: true,
                transmit: true
            }
        );
        assert_eq!(device.read_interrupt_clear(), INT_RX | INT_TX);

        device.write_masked_interrupt_status(INT_RT);
        assert_eq!(
            uart.take_interrupts(),
            Interrupts {
                receive: true,
                transmit: false
            }
        );
        assert_eq!(device.read_interrupt_clear(), INT_RT);

        device.write_masked_interrupt_status(INT_OE);
        assert!(
            uart.take_interrupts().receive,
            "an overrun is a receive event"
        );

        device.write_masked_interrupt_status(INT_TX);
        assert_eq!(
            uart.take_interrupts(),
            Interrupts {
                receive: false,
                transmit: true
            }
        );
    }

    #[test]
    fn received_bytes_without_a_buffer_are_dropped_and_counted() {
        let (mut device, mut uart) = device();
        let channel = SerialChannel::new();
        let (_memory, area) = data_area(16);
        let completions = Completions::default();
        // SAFETY: the area is leaked host memory.
        let mut receiver = unsafe { Receiver::new(&channel, &completions, area) };
        device.write_data(0x41);
        let pass = uart.receive(&mut receiver);
        assert_eq!(
            pass,
            Received {
                stored: 0,
                dropped: RECEIVE_BURST
            }
        );
        assert_eq!(
            channel.dropped.load(Ordering::Relaxed),
            RECEIVE_BURST as u64
        );
        receiver.signal().expect("advance");
        assert_eq!(
            completions.0.get(),
            0,
            "nothing completed, nothing signalled"
        );
    }

    #[test]
    fn a_receive_pass_fills_buffers_and_returns_the_last_part_full() {
        let (mut device, mut uart) = device();
        let channel = SerialChannel::new();
        let (memory, area) = data_area(64);
        let base = area.base;
        let completions = Completions::default();
        let (mut submit, _) = channel.receive.client(NoEvent, NoEvent);
        // Two records of 4 and 8 bytes, then a 40-byte buffer: one pass of
        // RECEIVE_BURST bytes fills the first and part of the second.
        submit
            .put(&buffer(
                1,
                &[
                    IoRec { base, len: 4 },
                    IoRec {
                        base: base + 8,
                        len: 8,
                    },
                ],
            ))
            .expect("room");
        submit
            .put(&buffer(
                2,
                &[IoRec {
                    base: base + 16,
                    len: 40,
                }],
            ))
            .expect("room");
        // SAFETY: the area is leaked host memory.
        let mut receiver = unsafe { Receiver::new(&channel, &completions, area) };
        // A byte with a framing error flag: only its data bits are taken.
        device.write_data(0x100 | 0x41);
        let pass = uart.receive(&mut receiver);
        assert_eq!(
            pass,
            Received {
                stored: RECEIVE_BURST,
                dropped: 0
            }
        );
        receiver.signal().expect("advance");
        assert_eq!(completions.0.get(), 1, "one signal for the pass");

        assert_eq!(
            channel.receive.complete.get(),
            Ok(Some(buffer(
                1,
                &[
                    IoRec { base, len: 4 },
                    IoRec {
                        base: base + 8,
                        len: 8
                    }
                ]
            )))
        );
        let rest = (RECEIVE_BURST - 12) as u64;
        assert_eq!(
            channel.receive.complete.get(),
            Ok(Some(buffer(
                2,
                &[IoRec {
                    base: base + 16,
                    len: rest
                }]
            ))),
            "the second buffer comes back cut to what arrived"
        );
        assert_eq!(channel.receive.complete.get(), Ok(None));
        // The first record, the gap between the records, the second record
        // and the second buffer's part, and the byte after it.
        let mut expected = vec![0x41; 4];
        expected.extend([0; 4]);
        expected.extend(vec![0x41; 8 + RECEIVE_BURST - 12]);
        expected.push(0);
        assert_eq!(&memory[..expected.len()], expected);
    }

    #[test]
    fn a_pass_that_stores_nothing_keeps_the_buffer() {
        let (mut device, mut uart) = device();
        let channel = SerialChannel::new();
        let (_memory, area) = data_area(16);
        let base = area.base;
        let completions = Completions::default();
        let (mut submit, _) = channel.receive.client(NoEvent, NoEvent);
        submit
            .put(&buffer(1, &[IoRec { base, len: 16 }]))
            .expect("room");
        // SAFETY: the area is leaked host memory.
        let mut receiver = unsafe { Receiver::new(&channel, &completions, area) };
        device.write_flag(FR_RXFE);
        assert_eq!(uart.receive(&mut receiver), Received::default());
        assert_eq!(channel.receive.complete.get(), Ok(None));

        // The next byte goes into the same buffer.
        device.write_flag(0);
        device.write_data(0x42);
        uart.receive(&mut receiver);
        assert_eq!(
            channel.receive.complete.get(),
            Ok(Some(buffer(1, &[IoRec { base, len: 16 }])))
        );
    }

    #[test]
    fn a_buffer_outside_the_data_area_comes_back_empty() {
        let (mut device, mut uart) = device();
        let channel = SerialChannel::new();
        let (memory, area) = data_area(64);
        let base = area.base;
        let completions = Completions::default();
        let (mut submit, _) = channel.receive.client(NoEvent, NoEvent);
        let outside = buffer(
            1,
            &[IoRec {
                base: base + 60,
                len: 8,
            }],
        );
        submit.put(&outside).expect("room");
        submit
            .put(&buffer(2, &[IoRec { base, len: 64 }]))
            .expect("room");
        // SAFETY: the area is leaked host memory.
        let mut receiver = unsafe { Receiver::new(&channel, &completions, area) };
        device.write_data(0x43);
        uart.receive(&mut receiver);
        assert_eq!(
            channel.receive.complete.get(),
            Ok(Some(buffer(
                1,
                &[IoRec {
                    base: base + 60,
                    len: 0
                }]
            )))
        );
        assert_eq!(
            channel.receive.complete.get(),
            Ok(Some(buffer(
                2,
                &[IoRec {
                    base,
                    len: RECEIVE_BURST as u64
                }]
            )))
        );
        // Every byte went to the second buffer, from its start.
        let (stored, untouched) = memory.split_at(RECEIVE_BURST);
        assert!(stored.iter().all(|&byte| byte == 0x43));
        assert!(untouched.iter().all(|&byte| byte == 0));
    }

    #[test]
    fn transmit_writes_one_burst() {
        let (device, mut uart) = device();
        let mut bytes = 0..20_u8;
        assert_eq!(uart.transmit(|| bytes.next()), TRANSMIT_BURST);
        assert_eq!(
            device.read_data(),
            u32::try_from(TRANSMIT_BURST).unwrap() - 1,
            "the last byte written"
        );
        assert_eq!(bytes.next(), Some(u8::try_from(TRANSMIT_BURST).unwrap()));
    }

    #[test]
    fn transmit_stops_when_the_bytes_run_out() {
        let (device, mut uart) = device();
        let mut bytes = [1_u8, 2, 3].into_iter();
        assert_eq!(uart.transmit(|| bytes.next()), 3);
        assert_eq!(device.read_data(), 3);
    }

    #[test]
    fn transmit_takes_no_byte_a_full_fifo_has_no_room_for() {
        let (mut device, mut uart) = device();
        device.write_flag(FR_TXFF);
        let mut asked = 0;
        assert_eq!(
            uart.transmit(|| {
                asked += 1;
                Some(7)
            }),
            0
        );
        assert_eq!(asked, 0);
    }
}

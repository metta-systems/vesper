//! Host tests of the PL011 register logic against an in-memory register
//! block.
//!
//! Each test plays the hardware through a *device view*: the PL011 register
//! layout with every register readable and writable, so a test can present
//! flags and received data and observe what the driver programmed.

#[cfg(test)]
mod tests {
    use {
        core::sync::atomic::Ordering,
        libserial_channel::{RING_BYTES, SerialChannel},
        tock_registers::{
            interfaces::{Readable, Writeable},
            register_structs,
            registers::ReadWrite,
        },
        uart_pl011::{PART_NUMBER0, Pl011, RECEIVE_BURST, RegisterBlock, Serviced, TRANSMIT_BURST},
    };

    register_structs! {
        /// The PL011 as the hardware sees it.
        #[allow(non_snake_case)]
        Device {
            (0x000 => DR: ReadWrite<u32>),
            (0x004 => RSR: ReadWrite<u32>),
            (0x008 => _reserved0),
            (0x018 => FR: ReadWrite<u32>),
            (0x01c => _reserved1),
            (0x024 => IBRD: ReadWrite<u32>),
            (0x028 => FBRD: ReadWrite<u32>),
            (0x02c => LCR_H: ReadWrite<u32>),
            (0x030 => CR: ReadWrite<u32>),
            (0x034 => IFLS: ReadWrite<u32>),
            (0x038 => IMSC: ReadWrite<u32>),
            (0x03c => RIS: ReadWrite<u32>),
            (0x040 => MIS: ReadWrite<u32>),
            (0x044 => ICR: ReadWrite<u32>),
            (0x048 => DMACR: ReadWrite<u32>),
            (0x04c => _reserved2),
            (0xfe0 => PERIPH_ID0: ReadWrite<u32>),
            (0xfe4 => _identification),
            (0x1000 => @END),
        }
    }

    const _: () = assert!(size_of::<Device>() == size_of::<RegisterBlock>());

    const FR_RXFE: u32 = 1 << 4;
    const FR_TXFF: u32 = 1 << 5;
    const INT_RX: u32 = 1 << 4;
    const INT_TX: u32 = 1 << 5;
    const INT_RT: u32 = 1 << 6;
    const CR_UARTEN: u32 = 1 << 0;
    const CR_LBE: u32 = 1 << 7;
    const CR_TXE: u32 = 1 << 8;
    const CR_RXE: u32 = 1 << 9;
    const LCR_H_FEN: u32 = 1 << 4;
    const LCR_H_WLEN_8: u32 = 0b11 << 5;

    /// Zeroed, leaked backing memory for the device, and the driver's view
    /// of it.
    fn device() -> (&'static Device, Pl011<'static>) {
        let words = size_of::<Device>().div_ceil(size_of::<u64>());
        let memory: &'static mut [u64] = Box::leak(vec![0_u64; words].into_boxed_slice());
        // SAFETY: zeroed, aligned memory of the block's size, leaked for
        // 'static; both views are plain `u32` cells at the same offsets.
        let (device, registers) = unsafe {
            (
                &*memory.as_ptr().cast::<Device>(),
                &*memory.as_ptr().cast::<RegisterBlock>(),
            )
        };
        (device, Pl011::new(registers))
    }

    #[test]
    fn a_pl011_identifies_itself() {
        let (device, uart) = device();
        assert!(!uart.is_pl011());
        device.PERIPH_ID0.set(PART_NUMBER0);
        assert!(uart.is_pl011());
    }

    #[test]
    fn configure_sets_8n1_with_fifos_and_receive_interrupts() {
        let (device, uart) = device();
        device.DMACR.set(0b11);
        device.IMSC.set(0x7ff);
        uart.configure(false);
        assert_eq!(device.LCR_H.get(), LCR_H_WLEN_8 | LCR_H_FEN);
        assert_eq!(device.IMSC.get(), INT_RX | INT_RT);
        assert_eq!(device.IFLS.get(), 0, "both trigger levels at 1/8");
        assert_eq!(device.DMACR.get(), 0);
        assert_eq!(device.ICR.get(), 0x7ff, "every pending interrupt cleared");
        assert_eq!(device.CR.get(), CR_UARTEN | CR_TXE | CR_RXE);
    }

    #[test]
    fn configure_keeps_the_firmware_baud_rate() {
        let (device, uart) = device();
        device.IBRD.set(26);
        device.FBRD.set(3);
        uart.configure(false);
        assert_eq!((device.IBRD.get(), device.FBRD.get()), (26, 3));
    }

    #[test]
    fn configure_loops_back_on_request() {
        let (device, uart) = device();
        uart.configure(true);
        assert_eq!(device.CR.get(), CR_UARTEN | CR_TXE | CR_RXE | CR_LBE);
    }

    #[test]
    fn quiesce_masks_everything_and_switches_off() {
        let (device, uart) = device();
        uart.configure(true);
        uart.quiesce();
        assert_eq!(device.IMSC.get(), 0);
        assert_eq!(device.CR.get() & CR_UARTEN, 0);
    }

    #[test]
    fn service_takes_one_burst_of_received_bytes() {
        let (device, uart) = device();
        let channel = SerialChannel::new();
        // A byte with a framing error flag: only its data bits are taken.
        device.DR.set(0x100 | 0x41);
        device.FR.set(FR_TXFF);
        let serviced = uart.service(&channel);
        assert_eq!(
            serviced,
            Serviced {
                received: RECEIVE_BURST,
                transmitted: 0,
                dropped: 0
            }
        );
        assert_eq!(channel.receive.len(), RECEIVE_BURST);
        assert_eq!(channel.receive.pop(), Some(0x41));
        assert_eq!(device.ICR.get(), 0x7ff);
        assert!(serviced.moved_bytes());
    }

    #[test]
    fn service_counts_bytes_a_full_receive_ring_drops() {
        let (device, uart) = device();
        let channel = SerialChannel::new();
        for _ in 0..RING_BYTES - 2 {
            assert!(channel.receive.push(0));
        }
        device.DR.set(0x42);
        device.FR.set(FR_TXFF);
        let serviced = uart.service(&channel);
        assert_eq!(serviced.received, 2);
        assert_eq!(serviced.dropped, RECEIVE_BURST - 2);
        assert_eq!(
            channel.dropped.load(Ordering::Relaxed),
            (RECEIVE_BURST - 2) as u64
        );
    }

    #[test]
    fn service_sends_and_masks_transmit_when_the_ring_empties() {
        let (device, uart) = device();
        let channel = SerialChannel::new();
        uart.configure(false);
        for byte in [1, 2, 3] {
            assert!(channel.transmit.push(byte));
        }
        device.FR.set(FR_RXFE);
        let serviced = uart.service(&channel);
        assert_eq!(serviced.transmitted, 3);
        assert_eq!(device.DR.get(), 3, "the last byte written");
        assert!(channel.transmit.is_empty());
        assert_eq!(device.IMSC.get(), INT_RX | INT_RT);
    }

    #[test]
    fn service_sends_one_burst_and_unmasks_transmit_while_bytes_remain() {
        let (device, uart) = device();
        let channel = SerialChannel::new();
        uart.configure(false);
        for byte in 0..20 {
            assert!(channel.transmit.push(byte));
        }
        device.FR.set(FR_RXFE);
        let serviced = uart.service(&channel);
        assert_eq!(serviced.transmitted, TRANSMIT_BURST);
        assert_eq!(channel.transmit.len(), 20 - TRANSMIT_BURST);
        assert_eq!(device.IMSC.get(), INT_RX | INT_RT | INT_TX);
    }

    #[test]
    fn service_waits_for_room_in_a_full_transmit_fifo() {
        let (device, uart) = device();
        let channel = SerialChannel::new();
        uart.configure(false);
        assert!(channel.transmit.push(7));
        device.FR.set(FR_RXFE | FR_TXFF);
        let serviced = uart.service(&channel);
        assert_eq!(serviced, Serviced::default());
        assert!(!serviced.moved_bytes());
        assert_eq!(channel.transmit.len(), 1);
        assert_eq!(device.IMSC.get() & INT_TX, INT_TX);
    }
}

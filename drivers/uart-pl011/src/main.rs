// Host tests (`host-tests`) build this binary natively too; it is only an
// empty `main` there.
#![cfg_attr(target_os = "vesper", no_std, no_main)]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(all(target_os = "vesper", feature = "qemu"), feature(format_args_nl))]

//! EL0 driver component for the Arm PL011 UART.
//!
//! Kickstart loads one instance per enabled `arm,pl011` node (see
//! `libdevice-driver`). The instance runs two Threads:
//!
//! - the interrupt Thread, at the component's entry, maps the register page
//!   and, if a client attached a serial channel (see `libserial-channel`),
//!   the channel's control page and data area. It brings the UART up,
//!   loopback if the channel asks for it, and then serves the interrupts:
//!   each wakeup on its Notification moves received bytes into the client's
//!   receive buffers, advances the client's receive completion count, tells
//!   the transmit Thread when the transmit FIFO drained, and acks the
//!   interrupts it was woken for;
//! - the transmit Thread waits for the interrupt Thread to finish that setup,
//!   then for the client's transmit packets, writes each to the transmit
//!   FIFO one burst at a time, and completes it back.
//!
//! The interrupt Notification stays inside the driver: the client only ever
//! sees the channel's `EventCount`s. The two Threads share the driver's own
//! `EventCount` (`DriverInit::event_count`): its first advance says the
//! setup is done, and every later one that the transmit FIFO drained.
//!
//! Without a client the interrupt Thread serves a private channel nobody
//! reads: the UART stays quiet, received bytes are dropped, and the transmit
//! Thread waits for good.

#[cfg(target_os = "vesper")]
mod component {
    use {
        libdevice_driver::{
            DEVICE_WINDOW_VA, DriverInit, MAX_DEVICE_PAGES, device_address, driver_compatible,
            driver_init, driver_threads, local_key, map_device_pages,
        },
        libio_channel::{Cursor, Event, NoEvent, Packet, WaitError},
        libobject::{
            CapError, EventCountKey, FrameKey, InvalidKeyReason, IrqHandlerKey, NotificationKey,
            RawKey, Rights,
        },
        libserial_channel::{
            CONTROL_FRAME_SLOT, CONTROL_VA, DATA_AREA, DATA_AREA_VA, DATA_FRAME_SLOT, DATA_PAGES,
            LOOPBACK, RECEIVE_COMPLETED_SLOT, SerialChannel, TRANSMIT_COMPLETED_SLOT,
            TRANSMIT_SUBMITTED_SLOT,
        },
        libuser::semihosting as semi,
        uart_pl011::{Pl011, Receiver, RegisterBlock, TRANSMIT_BURST},
    };

    libuser::entry!(main);

    driver_compatible!("arm,pl011");

    driver_threads!(transmit_main);

    /// Span of the device window: one L3 table.
    const DEVICE_WINDOW_BYTES: u64 = 2 << 20;

    // The channel lies in the device window, above every register page.
    const _: () = assert!(CONTROL_VA >= DEVICE_WINDOW_VA + MAX_DEVICE_PAGES as u64 * 4096);
    const _: () = assert!(DATA_AREA.base + DATA_AREA.len <= DEVICE_WINDOW_VA + DEVICE_WINDOW_BYTES);

    /// The channel of a driver nobody attached to.
    static PRIVATE_CHANNEL: SerialChannel = SerialChannel::new();

    /// The physical address of the UART's registers.
    fn registers_paddr(init: &DriverInit) -> u64 {
        init.regions()
            .first()
            .unwrap_or_else(|| panic!("uart-pl011: no register range"))
            .start
    }

    /// The UART, once `map_device_pages` mapped its registers.
    fn uart(init: &DriverInit) -> Pl011<'static> {
        let registers = device_address(init, registers_paddr(init)).expect("registers mapped");
        // SAFETY: the interrupt Thread's `map_device_pages` mapped the
        // node's register page read/write as device memory at `registers`
        // for the driver's lifetime; the two Threads touch disjoint
        // registers once the UART is configured.
        Pl011::new(unsafe { &*(registers as *const RegisterBlock) })
    }

    /// The table's key at `slot`, as an `EventCount`.
    fn event_count(init: &DriverInit, slot: u32) -> EventCountKey {
        EventCountKey::from_key(local_key(init.table, slot))
    }

    /// Map the client's control page at [`CONTROL_VA`] and its data area at
    /// [`DATA_AREA_VA`]; whether a client attached.
    fn attach(init: &DriverInit) -> bool {
        let address_space = RawKey::from_wire(init.address_space);
        let map = |slot: u32, address: u64| {
            FrameKey::from_key(local_key(init.table, slot)).map(
                address_space,
                address,
                Rights(Rights::READ | Rights::WRITE),
                0,
            )
        };
        match map(CONTROL_FRAME_SLOT, CONTROL_VA) {
            Ok(()) => {}
            // Nobody ever installed a key there.
            Err(
                CapError::EmptySlot(_)
                | CapError::InvalidKey {
                    reason: InvalidKeyReason::NeverIssued,
                    ..
                },
            ) => return false,
            Err(error) => panic!("uart-pl011: mapping the channel failed: {:?}", error.code()),
        }
        for page in 0..DATA_PAGES {
            map(
                DATA_FRAME_SLOT + page,
                DATA_AREA_VA + u64::from(page) * 4096,
            )
            .unwrap_or_else(|error| {
                panic!(
                    "uart-pl011: mapping the data area failed: {:?}",
                    error.code()
                )
            });
        }
        true
    }

    fn main(init_va: u64) -> ! {
        // SAFETY: Kickstart passes the init page's address in x0.
        let init = unsafe { driver_init(init_va) };
        map_device_pages(init).unwrap_or_else(|error| {
            panic!("uart-pl011: mapping registers failed: {:?}", error.code())
        });
        let uart = uart(init);
        assert!(uart.is_pl011(), "uart-pl011: not a PL011");

        let attached = attach(init);
        let channel = if attached {
            // SAFETY: `attach` mapped the client's read/write page at
            // CONTROL_VA for the driver's lifetime; a fresh one is zeroed.
            unsafe { SerialChannel::at(CONTROL_VA) }
        } else {
            &PRIVATE_CHANNEL
        };
        let loopback = channel.has(LOOPBACK);
        uart.configure(loopback);
        semi::println!(
            "uart-pl011: {:#x} up{}{}",
            registers_paddr(init),
            if attached { ", client attached" } else { "" },
            if loopback { ", loopback" } else { "" }
        );

        if !attached {
            // SAFETY: the private channel names no buffers, so the receiver
            // never touches the data area.
            let receiver = unsafe { Receiver::new(channel, NoEvent, DATA_AREA) };
            serve_interrupts(init, &uart, receiver, None)
        }
        let completed = event_count(init, RECEIVE_COMPLETED_SLOT);
        // SAFETY: `attach` mapped the data area read/write for the driver's
        // lifetime.
        let receiver = unsafe { Receiver::new(channel, completed, DATA_AREA) };
        let transmit_room = EventCountKey::from_key(RawKey::from_wire(init.event_count));
        // The setup is done: let the transmit Thread in.
        transmit_room
            .advance(1)
            .unwrap_or_else(|error| panic!("uart-pl011: Advance failed: {:?}", error.code()));
        serve_interrupts(init, &uart, receiver, Some(&transmit_room))
    }

    /// The interrupt Thread's loop: on each wakeup, receive into
    /// `receiver`'s buffers, tell the transmit Thread through
    /// `transmit_room` when the transmit FIFO drained, and ack the
    /// interrupts that woke it.
    fn serve_interrupts<Completed: Event>(
        init: &DriverInit,
        uart: &Pl011<'_>,
        mut receiver: Receiver<'_, Completed>,
        transmit_room: Option<&EventCountKey>,
    ) -> ! {
        let handlers = || {
            init.interrupts().iter().map(|interrupt| {
                (
                    IrqHandlerKey::from_key(RawKey::from_wire(interrupt.handler)),
                    interrupt.bits,
                )
            })
        };
        // A newly issued line stays masked until its first Ack.
        for (handler, _bits) in handlers() {
            handler
                .ack()
                .unwrap_or_else(|error| panic!("uart-pl011: Ack failed: {:?}", error.code()));
        }

        let wakeups = NotificationKey::from_key(RawKey::from_wire(init.notification));
        loop {
            let woken = wakeups
                .wait(NotificationKey::WAIT_INFINITE)
                .unwrap_or_else(|error| panic!("uart-pl011: Wait failed: {:?}", error.code()));
            let interrupts = uart.take_interrupts();
            // Drain the receive FIFO before the transmit Thread sends more:
            // in loopback its next burst lands there.
            uart.receive(&mut receiver);
            // Only a client's EventCount can fail to advance.
            assert!(
                receiver.signal().is_ok(),
                "uart-pl011: receive Advance failed"
            );
            if interrupts.transmit
                && let Some(room) = transmit_room
            {
                room.advance(1).unwrap_or_else(|error| {
                    panic!("uart-pl011: transmit Advance failed: {:?}", error.code())
                });
            }
            // The interrupts that woke us are serviced: let them in again.
            for (handler, bits) in handlers() {
                if woken & bits != 0 {
                    handler.ack().unwrap_or_else(|error| {
                        panic!("uart-pl011: Ack failed: {:?}", error.code())
                    });
                }
            }
        }
    }

    /// The transmit FIFO's room, as the interrupt Thread reports it by
    /// advancing the driver's `EventCount` on each transmit interrupt.
    struct TransmitRoom {
        count: EventCountKey,
        /// The count that says the last full burst drained, if the
        /// transmit Thread has not waited for it yet
        awaiting: Option<u64>,
    }

    impl TransmitRoom {
        fn read(&self) -> u64 {
            self.count
                .read()
                .unwrap_or_else(|error| panic!("uart-pl011: Read failed: {:?}", error.code()))
        }

        /// Wait for the last full burst to drain.
        fn wait(&mut self) {
            if let Some(target) = self.awaiting.take() {
                self.count
                    .await_ge(target, EventCountKey::WAIT_INFINITE)
                    .unwrap_or_else(|error| panic!("uart-pl011: Await failed: {:?}", error.code()));
            }
        }
    }

    /// Send every byte of `packet`, which lies in the mapped data area, one
    /// burst at a time.
    ///
    /// Each burst waits for the one before to drain, even across packets:
    /// in loopback, the interrupt Thread empties the receive FIFO before it
    /// reports the room, so the FIFO never holds more than one burst. A
    /// burst short of [`TRANSMIT_BURST`] that ends the packet may never
    /// drain through the FIFO's trigger level, so nothing waits for it.
    fn send(uart: &Pl011<'_>, packet: &Packet, room: &mut TransmitRoom) {
        let mut cursor = Cursor::START;
        while !cursor.is_at_end(packet) {
            room.wait();
            let before = room.read();
            let written = uart.transmit(|| {
                cursor.next_address(packet).map(|address| {
                    // SAFETY: the data area holds the packet and is mapped
                    // (`attach`); the client gave the buffer to the driver
                    // until it comes back.
                    unsafe { (address as *const u8).read_volatile() }
                })
            });
            if written == TRANSMIT_BURST || !cursor.is_at_end(packet) {
                room.awaiting = Some(before + 1);
            }
        }
    }

    /// The transmit Thread: once the interrupt Thread advanced the driver's
    /// `EventCount` the first time, serve the client's transmit packets.
    fn transmit_main(init_va: u64) -> ! {
        // SAFETY: Kickstart passes the init page's address in x0.
        let init = unsafe { driver_init(init_va) };
        let mut room = TransmitRoom {
            count: EventCountKey::from_key(RawKey::from_wire(init.event_count)),
            awaiting: Some(1),
        };
        room.wait();
        let uart = uart(init);
        // SAFETY: the interrupt Thread mapped the client's channel at
        // CONTROL_VA before its first advance.
        let channel = unsafe { SerialChannel::at(CONTROL_VA) };
        let (mut requests, mut completions) = channel.transmit.server(
            event_count(init, TRANSMIT_SUBMITTED_SLOT),
            event_count(init, TRANSMIT_COMPLETED_SLOT),
        );
        loop {
            let mut packet = match requests.wait() {
                Ok(packet) => packet,
                // The client garbled its FIFO; what it published is gone.
                Err(WaitError::Corrupt) => continue,
                Err(WaitError::Event(error)) => {
                    panic!("uart-pl011: transmit Await failed: {:?}", error.code())
                }
            };
            if DATA_AREA.holds(&packet) {
                send(&uart, &packet, &mut room);
            } else {
                // A packet outside the data area comes back with nothing sent.
                Cursor::START.cut(&mut packet);
            }
            // A client with more packets out than the FIFO holds loses the
            // extra ones.
            let _lost = completions.put(&packet);
            completions.signal().unwrap_or_else(|error| {
                panic!("uart-pl011: transmit Advance failed: {:?}", error.code())
            });
        }
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}

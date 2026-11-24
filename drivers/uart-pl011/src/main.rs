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
//! Both Threads run at input priority (`libdevice_driver::Priority::INPUT`),
//! ahead of CPU-bound programs: a burst drains in 40 us at 2 Mbaud, and at
//! normal priority each transmit interrupt waited behind the running
//! program's slice, sending one burst per scheduling round.
//!
//! Without a client the interrupt Thread serves a private channel nobody
//! reads: the UART stays quiet, received bytes are dropped, and the transmit
//! Thread waits for good.

#[cfg(target_os = "vesper")]
mod component {
    use {
        core::sync::atomic::{AtomicU64, Ordering},
        libdevice_driver::{
            DEVICE_WINDOW_VA, DriverInit, MAX_DEVICE_PAGES, Priority, device_address,
            driver_compatible, driver_init, driver_threads, entry_priority, local_key,
            map_device_pages,
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
        uart_pl011::{
            Pl011, Receiver, RegisterBlock, TRANSMIT_BURST, id_names_pl011, loopback_burst,
        },
    };

    libuser::entry!(main);

    driver_compatible!("arm,pl011");

    entry_priority!(Priority::INPUT);
    driver_threads!(transmit_main => Priority::INPUT);

    /// Span of the device window: one L3 table.
    const DEVICE_WINDOW_BYTES: u64 = 2 << 20;

    // The channel lies in the device window, above every register page.
    const _: () = assert!(CONTROL_VA >= DEVICE_WINDOW_VA + MAX_DEVICE_PAGES as u64 * 4096);
    const _: () = assert!(DATA_AREA.base + DATA_AREA.len <= DEVICE_WINDOW_VA + DEVICE_WINDOW_BYTES);

    /// The channel of a driver nobody attached to.
    static PRIVATE_CHANNEL: SerialChannel = SerialChannel::new();

    /// Bytes the interrupt Thread took from the receive FIFO, stored or
    /// dropped: in loopback, what of the sent bytes came back.
    static RECEIVED: AtomicU64 = AtomicU64::new(0);

    /// Bytes left in the receive buffer the interrupt Thread is filling, as
    /// of its last pass; published before [`RECEIVED`] moves.
    static FILLING_ROOM: AtomicU64 = AtomicU64::new(0);

    /// How long the transmit Thread naps, in loopback, while the client has
    /// no receive room for the next burst: the client queues its buffers
    /// without a signal.
    const LOOPBACK_POLL_NS: u64 = 100_000;

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
        // for the driver's lifetime. Each Thread makes its own handle; once
        // the UART is configured they touch disjoint registers, except the
        // data register, where the interrupt Thread's reads pop the receive
        // FIFO and the transmit Thread's writes push the transmit FIFO.
        Pl011::new(unsafe { RegisterBlock::new_mmio(registers as *mut RegisterBlock) })
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
                libobject::FrameAttribute::Default,
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
        let mut uart = uart(init);
        // A node may state the ID instead (`arm,primecell-periphid`): the
        // Raspberry Pi 5's uart10 implements only the first 0x200 bytes of its
        // page, so its ID registers at 0xFE0 read back something else.
        let is_pl011 = match u32::try_from(init.primecell_id) {
            Ok(0) => uart.is_pl011(),
            Ok(stated) => id_names_pl011(stated),
            Err(_) => false,
        };
        assert!(
            is_pl011,
            "uart-pl011: not a PL011 (stated peripheral ID {:#x})",
            init.primecell_id
        );

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
            serve_interrupts(init, &mut uart, receiver, None)
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
        serve_interrupts(init, &mut uart, receiver, Some(&transmit_room))
    }

    /// The interrupt Thread's loop: on each wakeup, receive into
    /// `receiver`'s buffers, tell the transmit Thread through
    /// `transmit_room` when the transmit FIFO drained, and ack the
    /// interrupts that woke it.
    fn serve_interrupts<Completed: Event>(
        init: &DriverInit,
        uart: &mut Pl011<'_>,
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
                .await_bits(NotificationKey::WAIT_INFINITE)
                .unwrap_or_else(|error| panic!("uart-pl011: Wait failed: {:?}", error.code()));
            let interrupts = uart.take_interrupts();
            // Drain the receive FIFO before the transmit Thread sends more:
            // in loopback its next burst lands there.
            let pass = uart.receive(&mut receiver);
            FILLING_ROOM.store(receiver.filling_room(), Ordering::Relaxed);
            RECEIVED.fetch_add((pass.stored + pass.dropped) as u64, Ordering::Release);
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

    /// How long the transmit Thread waits for a burst to drain before it
    /// sends on without the transmit interrupt: a burst takes 40 us at
    /// 2 Mbaud and 0.7 ms at 115 200 baud. Sending early is safe, since
    /// [`Pl011::transmit`] stops at a full FIFO; the wait only paces the
    /// bursts. Without the bound a transmit interrupt served late or never
    /// stopped the output for good.
    const ROOM_TIMEOUT_NS: u64 = 10_000_000;

    /// The transmit FIFO's room, as the interrupt Thread reports it by
    /// advancing the driver's `EventCount` on each transmit interrupt.
    struct TransmitRoom {
        count: EventCountKey,
        /// The count that says the last full burst drained, if the
        /// transmit Thread has not waited for it yet
        awaiting: Option<u64>,
        /// Bursts sent on after [`ROOM_TIMEOUT_NS`] without their transmit
        /// interrupt
        missed: u64,
        /// Bytes written to the transmit FIFO
        sent: u64,
    }

    impl TransmitRoom {
        fn read(&self) -> u64 {
            self.count
                .read()
                .unwrap_or_else(|error| panic!("uart-pl011: Read failed: {:?}", error.code()))
        }

        /// Wait for the last full burst to drain, at most
        /// [`ROOM_TIMEOUT_NS`].
        fn wait(&mut self) {
            let Some(target) = self.awaiting.take() else {
                return;
            };
            match self.count.await_ge(target, ROOM_TIMEOUT_NS) {
                Ok(_) => {}
                Err(CapError::TimedOut { .. }) => {
                    self.missed += 1;
                    // On the semihosting console only: a line on the UART
                    // would land in the middle of the client's.
                    semi::println!(
                        "uart-pl011: sent on without a transmit interrupt ({} so far)",
                        self.missed
                    );
                }
                Err(error) => panic!("uart-pl011: Await failed: {:?}", error.code()),
            }
        }

        /// In loopback, wait until a burst can come back whole and return its
        /// size ([`loopback_burst`]), napping so the client gets the CPU to
        /// queue receive buffers. An advance of the count ends a nap early,
        /// which only means looking again.
        fn wait_for_loopback_room(&self, channel: &SerialChannel) -> usize {
            loop {
                let received = RECEIVED.load(Ordering::Acquire);
                let filling_room = FILLING_ROOM.load(Ordering::Relaxed);
                let burst = loopback_burst(
                    self.sent.saturating_sub(received),
                    filling_room,
                    channel.receive.submit.queued_slots(),
                );
                if burst > 0 {
                    return burst;
                }
                match self.count.await_ge(u64::MAX, LOOPBACK_POLL_NS) {
                    Ok(_) | Err(CapError::TimedOut { .. }) => {}
                    Err(error) => panic!("uart-pl011: Await failed: {:?}", error.code()),
                }
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
    ///
    /// In loopback (`loopback` is the channel) a burst instead waits until
    /// every byte sent came back, which also means the FIFO drained, and is
    /// no larger than the receive buffers can take
    /// ([`TransmitRoom::wait_for_loopback_room`]).
    fn send(
        uart: &mut Pl011<'_>,
        packet: &Packet,
        room: &mut TransmitRoom,
        loopback: Option<&SerialChannel>,
    ) {
        let mut cursor = Cursor::START;
        while !cursor.is_at_end(packet) {
            let limit = match loopback {
                Some(channel) => room.wait_for_loopback_room(channel),
                None => {
                    room.wait();
                    TRANSMIT_BURST
                }
            };
            let before = room.read();
            let mut taken = 0;
            let written = uart.transmit(|| {
                if taken == limit {
                    return None;
                }
                taken += 1;
                cursor.next_address(packet).map(|address| {
                    // SAFETY: the data area holds the packet and is mapped
                    // (`attach`); the client gave the buffer to the driver
                    // until it comes back.
                    unsafe { (address as *const u8).read_volatile() }
                })
            });
            room.sent += written as u64;
            if loopback.is_none() && (written == TRANSMIT_BURST || !cursor.is_at_end(packet)) {
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
            awaiting: None,
            missed: 0,
            sent: 0,
        };
        // The setup's advance: the interrupt Thread brings the UART up first.
        room.count
            .await_ge(1, EventCountKey::WAIT_INFINITE)
            .unwrap_or_else(|error| panic!("uart-pl011: Await failed: {:?}", error.code()));
        let mut uart = uart(init);
        // SAFETY: the interrupt Thread mapped the client's channel at
        // CONTROL_VA before its first advance.
        let channel = unsafe { SerialChannel::at(CONTROL_VA) };
        let (mut requests, mut completions) = channel.transmit.server(
            event_count(init, TRANSMIT_SUBMITTED_SLOT),
            event_count(init, TRANSMIT_COMPLETED_SLOT),
        );
        let loopback = channel.has(LOOPBACK).then_some(channel);
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
                send(&mut uart, &packet, &mut room, loopback);
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

// Host tests (`host-tests`) build this binary natively too; it is only an
// empty `main` there.
#![cfg_attr(target_os = "vesper", no_std, no_main)]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(all(target_os = "vesper", feature = "qemu"), feature(format_args_nl))]

//! EL0 driver component for the Arm PL011 UART.
//!
//! Kickstart loads one instance per enabled `arm,pl011` node (see
//! `libdevice-driver`). The instance maps its register page, and, if a
//! client attached a serial channel (see `libserial-channel`), maps the
//! channel page at [`CHANNEL_VA`]. It brings the UART up, loopback if the
//! channel asks for it, and then serves the channel: each wakeup on its
//! Notification, an interrupt or the client's doorbell, moves bytes between
//! the FIFOs and the channel's rings, signals the client when it moved any,
//! and acks the interrupts it was woken for.
//!
//! Without a client the driver serves a private channel nobody reads: the
//! UART stays quiet and received bytes are dropped.

#[cfg(target_os = "vesper")]
mod component {
    use {
        libdevice_driver::{
            DEVICE_WINDOW_VA, DriverInit, MAX_DEVICE_PAGES, device_address, driver_compatible,
            driver_init, local_key, map_device_pages,
        },
        libobject::{
            CapError, FrameKey, InvalidKeyReason, IrqHandlerKey, NotificationKey, RawKey, Rights,
        },
        libserial_channel::{CHANNEL_FRAME_SLOT, CLIENT_SLOT, LOOPBACK, SerialChannel},
        libuser::semihosting as semi,
        uart_pl011::{Pl011, RegisterBlock},
    };

    libuser::entry!(main);

    driver_compatible!("arm,pl011");

    /// Where the driver maps a client's channel page: in its device window,
    /// above every register page.
    pub const CHANNEL_VA: u64 = DEVICE_WINDOW_VA + MAX_DEVICE_PAGES as u64 * 4096;

    /// The channel of a driver nobody attached to.
    static PRIVATE_CHANNEL: SerialChannel = SerialChannel::new();

    /// The client's channel, mapped from [`CHANNEL_FRAME_SLOT`], and its
    /// Notification; the private channel if no client attached.
    fn attach(init: &DriverInit) -> (&'static SerialChannel, Option<NotificationKey>) {
        let frame = FrameKey::from_key(local_key(init.table, CHANNEL_FRAME_SLOT));
        let address_space = RawKey::from_wire(init.address_space);
        match frame.map(
            address_space,
            CHANNEL_VA,
            Rights(Rights::READ | Rights::WRITE),
            0,
        ) {
            // SAFETY: the client's read/write page is mapped at CHANNEL_VA
            // for the driver's lifetime; a fresh one is zeroed.
            Ok(()) => (
                unsafe { SerialChannel::at(CHANNEL_VA) },
                Some(NotificationKey::from_key(local_key(
                    init.table,
                    CLIENT_SLOT,
                ))),
            ),
            // Nobody ever installed a key there.
            Err(
                CapError::EmptySlot(_)
                | CapError::InvalidKey {
                    reason: InvalidKeyReason::NeverIssued,
                    ..
                },
            ) => (&PRIVATE_CHANNEL, None),
            Err(error) => panic!("uart-pl011: mapping the channel failed: {:?}", error.code()),
        }
    }

    fn main(init_va: u64) -> ! {
        // SAFETY: Kickstart passes the init page's address in x0.
        let init = unsafe { driver_init(init_va) };
        let registers_paddr = init
            .regions()
            .first()
            .unwrap_or_else(|| panic!("uart-pl011: no register range"))
            .start;
        map_device_pages(init).unwrap_or_else(|error| {
            panic!("uart-pl011: mapping registers failed: {:?}", error.code())
        });
        let registers = device_address(init, registers_paddr).expect("registers mapped");
        // SAFETY: `map_device_pages` mapped the node's register page
        // read/write as device memory at `registers` for the driver's
        // lifetime; only this Thread touches it.
        let uart = Pl011::new(unsafe { &*(registers as *const RegisterBlock) });
        assert!(uart.is_pl011(), "uart-pl011: not a PL011");

        let (channel, client) = attach(init);
        let loopback = channel.has(LOOPBACK);
        uart.configure(loopback);
        semi::println!(
            "uart-pl011: {:#x} up{}{}",
            registers_paddr,
            if client.is_some() {
                ", client attached"
            } else {
                ""
            },
            if loopback { ", loopback" } else { "" }
        );

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
            let serviced = uart.service(channel);
            if serviced.moved_bytes()
                && let Some(client) = &client
            {
                client.signal(0).unwrap_or_else(|error| {
                    panic!("uart-pl011: client Signal failed: {:?}", error.code())
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
}

#[cfg(not(target_os = "vesper"))]
fn main() {}

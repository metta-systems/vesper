#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! driver-test's probe driver (see `driver-protocol`). It checks what its
//! init page hands it: it maps its register pages, reads an identifying
//! register, acks each interrupt, carves an object from its own memory, and
//! then reports on the key driver-test installed. Any failure panics, which
//! ends the QEMU run with a failure.

use {
    core::ptr::read_volatile,
    driver_protocol::{PL011, report_key},
    libdevice_driver::{device_address, driver_compatible, driver_init, map_device_pages},
    libobject::{
        IrqHandlerKey, KeyTableKey, NotificationKey, ObjectType, RawKey, Rights, UntypedKey,
    },
    libuser::semihosting as semi,
};

libuser::entry!(main);

driver_compatible!("arm,pl011", "brcm,bcm2835-aux", "brcm,bcm2835-aux-uart");

/// PL011 `UARTPeriphID0`: 0x11 on every PL011.
const PL011_PERIPH_ID0: u64 = 0xfe0;

fn main(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    assert!(!init.regions().is_empty(), "no register ranges");
    assert!(!init.pages().is_empty(), "no register pages");

    let next_slot = map_device_pages(init)
        .unwrap_or_else(|error| panic!("mapping registers failed: {:?}", error.code()));
    let base = device_address(init, init.regions()[0].start).expect("registers mapped");
    // SAFETY: `map_device_pages` mapped the node's register pages read/write
    // as device memory; the register is a 32-bit read without side effects.
    let first_word = unsafe {
        read_volatile(
            (base
                + if init.compatible_index == PL011 {
                    PL011_PERIPH_ID0
                } else {
                    0
                }) as *const u32,
        )
    };
    if init.compatible_index == PL011 {
        assert_eq!(first_word & 0xff, 0x11, "not a PL011");
    }

    for interrupt in init.interrupts() {
        IrqHandlerKey::from_key(RawKey::from_wire(interrupt.handler))
            .ack()
            .unwrap_or_else(|error| {
                panic!("Ack of line {} failed: {:?}", interrupt.line, error.code())
            });
    }

    let own_table = KeyTableKey::from_key(RawKey::from_wire(init.table));
    UntypedKey::from_key(RawKey::from_wire(init.memory))
        .retype(
            ObjectType::NOTIFICATION,
            0,
            0,
            1,
            &own_table,
            u32::try_from(next_slot).unwrap_or(u32::MAX),
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("Retype from own memory failed: {:?}", error.code()));

    semi::println!(
        "driver-probe: compatible {}, {} pages, {} interrupts, first register {first_word:#x}",
        init.compatible_index,
        init.pages().len(),
        init.interrupts().len()
    );
    NotificationKey::from_key(report_key(init.table))
        .signal(0)
        .unwrap_or_else(|error| panic!("report failed: {:?}", error.code()));
    let interrupts = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = interrupts.wait(NotificationKey::WAIT_INFINITE);
    }
}

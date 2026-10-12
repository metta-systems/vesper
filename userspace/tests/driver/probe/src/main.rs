#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! driver-test's probe driver (see `driver-protocol`). It checks what its
//! init page hands it: it maps its register pages, reads an identifying
//! register, acks each interrupt, carves an object from its own memory, and
//! then calls `echo_v1` through its imports and reports on the key
//! driver-test installed. It also exports `echo_v1` to the probe instances
//! whose nodes link to its node. Any failure panics, which
//! ends the QEMU run with a failure.

use {
    core::ptr::read_volatile,
    driver_echo::echo_v1::{EchoV1, Exception},
    driver_echo_client::echo_v1::{self, Bindings, Client, Error},
    driver_protocol::{AUX, AUX_UART, PL011, report_key},
    libdevice_driver::{INIT_VA, device_address, driver_compatible, driver_init, map_device_pages},
    libmeddle_runtime::ExclusiveStacks,
    libobject::{
        IrqHandlerKey, KeyTableKey, NotificationKey, ObjectType, RawKey, Rights, UntypedKey,
        export::init_return_key, thread::ThreadReturnKey,
    },
    libuser::semihosting as semi,
};

libuser::entry!(main);

driver_compatible!("arm,pl011", "brcm,bcm2835-aux", "brcm,bcm2835-aux-uart");

/// PL011 `UARTPeriphID0`: 0x11 on every PL011.
const PL011_PERIPH_ID0: u64 = 0xfe0;

/// The probe's `echo_v1`, called on the calling probe's Thread inside this
/// probe's `AddressSpace`.
struct Echo;

impl Echo {
    fn init() -> &'static libdevice_driver::DriverInit {
        // SAFETY: Kickstart maps every driver's init record at `INIT_VA`, and
        // an export runs in this driver's own `AddressSpace`.
        unsafe { driver_init(INIT_VA) }
    }
}

impl EchoV1 for Echo {
    fn enter() {
        init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
            Self::init().return_key,
        )));
    }

    fn identify() -> u64 {
        Self::init().compatible_index
    }

    fn twice(value: u64) -> Result<u64, Exception> {
        value.checked_mul(2).ok_or(Exception::Overflow { value })
    }
}

driver_echo::echo_v1_export!(Echo);

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

    // `uart1` links to `aux` through `clocks`; the other nodes' first clock is
    // the clock manager, which no probe serves.
    let echo = Client::new(
        Bindings::from_imports(init.imports()),
        // SAFETY: this Thread is the only one calling through the probe's
        // imports.
        unsafe { ExclusiveStacks::new() },
    );
    if init.compatible_index == AUX_UART {
        assert_eq!(echo.identify(), Ok(AUX), "uart1 calls the aux instance");
        assert_eq!(echo.twice(21), Ok(42));
        assert_eq!(
            echo.twice(u64::MAX),
            Err(Error::Exception(echo_v1::Exception::Overflow {
                value: u64::MAX
            }))
        );
    } else {
        assert_eq!(echo.identify(), Err(Error::NotGranted));
    }

    semi::println!(
        "driver-probe: compatible {}, {} pages, {} interrupts, {} imports, first register {first_word:#x}",
        init.compatible_index,
        init.pages().len(),
        init.interrupts().len(),
        init.imports().len()
    );
    NotificationKey::from_key(report_key(init.table))
        .signal(0)
        .unwrap_or_else(|error| panic!("report failed: {:?}", error.code()));
    let interrupts = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = interrupts.wait(NotificationKey::WAIT_INFINITE);
    }
}

#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! mailbox-test's client, loaded for the firmware power node. It calls the
//! mailbox component through the `mailbox_v1` import Kickstart wired along
//! `firmware` and `mboxes`, checks the firmware's answers, and reports on the key
//! mailbox-test installed (`driver_protocol::REPORT_SLOT`).

use {
    driver_protocol::report_key,
    libdevice_driver::{driver_compatible, driver_init},
    libmeddle_runtime::ExclusiveStacks,
    libobject::{NotificationKey, RawKey},
    mailbox_client::mailbox_v1::{Bindings, Client, Clock, Device},
};

libuser::entry!(main);

/// The revision code QEMU's firmware reports for the emulated board.
#[cfg(board_rpi3)]
const BOARD_REVISION: u32 = 0x00a0_2082;
#[cfg(board_rpi4)]
const BOARD_REVISION: u32 = 0x00b0_3115;

driver_compatible!("raspberrypi,bcm2835-power");

/// Report the firmware's answers (under QEMU, on the semihosting console).
#[cfg(feature = "qemu")]
fn report(revision: u32, serial: u64, uart_hz: u32, uart_on: bool) {
    use libuser::semihosting as semi;

    semi::println!(
        "mailbox-test-client: board revision {revision:#x}, serial {serial:#x}, UART clock {uart_hz} Hz, UART0 powered {uart_on}"
    );
}

#[cfg(not(feature = "qemu"))]
fn report(_revision: u32, _serial: u64, _uart_hz: u32, _uart_on: bool) {}

fn main(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    let mailbox = Client::new(
        Bindings::from_imports(init.imports()),
        // SAFETY: this Thread is the only one calling through the client's
        // imports.
        unsafe { ExclusiveStacks::new() },
    );
    let revision = mailbox
        .get_board_revision()
        .unwrap_or_else(|error| panic!("get_board_revision failed: {error:?}"));
    assert_eq!(
        revision, BOARD_REVISION,
        "the firmware reports the board's revision"
    );
    let serial = mailbox
        .get_board_serial()
        .unwrap_or_else(|error| panic!("get_board_serial failed: {error:?}"));
    let uart_hz = mailbox
        .get_clock_rate(Clock::Uart)
        .unwrap_or_else(|error| panic!("get_clock_rate failed: {error:?}"));
    assert_ne!(uart_hz, 0, "the UART clock runs");
    let uart_on = mailbox
        .get_power_state(Device::Uart0)
        .unwrap_or_else(|error| panic!("get_power_state failed: {error:?}"));
    report(revision, serial, uart_hz, uart_on);
    NotificationKey::from_key(report_key(init.table))
        .signal(0)
        .unwrap_or_else(|error| panic!("report failed: {:?}", error.code()));
    let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = park.wait(NotificationKey::WAIT_INFINITE);
    }
}

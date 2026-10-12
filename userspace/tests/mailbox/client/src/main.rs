#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! mailbox-test's client, loaded for the firmware power node. It calls the
//! mailbox component through the `mailbox_v1` import Kickstart wired along
//! `firmware` and `mboxes`, checks the answer, and reports on the key
//! mailbox-test installed (`driver_protocol::REPORT_SLOT`).

use {
    driver_protocol::report_key,
    libdevice_driver::{driver_compatible, driver_init},
    libmeddle_runtime::ExclusiveStacks,
    libobject::{NotificationKey, RawKey},
    libuser::semihosting as semi,
    mailbox_client::mailbox_v1::{Bindings, Client, Error, Exception},
};

libuser::entry!(main);

driver_compatible!("raspberrypi,bcm2835-power");

/// The mailbox component's answer while it serves no requests.
const NOT_SERVED: u32 = u32::MAX;

fn main(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    let mailbox = Client::new(
        Bindings::from_imports(init.imports()),
        // SAFETY: this Thread is the only one calling through the client's
        // imports.
        unsafe { ExclusiveStacks::new() },
    );
    let answer = mailbox.get_board_revision();
    assert_eq!(
        answer,
        Err(Error::Exception(Exception::FirmwareError {
            code: NOT_SERVED
        })),
        "the mailbox component answers through the firmware link"
    );
    semi::println!("mailbox-test-client: get_board_revision reached the mailbox component");
    NotificationKey::from_key(report_key(init.table))
        .signal(0)
        .unwrap_or_else(|error| panic!("report failed: {:?}", error.code()));
    let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = park.wait(NotificationKey::WAIT_INFINITE);
    }
}

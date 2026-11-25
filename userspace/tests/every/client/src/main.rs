#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! every-test's client: it imports `echo_v1 via every max 2`, so Kickstart
//! links `count` once to each of the first two of the three loaded exporters.
//!
//! It checks that it got two instances, each with a data area of its own,
//! and that they are different instances: instance 0 is called twice and
//! instance 1 once, so they answer 1, 2 and 1. An instance refuses a
//! reference into the other instance's area. Then it reports on the key
//! every-test installed. Any failure panics, which ends the QEMU run.

use {
    driver_protocol::report_key,
    echo::echo_v1::Exception,
    echo_client::echo_v1::{Bindings, Client, Error},
    libdevice_driver::driver_init,
    libobject::{NotificationKey, RawKey},
    libuser::semihosting as semi,
};

libuser::entry!(main);

fn main(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    let imports = init.imports();
    assert_eq!(
        Bindings::instances(imports),
        2,
        "the first two of the three exporters"
    );
    let first = Client::new(Bindings::from_imports_nth(imports, 0));
    let second = Client::new(Bindings::from_imports_nth(imports, 1));
    let first_area = first.data_area().expect("instance 0 shares an area");
    let second_area = second.data_area().expect("instance 1 shares an area");
    assert_ne!(
        first_area.base, second_area.base,
        "each instance has an area of its own"
    );
    let note =
        |area: &libmeddle_runtime::DataArea| area.reference(0, 8).expect("a note inside the area");

    assert_eq!(first.count(note(&first_area)), Ok(1));
    assert_eq!(first.count(note(&first_area)), Ok(2));
    assert_eq!(
        second.count(note(&second_area)),
        Ok(1),
        "instance 1 is another exporter, with a count of its own"
    );
    // An instance checks the reference against its own pair's area.
    assert_eq!(
        second.count(note(&first_area)),
        Err(Error::Exception(Exception::NotPermitted)),
        "instance 0's area is not instance 1's"
    );
    // Beyond the linked instances there are no bindings.
    let third = Client::new(Bindings::from_imports_nth(imports, 2));
    assert_eq!(
        third.count(note(&first_area)),
        Err(Error::NotGranted),
        "no third instance"
    );
    semi::println!(
        "every-client: 2 instances, areas {:#x} and {:#x}; counts 1, 2 and 1",
        first_area.base,
        second_area.base
    );

    NotificationKey::from_key(report_key(init.table))
        .signal(0)
        .unwrap_or_else(|error| panic!("report failed: {:?}", error.code()));
    let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
    }
}

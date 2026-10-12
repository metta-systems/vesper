#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! mailbox-test's client, loaded for the vcsm node. It calls the
//! mailbox component through the `mailbox_v1` import Kickstart wired along
//! `firmware` and `mboxes`, checks the firmware's answers, and reports on the key
//! mailbox-test installed (`driver_protocol::REPORT_SLOT`).

use {
    driver_protocol::report_key,
    fb_client::fb_v1::{
        Bindings as FbBindings, Client as FbClient, Error as FbError, Exception as FbException,
    },
    libdevice_driver::{driver_compatible, driver_init},
    libmeddle_runtime::{ExclusiveStacks, IoRec},
    libobject::{NotificationKey, RawKey},
    mailbox_client::mailbox_v1::{Bindings, Client, Clock, Device, Error, Exception},
    mailbox_impl::tag,
    power_client::power_v1::{
        Bindings as PowerBindings, Client as PowerClient, Error as PowerError,
    },
};

libuser::entry!(main);

/// The revision code QEMU's firmware reports for the emulated board.
#[cfg(board_rpi3)]
const BOARD_REVISION: u32 = 0x00a0_2082;
#[cfg(board_rpi4)]
const BOARD_REVISION: u32 = 0x00b0_3115;

driver_compatible!("raspberrypi,bcm2835-vcsm");

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

/// A request of one tag with `value_words` zeroed value words: header, tag,
/// values and end tag.
fn request(id: u32, value_words: usize) -> ([u32; 16], usize) {
    let mut words = [0_u32; 16];
    words[2] = id;
    words[3] = u32::try_from(value_words * 4).unwrap_or(0);
    let length = 6 + value_words;
    words[0] = u32::try_from(length * 4).unwrap_or(0);
    (words, length)
}

/// The property buffer checks, through the shared data area: a real request
/// is answered in place; buffers outside the area, malformed buffers and
/// refused tags are not permitted and leave the mailbox usable.
fn check_property(mailbox: &Client) {
    let area = mailbox
        .data_area()
        .expect("the client shares a data area with the mailbox");
    assert_eq!(area.len, 4096);
    // Offset 0x40, to see the offset honoured.
    let offset = 0x40;

    let (words, length) = request(tag::GET_ARM_MEMORY, 2);
    assert!(area.write_words(offset, &words[..length]));
    let buffer = area
        .reference(offset, u64::try_from(length * 4).unwrap_or(0))
        .expect("the buffer lies in the area");
    mailbox
        .property(buffer)
        .unwrap_or_else(|error| panic!("property GetArmMemory failed: {error:?}"));
    let mut answer = [0_u32; 16];
    assert!(area.read_words(offset, &mut answer[..length]));
    assert_eq!(answer[1], 0x8000_0000, "the firmware answered the buffer");
    assert_ne!(answer[4] & 0x8000_0000, 0, "the firmware answered the tag");
    assert_eq!(answer[5], 0, "ARM memory starts at zero");
    assert_ne!(answer[6], 0, "ARM memory has a size");

    // Refused tags and malformed buffers.
    for (refused, name) in [
        (request(tag::EXECUTE_CODE, 2), "ExecuteCode"),
        (request(tag::ALLOCATE_MEMORY, 3), "AllocateMemory"),
        (request(tag::SET_CLOCK_RATE, 3), "SetClockRate"),
        (request(0x0001_00ff, 1), "an unknown tag"),
    ] {
        let (words, length) = refused;
        assert!(area.write_words(0, &words[..length]));
        let buffer = area
            .reference(0, u64::try_from(length * 4).unwrap_or(0))
            .unwrap();
        assert_eq!(
            mailbox.property(buffer),
            Err(Error::Exception(Exception::NotPermitted)),
            "{name} must be refused"
        );
    }
    let (mut words, length) = request(tag::GET_BOARD_REVISION, 1);
    words[0] += 4; // The header claims more bytes than the buffer has.
    assert!(area.write_words(0, &words[..length]));
    let buffer = area
        .reference(0, u64::try_from(length * 4).unwrap_or(0))
        .unwrap();
    assert_eq!(
        mailbox.property(buffer),
        Err(Error::Exception(Exception::NotPermitted))
    );

    // References that are not inside this client's data area.
    let (words, length) = request(tag::GET_BOARD_REVISION, 1);
    assert!(area.write_words(0, &words[..length]));
    let bytes = u64::try_from(length * 4).unwrap_or(0);
    for outside in [
        IoRec {
            base: 0,
            len: bytes,
        },
        IoRec {
            base: area.base - 0x1000,
            len: bytes,
        },
        IoRec {
            base: area.base + 0xff0,
            len: bytes,
        },
        IoRec {
            base: area.base + 0x1000,
            len: bytes,
        },
        IoRec {
            base: area.base + 2,
            len: bytes,
        },
        IoRec {
            base: u64::MAX - 8,
            len: 32,
        },
    ] {
        assert_eq!(
            mailbox.property(outside),
            Err(Error::Exception(Exception::NotPermitted)),
            "{outside:?}"
        );
    }

    // The mailbox still answers after every refusal.
    assert_eq!(mailbox.property(area.reference(0, bytes).unwrap()), Ok(()));
    assert!(area.read_words(0, &mut answer[..length]));
    assert_eq!(answer[5], BOARD_REVISION);
}

/// The power component: the client may power the devices off, but holds no
/// Invocation to halt or reset the board.
fn check_power(init: &libdevice_driver::DriverInit) {
    let power = PowerClient::new(
        PowerBindings::from_imports(init.imports()),
        // SAFETY: this Thread is the only one calling through the client's
        // imports.
        unsafe { ExclusiveStacks::new() },
    );
    power
        .devices_off()
        .unwrap_or_else(|error| panic!("devices_off failed: {error:?}"));
    assert_eq!(power.halt(), Err(PowerError::NotGranted));
    assert_eq!(power.reset(), Err(PowerError::NotGranted));
}

/// The fb component: nothing is configured at first; configuring a 640 by 480
/// display of 32 bits per pixel allocates a framebuffer.
fn check_fb(init: &libdevice_driver::DriverInit) {
    let fb = FbClient::new(
        FbBindings::from_imports(init.imports()),
        // SAFETY: as above.
        unsafe { ExclusiveStacks::new() },
    );
    assert_eq!(
        fb.address(),
        Err(FbError::Exception(FbException::NotConfigured))
    );
    let pitch = fb
        .configure(640, 480, 32)
        .unwrap_or_else(|error| panic!("configure failed: {error:?}"));
    assert_eq!(pitch, 640 * 4, "32 bits per pixel");
    let address = fb
        .address()
        .unwrap_or_else(|error| panic!("address failed: {error:?}"));
    let bytes = fb
        .size()
        .unwrap_or_else(|error| panic!("size failed: {error:?}"));
    assert_ne!(address, 0, "the framebuffer has an address");
    assert!(
        u64::from(bytes) >= u64::from(pitch) * 480,
        "it holds every row"
    );
    fb.pixel_order()
        .unwrap_or_else(|error| panic!("pixel_order failed: {error:?}"));
}

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
    check_property(&mailbox);
    check_power(init);
    check_fb(init);
    report(revision, serial, uart_hz, uart_on);
    NotificationKey::from_key(report_key(init.table))
        .signal(0)
        .unwrap_or_else(|error| panic!("report failed: {:?}", error.code()));
    let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = park.wait(NotificationKey::WAIT_INFINITE);
    }
}

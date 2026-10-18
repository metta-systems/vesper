#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! mailbox-test's client, loaded for the vcsm node. It calls the
//! mailbox component through the `mailbox_v1` import Kickstart wired along
//! `firmware` and `mboxes`, checks the firmware's answers, and reports on the key
//! mailbox-test installed (`driver_protocol::REPORT_SLOT`).

use {
    driver_protocol::{
        COPY_SLOT, FRAME_TABLE_BITS, FRAME_TABLE_GUARD, FRAME_TABLE_SLOT, lend_table_key,
        plain_table_key, report_key,
    },
    fb_client::fb_v1::{
        Bindings as FbBindings, Client as FbClient, Error as FbError, Exception as FbException,
    },
    libdevice_driver::{driver_init, driver_threads, local_key},
    libmeddle_runtime::{ExclusiveStacks, IoRec},
    libobject::{
        EventCountKey, FrameKey, KeySlot, KeyTableKey, NotificationKey, ObjectType, RawKey, Rights,
        UntypedKey,
    },
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

driver_threads!(background);

/// The client's second Thread: it reports in on the component's own
/// `EventCount`, which the first Thread waits for before it starts calling.
fn background(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    EventCountKey::from_key(RawKey::from_wire(init.event_count))
        .advance(1)
        .unwrap_or_else(|error| panic!("background advance failed: {:?}", error.code()));
    let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
    }
}

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

/// The base and size of the `VideoCore` memory, from the firmware.
fn vc_memory(mailbox: &Client) -> (u64, u64) {
    let area = mailbox.data_area().expect("a shared data area");
    let words = [32, 0, tag::GET_VC_MEMORY, 8, 0, 0, 0, 0];
    assert!(area.write_words(0, &words));
    mailbox
        .property(area.reference(0, 32).expect("inside the area"))
        .unwrap_or_else(|error| panic!("property GetVcMemory failed: {error:?}"));
    let mut answer = [0_u32; 8];
    assert!(area.read_words(0, &mut answer));
    assert_ne!(answer[4] & 0x8000_0000, 0, "the firmware answered the tag");
    (u64::from(answer[5]), u64::from(answer[6]))
}

/// Report where the framebuffer and the `VideoCore` memory are (under QEMU).
#[cfg(feature = "qemu")]
fn report_framebuffer(address: u64, bytes: u32, vc_base: u64, vc_size: u64) {
    use libuser::semihosting as semi;

    semi::println!(
        "mailbox-test-client: framebuffer {address:#x}+{bytes:#x}, VideoCore memory {vc_base:#x}+{vc_size:#x}"
    );
}

#[cfg(not(feature = "qemu"))]
fn report_framebuffer(_address: u64, _bytes: u32, _vc_base: u64, _vc_size: u64) {}

/// The fb component: nothing is configured at first; configuring a 640 by 480
/// display of 32 bits per pixel allocates a framebuffer.
fn check_fb(init: &libdevice_driver::DriverInit, mailbox: &Client) {
    let fb = FbClient::new(
        FbBindings::from_imports(init.imports()),
        // SAFETY: as above.
        unsafe { ExclusiveStacks::new() },
    );
    assert_eq!(
        fb.address(),
        Err(FbError::Exception(FbException::NotConfigured))
    );
    // There is nothing to hand out before the display is configured.
    assert_eq!(
        fb.frames(&KeyTableKey::from_key(lend_table_key(init.table)), 0),
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
    // The framebuffer lies in the `VideoCore` memory the firmware reports.
    let (vc_base, vc_size) = vc_memory(mailbox);
    report_framebuffer(address, bytes, vc_base, vc_size);
    assert!(
        vc_base <= address && address + u64::from(bytes) <= vc_base + vc_size,
        "the framebuffer lies in the VideoCore memory"
    );
    fb.pixel_order()
        .unwrap_or_else(|error| panic!("pixel_order failed: {error:?}"));
    check_frames(init, &fb, address, bytes);
}

/// The framebuffer's Frames, from the fb component into a table the client
/// carves for them and lends for the call (`CallWithKey`): fb carves the
/// Frames straight into the borrowed table. The Frames do not fit the
/// client's own table, which is why it lends another.
fn check_frames(init: &libdevice_driver::DriverInit, fb: &FbClient, address: u64, bytes: u32) {
    let own = KeyTableKey::from_key(RawKey::from_wire(init.table));
    let mut own_self = KeyTableKey::from_key(local_key(init.table, KeySlot::SELF_KEYTABLE.0));
    let memory = UntypedKey::from_key(RawKey::from_wire(init.memory));
    let table_with = |slot: u32, size_bits: u8, guard: u32| {
        KeyTableKey::from_key(
            memory
                .retype(
                    ObjectType::KEY_TABLE,
                    size_bits,
                    guard,
                    1,
                    &own,
                    slot,
                    Rights::all(),
                )
                .unwrap_or_else(|error| panic!("KeyTable Retype failed: {:?}", error.code())),
        )
    };
    let table = table_with(FRAME_TABLE_SLOT, FRAME_TABLE_BITS, FRAME_TABLE_GUARD);

    // A table key without `LEND` is refused before the call starts
    // (`InsufficientRights`, status 6).
    let plain = KeyTableKey::from_key(plain_table_key(init.table));
    assert_eq!(
        fb.frames(&plain, 0),
        Err(FbError::Call {
            status: 6,
            detail: [0, 0]
        })
    );
    // A table too small for the Frames fails the first install and hands
    // nothing out, so the real call below still works.
    let small = table_with(FRAME_TABLE_SLOT + 2, 1, 0x1234);
    assert_eq!(
        fb.frames(&small, 0),
        Err(FbError::Exception(FbException::InstallFailed {
            installed: 0
        }))
    );

    // The hand-over.
    let installed = fb
        .frames(&table, 0)
        .unwrap_or_else(|error| panic!("frames failed: {error:?}"));
    assert!(installed >= 1, "the framebuffer needs Frames");

    // The Frames cover the framebuffer's pages exactly, in address order, as
    // 4 KiB and 2 MiB Frames: copy each out of the table and read its extent.
    let start = address & !0xfff;
    let end = (address + u64::from(bytes)).next_multiple_of(0x1000);
    let mut cursor = start;
    for index in 0..installed {
        let source = RawKey::new(KeySlot((FRAME_TABLE_GUARD << FRAME_TABLE_BITS) | index), 1);
        let copy = table
            .copy_derive(source, &own, COPY_SLOT, Rights(Rights::GRANT))
            .unwrap_or_else(|error| panic!("Frame {index} missing: {:?}", error.code()));
        let (base, size) = FrameKey::from_key(copy)
            .get_extent()
            .unwrap_or_else(|error| panic!("Frame {index} has no extent: {:?}", error.code()));
        assert_eq!(base, cursor, "Frame {index} follows the one before");
        assert!(size == 4096 || size == 2 << 20, "Frame {index}: {size:#x}");
        cursor += size;
        own_self
            .delete(copy)
            .unwrap_or_else(|error| panic!("delete failed: {:?}", error.code()));
    }
    assert_eq!(cursor, end, "the Frames reach the end of the framebuffer");
    report_frames(installed, start, end);

    // The memory is handed out for good. This call also shows the window for
    // the lent key is free again after the first call returned.
    assert_eq!(
        fb.frames(&table, installed),
        Err(FbError::Exception(FbException::AlreadyHandedOut))
    );
}

/// Report what was handed out (under QEMU).
#[cfg(feature = "qemu")]
fn report_frames(frames: u32, start: u64, end: u64) {
    use libuser::semihosting as semi;

    semi::println!("mailbox-test-client: {frames} Frames cover {start:#x}..{end:#x}");
}

#[cfg(not(feature = "qemu"))]
fn report_frames(_frames: u32, _start: u64, _end: u64) {}

fn main(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    // The extra Thread started and ran: Kickstart loads a program's Threads
    // like a driver's.
    EventCountKey::from_key(RawKey::from_wire(init.event_count))
        .await_ge(1, EventCountKey::WAIT_INFINITE)
        .unwrap_or_else(|error| {
            panic!(
                "waiting for the background Thread failed: {:?}",
                error.code()
            )
        });
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
    // The LED, whichever way the board drives it: the call must succeed.
    for on in [true, false] {
        mailbox
            .set_led(on)
            .unwrap_or_else(|error| panic!("set_led({on}) failed: {error:?}"));
    }
    check_property(&mailbox);
    check_power(init);
    check_fb(init, &mailbox);
    report(revision, serial, uart_hz, uart_on);
    NotificationKey::from_key(report_key(init.table))
        .signal(0)
        .unwrap_or_else(|error| panic!("report failed: {:?}", error.code()));
    let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
    }
}

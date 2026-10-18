#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! usb-test's program. It calls the DWC2 host controller component through the
//! `usb_hcd_v1` import Kickstart wired along the controller's compatible
//! string. It resets the root port, enumerates the device behind it with
//! `vesper_usb_protocol` over a control pipe (the work `usbd` takes over in
//! USB3), checks that the root device is a hub, then checks the rules of pipes
//! and control stages, and reports on the key usb-test installed
//! (`driver_protocol::REPORT_SLOT`).

use {
    driver_protocol::report_key,
    libdevice_driver::{driver_init, driver_threads},
    libmeddle_runtime::{DataArea, ExclusiveStacks, IoRec},
    libobject::{EventCountKey, NotificationKey, RawKey},
    usb_hcd_client::usb_hcd_v1::{Bindings, Client, Error, Exception, Speed, TransferKind},
    vesper_usb_protocol::{
        descriptor::{self, Descriptor},
        enumeration::{Enumerator, Event, Failure, Speed as UsbSpeed, Step},
        setup::{DESCRIPTOR_CONFIGURATION, DESCRIPTOR_DEVICE, REQUEST_SET_ADDRESS, SetupPacket},
    },
};

libuser::entry!(main);

/// Address the root device is enumerated to.
const ROOT_ADDRESS: u8 = 1;
/// Hub device class code (USB 2.0 §11.23).
const CLASS_HUB: u8 = 9;
/// Largest reply the program reads: a configuration descriptor.
const REPLY_CAPACITY: usize = 256;
/// Pipes the controller keeps open at once.
const MAX_PIPES: usize = 16;

driver_threads!(background);

/// The program's second Thread: it reports in on the component's own
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

fn speed_of(speed: Speed) -> UsbSpeed {
    match speed {
        Speed::Low => UsbSpeed::Low,
        Speed::Full => UsbSpeed::Full,
        Speed::High => UsbSpeed::High,
    }
}

/// The host controller, and the data area it shares with this program.
struct Hcd<'a> {
    client: &'a Client,
    area: DataArea,
}

impl Hcd<'_> {
    /// Run `setup` on `pipe`, sending `out` as its data stage. A reply goes to
    /// `reply`; returns the bytes of it.
    fn control(
        &self,
        pipe: u32,
        setup: SetupPacket,
        out: &[u8],
        reply: &mut [u8],
    ) -> Result<usize, Error> {
        assert!(self.area.write_bytes(0, &setup.to_bytes()));
        assert!(self.area.write_bytes(8, out));
        let stage = self
            .area
            .reference(0, 8 + u64::from(setup.length))
            .expect("the stage lies in the area");
        let moved = usize::try_from(self.client.control(pipe, stage)?).expect("fits");
        let got = reply.get_mut(..moved).expect("the reply fits");
        assert!(self.area.read_bytes(8, got));
        Ok(moved)
    }
}

fn exception(exception: Exception) -> Result<u32, Error> {
    Err(Error::Exception(exception))
}

/// Enumerate the device on the root port to `ROOT_ADDRESS`. Returns its
/// device descriptor, its configuration descriptor and the length of that.
fn enumerate(hcd: &Hcd<'_>) -> ([u8; 18], [u8; REPLY_CAPACITY], usize, UsbSpeed) {
    let mut enumerator = Enumerator::new(ROOT_ADDRESS).expect("a valid address");
    let mut address = 0;
    let mut max_packet = 8;
    let mut speed = UsbSpeed::Full;
    let mut device = [0; 18];
    let mut configuration = [0; REPLY_CAPACITY];
    let mut configuration_len = 0;
    let mut reply = [0; REPLY_CAPACITY];
    // The pipe to the device as it is addressed now.
    let mut pipe = None::<(u32, u8, u16)>;
    let mut step = enumerator.next(Event::Start);
    loop {
        step = match step {
            Step::ResetPort => {
                let reset = hcd
                    .client
                    .reset_port(0)
                    .unwrap_or_else(|error| panic!("reset_port failed: {error:?}"));
                speed = speed_of(reset);
                enumerator.next(Event::PortReset(speed))
            }
            Step::Delay { .. } => enumerator.next(Event::Delayed),
            Step::Control { setup } => {
                // Reopen the pipe when the device's address or packet size
                // changed since it was opened.
                let wanted = (address, max_packet);
                if pipe.is_none_or(|(_, open_address, open_packet)| {
                    (open_address, open_packet) != (wanted.0, wanted.1)
                }) {
                    if let Some((handle, _, _)) = pipe {
                        hcd.client
                            .close_pipe(handle)
                            .unwrap_or_else(|error| panic!("close_pipe failed: {error:?}"));
                    }
                    let handle = hcd
                        .client
                        .open_pipe(
                            u32::from(address),
                            0,
                            match speed {
                                UsbSpeed::Low => Speed::Low,
                                UsbSpeed::Full => Speed::Full,
                                UsbSpeed::High => Speed::High,
                            },
                            TransferKind::Control,
                            max_packet,
                            0,
                        )
                        .unwrap_or_else(|error| panic!("open_pipe failed: {error:?}"));
                    pipe = Some((handle, address, max_packet));
                }
                let (handle, _, _) = pipe.expect("a pipe is open");
                let length = usize::from(setup.length).min(reply.len());
                let data = reply.get_mut(..length).unwrap_or(&mut []);
                match hcd.control(handle, setup, &[], data) {
                    Ok(moved) => {
                        let got = reply.get(..moved).unwrap_or_default();
                        let descriptor_type = setup.value >> 8;
                        if setup.request == REQUEST_SET_ADDRESS {
                            address = u8::try_from(setup.value).expect("a 7-bit address");
                        }
                        if descriptor_type == u16::from(DESCRIPTOR_DEVICE) {
                            if setup.length == 8 {
                                max_packet = u16::from(got.get(7).copied().unwrap_or(8));
                            } else if let Some(copy) = device.get_mut(..got.len()) {
                                copy.copy_from_slice(got);
                            }
                        }
                        if descriptor_type == u16::from(DESCRIPTOR_CONFIGURATION)
                            && setup.length > 9
                            && let Some(copy) = configuration.get_mut(..got.len())
                        {
                            copy.copy_from_slice(got);
                            configuration_len = got.len();
                        }
                        enumerator.next(Event::Control(got))
                    }
                    Err(Error::Exception(Exception::Stalled)) => {
                        enumerator.next(Event::ControlFailed(Failure::Stalled))
                    }
                    Err(_) => enumerator.next(Event::ControlFailed(Failure::Transfer)),
                }
            }
            Step::Configured { .. } => break,
            Step::Failed(failure) => panic!("enumeration failed: {failure:?}"),
        };
    }
    if let Some((handle, _, _)) = pipe {
        hcd.client
            .close_pipe(handle)
            .unwrap_or_else(|error| panic!("close_pipe failed: {error:?}"));
    }
    (device, configuration, configuration_len, speed)
}

/// Pipe and stage rules the controller component enforces.
fn check_rules(hcd: &Hcd<'_>, speed: Speed) {
    let client = hcd.client;
    let full =
        |address: u32| client.open_pipe(address, 0, Speed::Full, TransferKind::Control, 8, 0);

    // Status of the ports: the root port is connected and enabled, and no
    // other port exists.
    let status = client.port_status(0).expect("port status");
    assert_eq!(status & 0b11, 0b11, "connected and enabled: {status:#x}");
    assert_eq!(client.port_status(1).expect("port status"), 0);
    assert_eq!(
        client.reset_port(1),
        Err(Error::Exception(Exception::NoDevice)),
        "only the root port exists"
    );

    // A usable pipe to the enumerated device.
    let pipe = client
        .open_pipe(
            u32::from(ROOT_ADDRESS),
            0,
            speed,
            TransferKind::Control,
            64,
            0,
        )
        .unwrap_or_else(|error| panic!("open_pipe failed: {error:?}"));
    assert_ne!(pipe, 0, "zero is never a handle");
    let device = SetupPacket::get_descriptor(DESCRIPTOR_DEVICE, 0, 18);
    let mut reply = [0; 18];
    assert_eq!(
        hcd.control(pipe, device, &[], &mut reply).expect("works"),
        18
    );
    assert_eq!(reply[1], 1, "a device descriptor");
    client.clear_halt(pipe).expect("clear_halt on an open pipe");

    // Stages the component refuses: too short for the setup packet, shorter
    // than the data the setup packet announces, outside the area, misplaced.
    let stage = |len: u64| hcd.area.reference(0, len).expect("in the area");
    assert!(hcd.area.write_bytes(0, &device.to_bytes()));
    for refused in [stage(4), stage(8 + 17), stage(0)] {
        assert_eq!(
            client.control(pipe, refused),
            exception(Exception::NotPermitted),
            "{refused:?}"
        );
    }
    for outside in [
        IoRec { base: 0, len: 26 },
        IoRec {
            base: hcd.area.base - 8,
            len: 26,
        },
        IoRec {
            base: hcd.area.base + hcd.area.len - 8,
            len: 26,
        },
        IoRec {
            base: hcd.area.base,
            len: u64::MAX,
        },
    ] {
        assert_eq!(
            client.control(pipe, outside),
            exception(Exception::NotPermitted),
            "{outside:?}"
        );
    }
    // A data stage the controller's page cannot hold.
    let huge = SetupPacket::get_descriptor(DESCRIPTOR_CONFIGURATION, 0, 4033);
    assert!(hcd.area.write_bytes(0, &huge.to_bytes()));
    let big = IoRec {
        base: hcd.area.base,
        len: 8 + 4033,
    };
    assert_eq!(
        client.control(pipe, big),
        exception(Exception::NotPermitted),
        "the area holds the stage, the controller's page does not hold the data"
    );
    // A refused stage leaves the pipe usable.
    assert_eq!(
        hcd.control(pipe, device, &[], &mut reply).expect("works"),
        18
    );

    // Pipes the component refuses to open.
    for (address, endpoint, kind, max_packet) in [
        (128, 0, TransferKind::Control, 8),
        (1, 16, TransferKind::Control, 8),
        (1, 0, TransferKind::Control, 7),
        (1, 0, TransferKind::Control, 0),
        (1, 0, TransferKind::Control, 128),
        (1, 1, TransferKind::Interrupt, 8),
        (1, 2, TransferKind::Bulk, 64),
    ] {
        assert_eq!(
            client.open_pipe(address, endpoint, Speed::Full, kind, max_packet, 0),
            exception(Exception::NotPermitted),
            "{address} {endpoint} {max_packet}"
        );
    }

    // Routes: validated, kept, and not served yet.
    let routed = full(2).expect("a second pipe");
    assert_ne!(routed, pipe);
    assert_eq!(
        client.set_route(routed, 0, 1),
        Err(Error::Exception(Exception::NotPermitted))
    );
    assert_eq!(
        client.set_route(routed, 1, 0),
        Err(Error::Exception(Exception::NotPermitted))
    );
    assert_eq!(
        client.set_route(routed, 128, 1),
        Err(Error::Exception(Exception::NotPermitted))
    );
    client
        .set_route(routed, 1, 2)
        .expect("a full-speed device can sit behind a hub");
    assert_eq!(
        client.control(routed, stage(26)),
        exception(Exception::NotPermitted),
        "split transactions are not implemented"
    );
    let fast = client
        .open_pipe(3, 0, Speed::High, TransferKind::Control, 64, 0)
        .expect("a high-speed pipe");
    assert_eq!(
        client.set_route(fast, 1, 2),
        Err(Error::Exception(Exception::NotPermitted))
    );
    assert_eq!(
        client.set_route(0, 1, 2),
        Err(Error::Exception(Exception::InvalidPipe))
    );

    // Closed, never-opened and stale handles.
    for handle in [0, 1 << 16, u32::MAX] {
        assert_eq!(
            client.control(handle, stage(26)),
            exception(Exception::InvalidPipe)
        );
        assert_eq!(
            client.close_pipe(handle),
            Err(Error::Exception(Exception::InvalidPipe))
        );
        assert_eq!(
            client.clear_halt(handle),
            Err(Error::Exception(Exception::InvalidPipe))
        );
    }
    client.close_pipe(fast).expect("close");
    client.close_pipe(routed).expect("close");
    assert_eq!(
        client.close_pipe(routed),
        Err(Error::Exception(Exception::InvalidPipe))
    );
    assert_eq!(
        client.control(routed, stage(26)),
        exception(Exception::InvalidPipe)
    );
    let reused = full(2).expect("the slot is free again");
    assert_ne!(reused, routed, "a reused slot has a new handle");
    assert_eq!(
        client.clear_halt(routed),
        Err(Error::Exception(Exception::InvalidPipe))
    );
    client.close_pipe(reused).expect("close");

    // The table has room for MAX_PIPES pipes, and no more.
    client.close_pipe(pipe).expect("close");
    let mut handles = [0; MAX_PIPES];
    for handle in &mut handles {
        *handle = full(1).expect("room for the pipe");
    }
    assert_eq!(full(1), exception(Exception::Busy));
    for handle in handles {
        client.close_pipe(handle).expect("close");
    }
    full(1)
        .and_then(|handle| client.close_pipe(handle))
        .expect("room again");
}

fn main(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    EventCountKey::from_key(RawKey::from_wire(init.event_count))
        .await_ge(1, EventCountKey::WAIT_INFINITE)
        .unwrap_or_else(|error| {
            panic!(
                "waiting for the background Thread failed: {:?}",
                error.code()
            )
        });
    let client = Client::new(
        Bindings::from_imports(init.imports()),
        // SAFETY: this Thread is the only one calling through the client's
        // imports.
        unsafe { ExclusiveStacks::new() },
    );
    let area = client
        .data_area()
        .expect("the client shares a data area with the controller");
    assert_eq!(area.len, 4096);
    let hcd = Hcd {
        client: &client,
        area,
    };

    assert_eq!(
        client.port_status(0).expect("port status") & 1,
        1,
        "a device is attached"
    );
    let (device, configuration, configuration_len, speed) = enumerate(&hcd);
    let Some(Ok(Descriptor::Device(root))) = descriptor::walk(&device).next() else {
        panic!("the root device's descriptor is malformed");
    };
    let interfaces = descriptor::walk(configuration.get(..configuration_len).unwrap_or_default())
        .filter_map(|descriptor| match descriptor {
            Ok(Descriptor::Interface(interface)) => Some(interface.class_code),
            _ => None,
        })
        .count();
    #[cfg(feature = "qemu")]
    libuser::semihosting::println!(
        "usb-test-client: root port {speed:?}: device at address {ROOT_ADDRESS}, class {:#x}, vendor {:#06x} product {:#06x}, {interfaces} interfaces",
        root.class_code,
        root.vendor,
        root.product
    );
    assert_eq!(root.class_code, CLASS_HUB, "the root device is a hub");
    assert!(interfaces > 0, "the hub has an interface");

    check_rules(
        &hcd,
        match speed {
            UsbSpeed::Low => Speed::Low,
            UsbSpeed::Full => Speed::Full,
            UsbSpeed::High => Speed::High,
        },
    );

    NotificationKey::from_key(report_key(init.table))
        .signal(0)
        .unwrap_or_else(|error| panic!("report failed: {:?}", error.code()));
    let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
    }
}

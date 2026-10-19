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
//!
//! A pipe is the four `pipe_v1` keys `open_pipe` installs in this program's
//! table: the program lends its table, names the first of four vacant slots and
//! reads the keys' values from the data area.

use {
    driver_protocol::report_key,
    libdevice_driver::{driver_init, driver_threads},
    libmeddle_runtime::{DataArea, ExclusiveStacks, IoRec},
    libobject::{EventCountKey, KeyTableKey, NotificationKey, RawKey, Rights},
    usb_hcd_client::{
        pipe_v1::{self, Exception as PipeException},
        usb_hcd_v1::{Bindings, Client, Error, Exception, Speed, TransferKind},
    },
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
/// Keys one pipe has: one per function of `pipe_v1`.
const PIPE_KEYS: usize = pipe_v1::FUNCTION_COUNT;
/// Groups of `PIPE_KEYS` slots the program sets aside for pipe keys: the
/// controller's pipes, and a few more for copies and refused attempts.
const GROUPS: usize = MAX_PIPES + 4;

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

/// An open pipe: its keys, and where they are in the program's table.
struct Pipe {
    client: pipe_v1::Client,
    group: usize,
    keys: [u64; PIPE_KEYS],
}

/// The host controller, the data area it shares with this program, and the
/// table the pipes' keys are installed in.
struct Hcd<'a> {
    client: &'a Client,
    area: DataArea,
    table: KeyTableKey,
    /// First slot of the first group.
    first_slot: u32,
    /// The target stack of calls into the controller, which pipe keys share.
    stack_end: u64,
    used: [bool; GROUPS],
}

impl Hcd<'_> {
    /// The first slot of group `group`.
    fn slot_of(&self, group: usize) -> u32 {
        self.first_slot + u32::try_from(group * PIPE_KEYS).expect("a slot fits in u32")
    }

    /// A group of vacant slots, marked used.
    fn take_group(&mut self) -> usize {
        let group = self
            .used
            .iter()
            .position(|used| !used)
            .expect("a free group of slots");
        self.used[group] = true;
        group
    }

    /// A pipe client for the keys at `keys`.
    fn pipe_client(&self, keys: &[u64; PIPE_KEYS]) -> pipe_v1::Client {
        let bindings = pipe_v1::Bindings::from_installed(keys, self.stack_end, Some(self.area))
            .expect("one key per function");
        pipe_v1::Client::new(
            bindings,
            // SAFETY: this Thread is the only one calling through the
            // controller's stacks.
            unsafe { ExclusiveStacks::new() },
        )
    }

    /// Open a pipe, installing its keys in a group of vacant slots.
    fn open(
        &mut self,
        address: u32,
        endpoint: u32,
        speed: Speed,
        kind: TransferKind,
        max_packet: u16,
    ) -> Result<Pipe, Error> {
        let group = self.take_group();
        let slot = self.slot_of(group);
        let opened = self.client.open_pipe(
            &self.table,
            slot,
            address,
            endpoint,
            speed,
            kind,
            max_packet,
        );
        if let Err(error) = opened {
            self.used[group] = false;
            return Err(error);
        }
        let mut keys = [0; PIPE_KEYS];
        assert!(
            self.area.read_u64s(0, &mut keys),
            "the keys are in the area"
        );
        Ok(Pipe {
            client: self.pipe_client(&keys),
            group,
            keys,
        })
    }

    /// Close `pipe` through its key and give its slots back.
    fn close(&mut self, pipe: &Pipe) {
        pipe.client
            .close_pipe()
            .unwrap_or_else(|error| panic!("close_pipe failed: {error:?}"));
        self.release(pipe);
    }

    /// Delete the keys of `pipe` from the table and free its group.
    fn release(&mut self, pipe: &Pipe) {
        for key in pipe.keys {
            self.table
                .delete(RawKey::from_wire(key))
                .unwrap_or_else(|error| panic!("deleting a pipe key failed: {:?}", error.code()));
        }
        self.used[pipe.group] = false;
    }

    /// Run `setup` on `pipe`, sending `out` as its data stage. A reply goes to
    /// `reply`; returns the bytes of it.
    fn control(
        &self,
        pipe: &Pipe,
        setup: SetupPacket,
        out: &[u8],
        reply: &mut [u8],
    ) -> Result<usize, pipe_v1::Error> {
        assert!(self.area.write_bytes(0, &setup.to_bytes()));
        assert!(self.area.write_bytes(8, out));
        let stage = self
            .area
            .reference(0, 8 + u64::from(setup.length))
            .expect("the stage lies in the area");
        let moved = usize::try_from(pipe.client.control(stage)?).expect("fits");
        let got = reply.get_mut(..moved).expect("the reply fits");
        assert!(self.area.read_bytes(8, got));
        Ok(moved)
    }
}

/// Enumerate the device on the root port to `ROOT_ADDRESS`. Returns its
/// device descriptor, its configuration descriptor and the length of that.
fn enumerate(hcd: &mut Hcd<'_>) -> ([u8; 18], [u8; REPLY_CAPACITY], usize, UsbSpeed) {
    let mut enumerator = Enumerator::new(ROOT_ADDRESS).expect("a valid address");
    let mut address = 0;
    let mut max_packet = 8;
    let mut speed = UsbSpeed::Full;
    let mut device = [0; 18];
    let mut configuration = [0; REPLY_CAPACITY];
    let mut configuration_len = 0;
    let mut reply = [0; REPLY_CAPACITY];
    // The pipe to the device as it is addressed now.
    let mut pipe = None::<(Pipe, u8, u16)>;
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
                if pipe.as_ref().is_none_or(|(_, open_address, open_packet)| {
                    (*open_address, *open_packet) != (wanted.0, wanted.1)
                }) {
                    if let Some((old, _, _)) = pipe.take() {
                        hcd.close(&old);
                    }
                    let opened = hcd
                        .open(
                            u32::from(address),
                            0,
                            match speed {
                                UsbSpeed::Low => Speed::Low,
                                UsbSpeed::Full => Speed::Full,
                                UsbSpeed::High => Speed::High,
                            },
                            TransferKind::Control,
                            max_packet,
                        )
                        .unwrap_or_else(|error| panic!("open_pipe failed: {error:?}"));
                    pipe = Some((opened, address, max_packet));
                }
                let (open, _, _) = pipe.as_ref().expect("a pipe is open");
                let length = usize::from(setup.length).min(reply.len());
                let data = reply.get_mut(..length).unwrap_or(&mut []);
                match hcd.control(open, setup, &[], data) {
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
                    Err(pipe_v1::Error::Exception(PipeException::Stalled)) => {
                        enumerator.next(Event::ControlFailed(Failure::Stalled))
                    }
                    Err(_) => enumerator.next(Event::ControlFailed(Failure::Transfer)),
                }
            }
            Step::Configured { .. } => break,
            Step::Failed(failure) => panic!("enumeration failed: {failure:?}"),
        };
    }
    if let Some((open, _, _)) = pipe {
        hcd.close(&open);
    }
    (device, configuration, configuration_len, speed)
}

/// Pipe and stage rules the controller component enforces.
#[allow(clippy::too_many_lines, reason = "one scenario, step by step")]
fn check_rules(hcd: &mut Hcd<'_>, speed: Speed) {
    let client = hcd.client;

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
    let pipe = hcd
        .open(u32::from(ROOT_ADDRESS), 0, speed, TransferKind::Control, 64)
        .unwrap_or_else(|error| panic!("open_pipe failed: {error:?}"));
    assert!(
        pipe.keys.iter().all(|&key| key != 0),
        "every function has a key"
    );
    let device = SetupPacket::get_descriptor(DESCRIPTOR_DEVICE, 0, 18);
    let mut reply = [0; 18];
    assert_eq!(
        hcd.control(&pipe, device, &[], &mut reply).expect("works"),
        18
    );
    assert_eq!(reply[1], 1, "a device descriptor");
    pipe.client
        .clear_halt()
        .expect("clear_halt on an open pipe");

    // Stages the component refuses: too short for the setup packet, shorter
    // than the data the setup packet announces, outside the area, misplaced.
    let area = hcd.area;
    let stage = |len: u64| area.reference(0, len).expect("in the area");
    assert!(hcd.area.write_bytes(0, &device.to_bytes()));
    for refused in [stage(4), stage(8 + 17), stage(0)] {
        assert_eq!(
            pipe.client.control(refused),
            Err(pipe_v1::Error::Exception(PipeException::NotPermitted)),
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
            pipe.client.control(outside),
            Err(pipe_v1::Error::Exception(PipeException::NotPermitted)),
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
        pipe.client.control(big),
        Err(pipe_v1::Error::Exception(PipeException::NotPermitted)),
        "the area holds the stage, the controller's page does not hold the data"
    );
    // A refused stage leaves the pipe usable.
    assert_eq!(
        hcd.control(&pipe, device, &[], &mut reply).expect("works"),
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
        assert!(
            matches!(
                hcd.open(address, endpoint, Speed::Full, kind, max_packet),
                Err(Error::Exception(Exception::NotPermitted))
            ),
            "{address} {endpoint} {max_packet}"
        );
    }
    // Slots that are not vacant, or outside the table, refuse the pipe, and the
    // controller takes the half-made pipe back.
    let taken = hcd.slot_of(pipe.group);
    for first_slot in [taken, taken + 1, u32::MAX - 1, 100_000] {
        assert_eq!(
            client.open_pipe(
                &hcd.table,
                first_slot,
                2,
                0,
                Speed::Full,
                TransferKind::Control,
                8
            ),
            Err(Error::Exception(Exception::NotPermitted)),
            "{first_slot:#x}"
        );
    }

    // Routes: validated, kept, and not served yet.
    let routed = hcd
        .open(2, 0, Speed::Full, TransferKind::Control, 8)
        .expect("a second pipe");
    assert_ne!(routed.keys, pipe.keys);
    for (hub, port) in [(0, 1), (1, 0), (128, 1)] {
        assert_eq!(
            routed.client.set_route(hub, port),
            Err(pipe_v1::Error::Exception(PipeException::NotPermitted)),
            "{hub} {port}"
        );
    }
    routed
        .client
        .set_route(1, 2)
        .expect("a full-speed device can sit behind a hub");
    assert_eq!(
        routed.client.control(stage(26)),
        Err(pipe_v1::Error::Exception(PipeException::NotPermitted)),
        "split transactions are not implemented"
    );
    let fast = hcd
        .open(3, 0, Speed::High, TransferKind::Control, 64)
        .expect("a high-speed pipe");
    assert_eq!(
        fast.client.set_route(1, 2),
        Err(pipe_v1::Error::Exception(PipeException::NotPermitted))
    );

    // Holding a key is the right to use the pipe: a copy of the keys in other
    // slots works, closing through the copy closes the pipe for every holder,
    // and the keys left behind fail.
    let copy_group = hcd.take_group();
    let copy_slot = hcd.slot_of(copy_group);
    let mut copies = [0; PIPE_KEYS];
    for (index, (copy, &key)) in copies.iter_mut().zip(&fast.keys).enumerate() {
        let slot = copy_slot + u32::try_from(index).expect("a small index");
        *copy = hcd
            .table
            .copy_derive(
                RawKey::from_wire(key),
                &hcd.table,
                slot,
                Rights(Rights::CALL),
            )
            .unwrap_or_else(|error| panic!("copying a pipe key failed: {:?}", error.code()))
            .to_wire();
    }
    let delegated = Pipe {
        client: hcd.pipe_client(&copies),
        group: copy_group,
        keys: copies,
    };
    delegated.client.clear_halt().expect("a copy works");
    delegated.client.close_pipe().expect("a copy closes");
    // The copy and the original name a pipe that is gone now.
    for gone in [&delegated, &fast] {
        assert_eq!(
            gone.client.clear_halt(),
            Err(pipe_v1::Error::Exception(PipeException::InvalidPipe))
        );
    }
    hcd.release(&delegated);
    hcd.release(&fast);

    // Keys of a closed pipe keep failing, even when its table slot is reused by
    // a new pipe.
    routed.client.close_pipe().expect("close");
    assert_eq!(
        routed.client.control(stage(26)),
        Err(pipe_v1::Error::Exception(PipeException::InvalidPipe))
    );
    let reused = hcd
        .open(2, 0, Speed::Full, TransferKind::Control, 8)
        .expect("the slot is free again");
    assert_ne!(
        reused.keys, routed.keys,
        "a reused pipe slot has a new badge"
    );
    for result in [
        routed.client.clear_halt(),
        routed.client.set_route(1, 2),
        routed.client.close_pipe(),
    ] {
        assert_eq!(
            result,
            Err(pipe_v1::Error::Exception(PipeException::InvalidPipe))
        );
    }
    reused.client.clear_halt().expect("the new pipe is fine");
    hcd.close(&reused);
    hcd.release(&routed);

    // The table has room for MAX_PIPES pipes, and no more.
    hcd.close(&pipe);
    let mut pipes: [Option<Pipe>; MAX_PIPES] = [const { None }; MAX_PIPES];
    for slot in &mut pipes {
        *slot = Some(
            hcd.open(1, 0, Speed::Full, TransferKind::Control, 8)
                .expect("room for the pipe"),
        );
    }
    assert!(
        matches!(
            hcd.open(1, 0, Speed::Full, TransferKind::Control, 8),
            Err(Error::Exception(Exception::Busy))
        ),
        "the table is full"
    );
    for slot in &mut pipes {
        hcd.close(&slot.take().expect("an open pipe"));
    }
    let last = hcd
        .open(1, 0, Speed::Full, TransferKind::Control, 8)
        .expect("room again");
    hcd.close(&last);
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
    let bindings = Bindings::from_imports(init.imports());
    let stack_end = bindings
        .open_pipe
        .as_ref()
        .expect("open_pipe is granted")
        .stack_end;
    let client = Client::new(
        bindings,
        // SAFETY: this Thread is the only one calling through the client's
        // imports.
        unsafe { ExclusiveStacks::new() },
    );
    let area = client
        .data_area()
        .expect("the client shares a data area with the controller");
    assert_eq!(area.len, 4096);
    let mut hcd = Hcd {
        client: &client,
        area,
        table: KeyTableKey::from_key(RawKey::from_wire(init.table)),
        first_slot: u32::try_from(init.first_free_slot).expect("a slot fits in u32"),
        stack_end,
        used: [false; GROUPS],
    };

    assert_eq!(
        client.port_status(0).expect("port status") & 1,
        1,
        "a device is attached"
    );
    let (device, configuration, configuration_len, speed) = enumerate(&mut hcd);
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
        &mut hcd,
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

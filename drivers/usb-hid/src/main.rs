//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

// Host tests build this binary natively too; it is only an empty `main` there.
#![cfg_attr(target_os = "vesper", no_std, no_main)]

//! The USB HID component, `usb-hid`.
//!
//! It serves the boot keyboard and the boot mouse. For each it asks the USB
//! core to claim the interface (`usb_v1.claim_interface`, retried while none is
//! attached), learns how to open the interface's interrupt-IN pipe
//! (`usb_v1.interrupt_pipe`), puts the device in boot protocol over a control
//! pipe of its own, with an idle rate (`Set_Idle`, HID 1.11 §7.2.4) that has
//! the keyboard send its state again every `KEYBOARD_IDLE_4MS` times 4 ms
//! while nothing changes (the mouse reports only changes), and opens the
//! interrupt pipe on the host controller (`usb_hcd_v1.open_pipe`,
//! `pipe_v1.configure`). A keyboard report lost on the way (a split
//! transaction the hub dropped) would otherwise leave a key down, or up, until
//! the next change; the repeated state puts it right. The reports
//! arrive on the io-channel the controller shares with this component, in
//! buffers it posted; each is decoded into input events and published, in one
//! frame, on the channel of every client of `input_v1` that posted a buffer.
//! A client that posted none misses the frame, and the next frame it gets
//! starts with a `Dropped` record.
//!
//! The component's one Thread runs at input priority
//! (`libdevice_driver::Priority::INPUT`), ahead of the CPU-bound programs: it
//! blocks between reports and does little work for each.
//!
//! The claims are polled: a device that went away is noticed within
//! `RECHECK_MS`, its pipe closed, its claim released and a `DeviceRemoved`
//! record published, and its interface is claimed again when it returns.
//!
//! `input_v1` serves a device's descriptor and a snapshot of its state to its
//! clients. They share the device table with the Thread that publishes
//! events, under a `Turnstile`.

/// `note!("text {value}")`: one trace line, on the semihosting console and on
/// the log channel when the image has a log server, else in the backlog
/// `input_v1.trace` drains.
///
/// What is traced: each claim with its interrupt pipe's parameters, the
/// device served or gone, and failures. Nothing is traced per report. The
/// claim is retried every `RECHECK_MS`, so a wait for a device and a set-up
/// that keeps failing are traced the 1st, 2nd, 4th, 8th... time with their
/// count; so are reports that did not decode and frames a client of
/// `input_v1` missed.
#[cfg(target_os = "vesper")]
macro_rules! note {
    ($($argument:tt)*) => {
        libserial_trace::note(format_args!($($argument)*))
    };
}

#[cfg(target_os = "vesper")]
mod component {
    use {
        core::cell::UnsafeCell,
        input::input_v1::{Exception, InputV1},
        libdevice_driver::{
            DriverInit, ExportedArea, INIT_VA, MAX_AREAS, Priority, Turn, Turnstile, driver_init,
            entry_priority,
        },
        libio_channel::{Consumer, Cursor, IoRec, NoEvent, Packet, Producer},
        libmeddle_runtime::{DataArea, IoRec as StageRec},
        libobject::{
            CapError, EventCountKey, KeyTableKey, ObjectType, RawKey, Rights, UntypedKey,
            export::init_return_key, thread::ThreadReturnKey,
        },
        usb_client::usb_v1::{
            Bindings as UsbBindings, Client as UsbClient, Error as UsbError,
            Exception as UsbException,
        },
        usb_core::claims::{HubRoute, PipeParams},
        usb_hcd_client::{
            pipe_v1::TransferKind,
            usb_hcd_v1::{Bindings, Client, Speed as HcdSpeed},
        },
        usb_hid::{
            channel::{EventChannel, SLOTS, data_area as event_data_area},
            decode::{Frame, MAX_FRAME_BYTES, interval_ms},
            devices::{DeviceKind, Devices, MAX_DEVICES, keyboard_descriptor, mouse_descriptor},
        },
        usb_host::pipes::{Pipe, Pipes},
        vesper_input_event::{
            descriptor::DESCRIPTOR_SIZE,
            record::{EVENT_SIZE, Event, Kind},
        },
        vesper_usb_protocol::{
            enumeration::Speed,
            hid_boot::{set_idle, set_protocol},
            report_channel::{ReportChannel, data_area as report_data_area},
        },
    };

    libuser::entry!(main);

    entry_priority!(Priority::INPUT);

    /// Milliseconds between two looks for devices to claim and for devices that
    /// went away.
    const RECHECK_MS: u64 = 250;
    /// The keyboard's idle rate, in 4 ms units: it sends its state again every
    /// 100 ms while nothing changes, so a lost report is put right within
    /// 100 ms (two, if the loss also cost the next report its DATA PID).
    /// HID 1.11 §7.2.4 recommends 500 ms; repeating the same state changes
    /// nothing here, and a stuck key should not run for half a second.
    const KEYBOARD_IDLE_4MS: u8 = 25;
    /// Empty buffers kept posted on the controller's channel.
    const BUFFERS: u64 = 4;
    /// Bytes of each: a boot report is at most a handful.
    const BUFFER_LEN: usize = 64;
    const BUFFER_BYTES: u64 = BUFFER_LEN as u64;
    /// Control packet size the setup requests use: every device accepts 8.
    const SETUP_PACKET: u16 = 8;
    /// HID class, boot subclass, and the keyboard and mouse protocols.
    const CLASS_HID: u32 = 3;
    const SUBCLASS_BOOT: u32 = 1;
    const PROTOCOL_KEYBOARD: u32 = 1;
    const PROTOCOL_MOUSE: u32 = 2;
    /// Bytes of a delivery: a `Dropped` record and a frame.
    const DELIVERY_BYTES: usize = EVENT_SIZE + MAX_FRAME_BYTES;

    /// What the exports and the publishing Thread share.
    struct Shared(UnsafeCell<Devices>);

    // SAFETY: the cell is used only by the holder of a `TURNSTILE` turn.
    unsafe impl Sync for Shared {}

    static DEVICES: Shared = Shared(UnsafeCell::new(Devices::new()));
    static TURNSTILE: Turnstile = Turnstile::new();

    fn init() -> &'static DriverInit {
        // SAFETY: Kickstart maps every component's init record at `INIT_VA`; it
        // is read in this component's own `AddressSpace` only.
        unsafe { driver_init(INIT_VA) }
    }

    #[allow(
        clippy::mut_from_ref,
        reason = "the exclusive borrow of the turn proves the holder has the cell to itself"
    )]
    fn devices<'turn>(_turn: &'turn mut Turn<'_>) -> &'turn mut Devices {
        // SAFETY: only the holder of a turn gets here, one at a time.
        unsafe { &mut *DEVICES.0.get() }
    }

    fn now_ns() -> u64 {
        libuser::clock::now_ns()
    }

    /// The data area of the client whose key carries `badge`.
    fn caller_area(badge: u64) -> Option<DataArea> {
        let (_, exported) = init().area_for_badge(badge)?;
        Some(DataArea {
            base: exported.base,
            len: exported.len,
        })
    }

    /// The input producer.
    struct Hid;

    impl InputV1 for Hid {
        fn enter(_badge: u64) {
            init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
                init().return_key,
            )));
        }

        fn descriptor(badge: u64, subject: u32, stage: StageRec) -> Result<(), Exception> {
            let area = caller_area(badge).ok_or(Exception::NotPermitted)?;
            if !area.contains(&stage) || stage.len < DESCRIPTOR_SIZE as u64 {
                return Err(Exception::NotPermitted);
            }
            let mut turn = TURNSTILE.enter(init());
            let device = devices(&mut turn).get(subject).ok_or(Exception::NoDevice)?;
            if area.write_bytes(stage.base - area.base, &device.descriptor().to_bytes()) {
                Ok(())
            } else {
                Err(Exception::NotPermitted)
            }
        }

        fn trace(badge: u64, text: StageRec) -> Result<u32, Exception> {
            let area = caller_area(badge).ok_or(Exception::NotPermitted)?;
            if !area.contains(&text) {
                return Err(Exception::NotPermitted);
            }
            let capacity =
                usize::try_from(text.len).map_err(|_overflow| Exception::NotPermitted)?;
            // The turn makes this the backlog's one reader.
            let _turn = TURNSTILE.enter(init());
            let mut written = 0;
            let mut chunk = [0; 256];
            while written < capacity {
                let count = libserial_trace::drain(&mut chunk[..(capacity - written).min(256)]);
                if count == 0 {
                    break;
                }
                let offset = text.base - area.base + written as u64;
                if !area.write_bytes(offset, &chunk[..count]) {
                    return Err(Exception::NotPermitted);
                }
                written += count;
            }
            Ok(u32::try_from(written).unwrap_or(u32::MAX))
        }

        fn snapshot(badge: u64, subject: u32, stage: StageRec) -> Result<u32, Exception> {
            let area = caller_area(badge).ok_or(Exception::NotPermitted)?;
            let mut frame = Frame::new();
            {
                let mut turn = TURNSTILE.enter(init());
                devices(&mut turn)
                    .snapshot(subject, now_ns(), &mut frame)
                    .ok_or(Exception::NoDevice)?;
            }
            let mut bytes = [0_u8; MAX_FRAME_BYTES];
            let length = frame.write_to(&mut bytes).ok_or(Exception::NotPermitted)?;
            if !area.contains(&stage) || stage.len < length as u64 {
                return Err(Exception::NotPermitted);
            }
            if area.write_bytes(stage.base - area.base, &bytes[..length]) {
                Ok(u32::try_from(length).unwrap_or(u32::MAX))
            } else {
                Err(Exception::NotPermitted)
            }
        }
    }

    input::input_v1_export!(Hid);

    /// A device being served: its claim, its interrupt pipe and its id.
    struct Active {
        subject: u32,
        claim: u32,
        pipe: Pipe,
        kind: DeviceKind,
    }

    type Buffers = Producer<'static, SLOTS, NoEvent>;
    type Completed = Consumer<'static, SLOTS, EventCountKey>;

    /// The component's state while it runs.
    struct Service<'a> {
        usb: &'a UsbClient,
        pipes: Pipes<'a>,
        /// Posts empty buffers on the controller's channel.
        buffers: Buffers,
        /// Gets the buffers back with reports.
        completed: Completed,
        /// The data page of the controller's channel.
        report_data: libio_channel::DataArea,
        /// Clients that missed a frame since their last delivery, by area index.
        missed: [bool; MAX_AREAS],
        active: [Option<Active>; MAX_DEVICES],
        /// Claims refused because nothing of the kind is attached yet, and
        /// claims that failed, by kind, for the trace's back-off.
        waits: [u32; 2],
        failures: [u32; 2],
        /// Reports that did not decode, and frames a client missed, for the
        /// trace's back-off.
        bad_reports: u32,
        missed_frames: u32,
    }

    /// Count one more on `counter`; the count when this one is traced (the
    /// 1st, 2nd, 4th, 8th...).
    fn counted(counter: &mut u32) -> Option<u32> {
        *counter = counter.wrapping_add(1);
        counter.is_power_of_two().then_some(*counter)
    }

    fn hcd_speed(speed: Speed) -> HcdSpeed {
        match speed {
            Speed::Low => HcdSpeed::Low,
            Speed::Full => HcdSpeed::Full,
            Speed::High => HcdSpeed::High,
        }
    }

    impl Service<'_> {
        const fn protocol(kind: DeviceKind) -> u32 {
            match kind {
                DeviceKind::Keyboard => PROTOCOL_KEYBOARD,
                DeviceKind::Mouse => PROTOCOL_MOUSE,
            }
        }

        fn serves(&self, kind: DeviceKind) -> bool {
            self.active
                .iter()
                .flatten()
                .any(|active| active.kind == kind)
        }

        /// Post the empty buffers the controller fills with reports.
        fn post_buffers(&mut self) {
            for index in 0..BUFFERS {
                let rec = IoRec {
                    base: self.report_data.base + index * BUFFER_BYTES,
                    len: BUFFER_BYTES,
                };
                if let Some(packet) = Packet::new(index, &[rec]) {
                    assert!(
                        self.buffers.put(&packet).is_ok(),
                        "usb-hid: the report channel has room for its buffers"
                    );
                }
            }
        }

        /// Try to claim and set up a device of `kind`.
        fn attach(&mut self, kind: DeviceKind) {
            let which = match kind {
                DeviceKind::Keyboard => 0,
                DeviceKind::Mouse => 1,
            };
            let claim = match self.usb.claim_interface(
                CLASS_HID,
                SUBCLASS_BOOT,
                Self::protocol(kind),
            ) {
                Ok(claim) => claim,
                Err(UsbError::Exception(UsbException::NotReady)) => {
                    // Waited for at 1, 2, 4, 8... seconds.
                    let ticks_per_second = 1000 / RECHECK_MS;
                    if let Some(waits) = counted(&mut self.waits[which])
                        && u64::from(waits) >= ticks_per_second
                    {
                        note!(
                            "usb-hid: the USB core offers no {kind:?} yet ({} s)",
                            u64::from(waits) / ticks_per_second
                        );
                    }
                    return;
                }
                Err(error) => {
                    if let Some(count) = counted(&mut self.failures[which]) {
                        note!(
                            "usb-hid: claiming a {kind:?} failed ({count} failures so far): {error:?}"
                        );
                    }
                    return;
                }
            };
            // The set-up's own lines follow the same back-off: a device that
            // refuses it is claimed again every `RECHECK_MS`.
            let report = (self.failures[which].wrapping_add(1)).is_power_of_two();
            let Some(active) = self.set_up(kind, claim, report) else {
                let _released = self.usb.release(claim);
                let count = counted(&mut self.failures[which]);
                if let Some(count) = count {
                    note!("usb-hid: setting up the {kind:?} failed ({count} failures so far)");
                }
                return;
            };
            let subject = active.subject;
            if let Some(slot) = self.active.iter_mut().find(|slot| slot.is_none()) {
                *slot = Some(active);
            }
            self.publish_one(Event::new(now_ns(), subject, Kind::DeviceAdded, 0, 0));
            note!(
                "usb-hid: serving a {kind:?} as device {subject} ({} failures before)",
                self.failures[which]
            );
        }

        /// Boot protocol and the interrupt pipe for the claim `claim` of a
        /// `kind`. Its lines are traced only when `report`.
        fn set_up(&mut self, kind: DeviceKind, claim: u32, report: bool) -> Option<Active> {
            macro_rules! report {
                ($($argument:tt)*) => {
                    if report {
                        note!($($argument)*);
                    }
                };
            }
            let Some(params) = self.pipe_params(claim) else {
                report!("usb-hid: claim {claim} of a {kind:?}: no interrupt pipe parameters");
                return None;
            };
            report!(
                "usb-hid: claimed a {kind:?} (claim {claim}): {:04x}:{:04x} at address {} interface {}, endpoint {:#04x}, {:?} speed{}, {} bytes, interval {}",
                params.vendor,
                params.product,
                params.device,
                params.interface,
                params.endpoint,
                params.speed,
                if params.route.is_some() {
                    " behind a hub's translator"
                } else {
                    ""
                },
                params.max_packet,
                params.interval
            );
            let speed = hcd_speed(params.speed);
            let address = u32::from(params.device);
            // Boot protocol and the idle rate, over a control pipe that is
            // closed again.
            let control = self
                .pipes
                .open_unconfigured(address, 0, speed)
                .map_err(|error| {
                    report!("usb-hid: no control pipe: {error:?}");
                })
                .ok()?;
            let configured = route(&control, params.route)
                && control
                    .client
                    .configure(TransferKind::Control, SETUP_PACKET, 0)
                    .is_ok();
            let protocol = if configured {
                self.pipes
                    .control(&control, set_protocol(params.interface, true), &[], &mut [])
                    .map(|_moved| ())
                    .map_err(|error| {
                        report!("usb-hid: the {kind:?} refused SET_PROTOCOL: {error:?}");
                    })
            } else {
                report!("usb-hid: the {kind:?}'s control pipe was refused");
                Err(())
            };
            // The keyboard repeats its state every `KEYBOARD_IDLE_4MS`; a mouse
            // reports only changes, as its reports carry motion (HID 1.11
            // §7.2.4 recommends an infinite idle rate for it). Set_Idle is
            // optional for a boot mouse (HID 1.11 Appendix G): a refusal is
            // only traced. A keyboard that refuses it reports only changes.
            let idle_4ms = match kind {
                DeviceKind::Keyboard => KEYBOARD_IDLE_4MS,
                DeviceKind::Mouse => 0,
            };
            if protocol.is_ok()
                && let Err(error) = self.pipes.control(
                    &control,
                    set_idle(params.interface, idle_4ms, 0),
                    &[],
                    &mut [],
                )
            {
                report!("usb-hid: the {kind:?} refused SET_IDLE: {error:?}");
            }
            self.pipes.close(&control);
            if protocol.is_err() {
                report!("usb-hid: the {kind:?} did not accept boot protocol");
                return None;
            }

            let pipe = self
                .pipes
                .open_unconfigured(address, u32::from(params.endpoint), speed)
                .map_err(|error| {
                    report!("usb-hid: no interrupt pipe: {error:?}");
                })
                .ok()?;
            let polled = route(&pipe, params.route)
                && pipe
                    .client
                    .configure(
                        TransferKind::Interrupt,
                        params.max_packet,
                        interval_ms(params.speed, params.interval),
                    )
                    .is_ok();
            if !polled {
                report!("usb-hid: the {kind:?}'s interrupt pipe was refused");
                self.pipes.close(&pipe);
                return None;
            }
            let descriptor = match kind {
                DeviceKind::Keyboard => keyboard_descriptor(params.vendor, params.product),
                DeviceKind::Mouse => mouse_descriptor(params.vendor, params.product),
            };
            let mut turn = TURNSTILE.enter(init());
            let Some(subject) = devices(&mut turn).add(kind, pipe.handle, descriptor) else {
                drop(turn);
                self.pipes.close(&pipe);
                return None;
            };
            Some(Active {
                subject,
                claim,
                pipe,
                kind,
            })
        }

        /// The record `usb_v1.interrupt_pipe` writes for `claim`.
        fn pipe_params(&self, claim: u32) -> Option<PipeParams> {
            let area = self.usb.data_area()?;
            let stage = area.reference(0, PipeParams::BYTES as u64)?;
            self.usb.interrupt_pipe(claim, stage).ok()?;
            let mut bytes = [0; PipeParams::BYTES];
            area.read_bytes(0, &mut bytes).then_some(())?;
            PipeParams::from_bytes(&bytes)
        }

        /// Whether the core says the device of `claim` went away.
        fn device_gone(&self, claim: u32) -> bool {
            let Some(stage) = self
                .usb
                .data_area()
                .and_then(|area| area.reference(0, PipeParams::BYTES as u64))
            else {
                return false;
            };
            matches!(
                self.usb.interrupt_pipe(claim, stage),
                Err(UsbError::Exception(UsbException::DeviceGone))
            )
        }

        /// Stop serving the devices that went away.
        fn detach_gone(&mut self) {
            for index in 0..self.active.len() {
                let Some(claim) = self.active[index].as_ref().map(|active| active.claim) else {
                    continue;
                };
                if !self.device_gone(claim) {
                    continue;
                }
                let Some(active) = self.active[index].take() else {
                    continue;
                };
                let _closed = active.pipe.client.close_pipe();
                self.pipes.release(&active.pipe);
                let _released = self.usb.release(active.claim);
                {
                    let mut turn = TURNSTILE.enter(init());
                    devices(&mut turn).remove(active.subject);
                }
                self.publish_one(Event::new(
                    now_ns(),
                    active.subject,
                    Kind::DeviceRemoved,
                    0,
                    0,
                ));
                note!("usb-hid: device {} went away", active.subject);
            }
        }

        /// Decode every report that came back, publish its events and post its
        /// buffer again.
        fn drain(&mut self) {
            while let Ok(Some(packet)) = self.completed.get() {
                let Some(rec) = packet.recs().first().copied() else {
                    continue;
                };
                // The buffer is where the controller put it; post it again
                // whatever came back.
                let buffer = IoRec {
                    base: rec.base,
                    len: BUFFER_BYTES,
                };
                if !self.report_data.contains(&buffer) {
                    continue;
                }
                let mut report = [0_u8; BUFFER_LEN];
                let length = usize::try_from(rec.len).unwrap_or(0).min(report.len());
                for (offset, byte) in report[..length].iter_mut().enumerate() {
                    // SAFETY: the buffer lies in the controller channel's data
                    // page, mapped read/write; the controller gave it back.
                    *byte = unsafe {
                        core::ptr::with_exposed_provenance::<u8>(
                            usize::try_from(rec.base).unwrap_or(0) + offset,
                        )
                        .read_volatile()
                    };
                }
                let mut frame = Frame::new();
                let decoded = {
                    let mut turn = TURNSTILE.enter(init());
                    devices(&mut turn).report(
                        u32::try_from(packet.value()).unwrap_or(0),
                        &report[..length],
                        now_ns(),
                        &mut frame,
                    )
                };
                match decoded {
                    Ok(Some(_)) if !frame.is_empty() => self.publish(&frame),
                    Ok(_) => {}
                    Err(error) => {
                        if let Some(count) = counted(&mut self.bad_reports) {
                            note!(
                                "usb-hid: a report of {length} bytes on pipe {} was not decoded ({count} so far): {error:?}",
                                packet.value()
                            );
                        }
                    }
                }
                if let Some(again) = Packet::new(0, &[buffer]) {
                    let _posted = self.buffers.put(&again);
                }
            }
        }

        fn publish_one(&mut self, event: Event) {
            let mut frame = Frame::new();
            frame.push(event);
            frame.push(Event::sync(event.timestamp, event.subject));
            self.publish(&frame);
        }

        /// Deliver `frame` to every client of `input_v1` that has a buffer
        /// posted; the others miss it.
        fn publish(&mut self, frame: &Frame) {
            for (index, area) in init().areas().iter().enumerate() {
                if area.channel_base != 0 {
                    let delivered = deliver(area, frame, self.missed[index]);
                    self.missed[index] = !delivered;
                    if !delivered && let Some(count) = counted(&mut self.missed_frames) {
                        note!(
                            "usb-hid: client {index} of input_v1 had no buffer posted and missed a frame ({count} so far); it gets Dropped next"
                        );
                    }
                }
            }
        }
    }

    /// Set the transaction translator route of `pipe`, if it has one.
    fn route(pipe: &Pipe, route: Option<HubRoute>) -> bool {
        route.is_none_or(|route| {
            pipe.client
                .set_route(u32::from(route.hub), u32::from(route.port))
                .is_ok()
        })
    }

    /// Put `frame` in the first empty buffer the client of `area` posted,
    /// after a `Dropped` record if it `missed` frames. Returns whether it was
    /// delivered.
    fn deliver(area: &ExportedArea, frame: &Frame, missed: bool) -> bool {
        // SAFETY: Kickstart mapped the channel's control page read/write at
        // `channel_base` for this component's lifetime.
        let channel = unsafe { EventChannel::at(area.channel_base) };
        let data = event_data_area(area.channel_base);
        let (mut buffers, mut done) = channel.events.server(
            NoEvent,
            EventCountKey::from_key(RawKey::from_wire(area.channel_event)),
        );
        let Ok(Some(mut packet)) = buffers.get() else {
            return false;
        };
        let mut bytes = [0_u8; DELIVERY_BYTES];
        let mut length = 0;
        if missed {
            bytes[..EVENT_SIZE]
                .copy_from_slice(&Event::new(now_ns(), 0, Kind::Dropped, 0, 0).to_bytes());
            length = EVENT_SIZE;
        }
        let Some(written) = frame.write_to(&mut bytes[length..]) else {
            return false;
        };
        length += written;
        // A frame is never split: the buffer holds it whole or comes back empty.
        let fits = data.holds(&packet) && packet.bytes() >= length as u64;
        let mut cursor = Cursor::START;
        if fits {
            for &byte in &bytes[..length] {
                let Some(address) = cursor.next_address(&packet) else {
                    break;
                };
                // SAFETY: the data area holds the packet and is mapped
                // read/write; the client gave the buffer to this component
                // until it comes back.
                unsafe {
                    core::ptr::with_exposed_provenance_mut::<u8>(
                        usize::try_from(address).unwrap_or(usize::MAX),
                    )
                    .write_volatile(byte);
                }
            }
        }
        cursor.cut(&mut packet);
        // A client with more buffers out than the FIFO holds loses the frame.
        let _lost = done.put(&packet);
        // Only a client's EventCount can fail to advance.
        let _signalled = done.signal();
        fits
    }

    fn main(_init_va: u64) -> ! {
        let init = init();
        note!("usb-hid: starting");
        // The first free slot holds the EventCount that counts the turns of
        // the exports; the pipes' slots follow it.
        let table = KeyTableKey::from_key(RawKey::from_wire(init.table));
        let turns_slot = u32::try_from(init.first_free_slot).expect("a slot fits in u32");
        let turns = UntypedKey::from_key(RawKey::from_wire(init.memory))
            .retype(
                ObjectType::EVENT_COUNT,
                0,
                0,
                1,
                &table,
                turns_slot,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("usb-hid: turns Retype failed: {:?}", error.code()));
        TURNSTILE.open(turns);
        // Say the exports can be served.
        EventCountKey::from_key(RawKey::from_wire(init.event_count))
            .advance(1)
            .unwrap_or_else(|error| panic!("usb-hid: ready advance failed: {:?}", error.code()));

        let usb = UsbClient::new(UsbBindings::from_imports(init.imports()));
        let hcd = Client::new(Bindings::from_imports(init.imports()));
        let area = hcd
            .data_area()
            .expect("usb-hid shares a data area with the controller");
        let channel_area = hcd
            .channel()
            .expect("usb-hid shares an io-channel with the controller");
        assert_ne!(
            channel_area.event, 0,
            "the controller's channel has an EventCount"
        );
        // SAFETY: Kickstart mapped the channel's control page read/write.
        let channel = unsafe { ReportChannel::at(channel_area.control) };
        let event = EventCountKey::from_key(RawKey::from_wire(channel_area.event));
        let (buffers, completed) = channel.reports.client(
            NoEvent,
            EventCountKey::from_key(RawKey::from_wire(channel_area.event)),
        );
        let report_data = report_data_area(channel_area.control);
        let mut service = Service {
            usb: &usb,
            pipes: Pipes::new(&hcd, area, table, turns_slot + 1),
            buffers,
            completed,
            report_data,
            missed: [false; MAX_AREAS],
            active: [const { None }; MAX_DEVICES],
            waits: [0; 2],
            failures: [0; 2],
            bad_reports: 0,
            missed_frames: 0,
        };
        service.post_buffers();
        let mut seen = 0;
        loop {
            for kind in [DeviceKind::Keyboard, DeviceKind::Mouse] {
                if !service.serves(kind) {
                    service.attach(kind);
                }
            }
            service.detach_gone();
            match event.await_ge(seen + 1, RECHECK_MS * 1_000_000) {
                Ok(count) => seen = count,
                Err(CapError::TimedOut { .. }) => {}
                Err(error) => panic!("usb-hid: waiting for reports failed: {:?}", error.code()),
            }
            service.drain();
        }
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}

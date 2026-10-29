//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

// Host tests build this binary natively too; it is only an empty `main` there.
#![cfg_attr(target_os = "vesper", no_std, no_main)]

//! The USB core component, `usb-core`.
//!
//! It calls the DWC2 host controller component (`usb_hcd_v1`) from one Thread.
//! The Thread resets the root port and enumerates the device there. A hub there
//! gets the hub driver: its descriptor is read, every port is powered, and each
//! port runs through `vesper_usb_protocol::hub::Port` (debounce, reset,
//! enumeration, teardown). A port at rest waits for the hub's Status Change
//! endpoint to name it (an interrupt pipe whose reports arrive on the
//! controller's report channel) before its status is read again; the timed
//! waits of debounce, reset and recovery stay timers, and a hub without that
//! pipe is polled. A low- or full-speed device behind a high-speed hub is reached with
//! split transactions: its pipes carry the hub and port as their route.
//!
//! Progress is traced on the log channel when the image has a log server, else
//! on the serial channel when the component has one (`serial = true` in its
//! manifest entry), and on the semihosting console under QEMU. What is traced:
//! the root port's first status and the wait for a device, each enumeration's
//! control transfers and outcome, each device and hub found, Status Change
//! pipes, port resets, devices gone, and failures. Nothing is traced while the
//! ports rest; a hub request that keeps failing is traced the 1st, 2nd, 4th,
//! 8th... time with its count.
//!
//! `usb_v1` serves claims: every interface of an enumerated device is offered,
//! `claim_interface` takes one by its class codes (or raises `not_ready` while
//! none is attached), and `interrupt_pipe` tells the claimant how to open the
//! interface's interrupt-IN pipe itself. The offers and claims are shared with
//! the exports' Threads under a `Turnstile`. Control transfers on a claim are
//! not served yet.

#[cfg(target_os = "vesper")]
#[macro_use]
mod trace;

#[cfg(target_os = "vesper")]
mod component {
    use {
        crate::trace::Trace,
        core::cell::UnsafeCell,
        libdevice_driver::{DriverInit, INIT_VA, Turn, Turnstile, driver_init},
        libio_channel::{Consumer, IoRec, NoEvent, Packet, Producer},
        libmeddle_runtime::DataArea,
        libobject::{
            CapError, EventCountKey, KeyTableKey, ObjectType, RawKey, Rights, UntypedKey,
            export::init_return_key, thread::ThreadReturnKey,
        },
        usb::usb_v1::{Exception, UsbV1},
        usb_core::{
            claims::{ClaimError, Claims, HubRoute, Offer, PipeParams, UseError, offers_of},
            devices::{AddressPool, Attachment, Device, DeviceTable, Removed},
        },
        usb_hcd_client::usb_hcd_v1::{Bindings, Client, Speed as HcdSpeed},
        usb_host::{
            enumerate::{EnumerateError, Enumerated, Route, enumerate},
            pipes::{Pipe, Pipes},
        },
        vesper_usb_protocol::{
            descriptor::{self, Descriptor},
            enumeration::{Speed, interval_ms},
            hub::{
                CLASS_HUB, HubDescriptor, MAX_HUB_DESCRIPTOR, PORT_POWER, PORT_RESET, Port,
                PortAction, PortEvent, PortStatus, clear_port_feature, get_hub_descriptor,
                get_port_status, set_port_feature,
            },
            report_channel::{ReportChannel, SLOTS, data_area as report_data_area},
            setup::SetupPacket,
        },
    };

    libuser::entry!(main);

    /// Buffers posted on the controller's report channel for the hubs' Status
    /// Change bitmaps, and the bytes of each.
    const REPORT_BUFFERS: u64 = 4;
    const REPORT_BYTES: u64 = 64;

    /// Address the root device is enumerated to.
    const ROOT_ADDRESS: u8 = 1;
    /// Ports of a hub the component drives.
    const MAX_PORTS: usize = 8;
    /// Milliseconds between two looks at a port that gave up.
    const FAILED_POLL_MS: u64 = 250;
    /// Hubs the component drives: the root hub and those behind it.
    const MAX_HUBS: usize = 4;
    /// Slot of the component's table where a boot test installs a Notification;
    /// the component signals it once every port of the hub has settled and a
    /// device sits behind the hub.
    #[cfg(feature = "qemu")]
    const REPORT_SLOT: u32 = 250;
    /// Milliseconds between two looks at the root port while no device is there.
    const ROOT_POLL_MS: u32 = 100;

    /// The USB core. Its exported functions run on callers' Threads and use the
    /// interfaces on offer and the claims on them.
    struct UsbCore;

    fn init() -> &'static DriverInit {
        // SAFETY: Kickstart maps every component's init record at `INIT_VA`; it
        // is read in this component's own `AddressSpace` only.
        unsafe { driver_init(INIT_VA) }
    }

    struct Shared(UnsafeCell<Claims>);

    // SAFETY: the cell is used only by the holder of a `TURNSTILE` turn.
    unsafe impl Sync for Shared {}

    static CLAIMS: Shared = Shared(UnsafeCell::new(Claims::new()));
    static TURNSTILE: Turnstile = Turnstile::new();

    #[allow(
        clippy::mut_from_ref,
        reason = "the exclusive borrow of the turn proves the holder has the cell to itself"
    )]
    fn claims<'turn>(_turn: &'turn mut Turn<'_>) -> &'turn mut Claims {
        // SAFETY: only the holder of a turn gets here, one at a time.
        unsafe { &mut *CLAIMS.0.get() }
    }

    impl UsbV1 for UsbCore {
        fn enter(_badge: u64) {
            init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
                init().return_key,
            )));
        }

        fn claim_interface(
            badge: u64,
            class_code: u32,
            subclass: u32,
            protocol: u32,
        ) -> Result<u32, Exception> {
            let codes = (
                u8::try_from(class_code),
                u8::try_from(subclass),
                u8::try_from(protocol),
            );
            // Class codes are bytes; nothing attached can match any other.
            let (Ok(class_code), Ok(subclass), Ok(protocol)) = codes else {
                return Err(Exception::NotReady);
            };
            let mut turn = TURNSTILE.enter(init());
            claims(&mut turn)
                .claim(badge, class_code, subclass, protocol)
                .map_err(|error| match error {
                    ClaimError::NotReady => Exception::NotReady,
                    ClaimError::Full => Exception::Busy,
                })
        }

        fn release(badge: u64, claim: u32) -> Result<(), Exception> {
            let mut turn = TURNSTILE.enter(init());
            claims(&mut turn)
                .release(badge, claim)
                .map_err(|_unknown| Exception::NoClaim)
        }

        fn interrupt_pipe(
            badge: u64,
            claim: u32,
            stage: usb::usb_v1::__runtime::IoRec,
        ) -> Result<(), Exception> {
            let mut turn = TURNSTILE.enter(init());
            let params =
                claims(&mut turn)
                    .interrupt_pipe(badge, claim)
                    .map_err(|error| match error {
                        UseError::NoClaim => Exception::NoClaim,
                        UseError::DeviceGone => Exception::DeviceGone,
                        UseError::NoEndpoint => Exception::NotPermitted,
                    })?;
            // The record goes to the start of the stage in the caller's own
            // data area.
            let (_, exported) = init()
                .area_for_badge(badge)
                .ok_or(Exception::NotPermitted)?;
            let area = DataArea {
                base: exported.base,
                len: exported.len,
            };
            if !area.contains(&stage) || stage.len < PipeParams::BYTES as u64 {
                return Err(Exception::NotPermitted);
            }
            if area.write_bytes(stage.base - area.base, &params.to_bytes()) {
                Ok(())
            } else {
                Err(Exception::NotPermitted)
            }
        }

        fn control(
            badge: u64,
            claim: u32,
            _stage: usb::usb_v1::__runtime::IoRec,
        ) -> Result<u32, Exception> {
            let mut turn = TURNSTILE.enter(init());
            // Control transfers on a claim are not served yet.
            claims(&mut turn)
                .check(badge, claim)
                .map_err(|error| match error {
                    UseError::DeviceGone => Exception::DeviceGone,
                    UseError::NoClaim | UseError::NoEndpoint => Exception::NoClaim,
                })
                .and(Err(Exception::NotPermitted))
        }

        fn trace(badge: u64, text: usb::usb_v1::__runtime::IoRec) -> Result<u32, Exception> {
            let (_, exported) = init()
                .area_for_badge(badge)
                .ok_or(Exception::NotPermitted)?;
            let area = DataArea {
                base: exported.base,
                len: exported.len,
            };
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
    }

    usb::usb_v1_export!(UsbCore);

    /// Wait `milliseconds` on the component's own `EventCount`, which nothing
    /// advances.
    fn sleep_ms(init: &DriverInit, milliseconds: u32) {
        let count = EventCountKey::from_key(RawKey::from_wire(init.event_count));
        match count.await_ge(u64::MAX, u64::from(milliseconds) * 1_000_000) {
            Ok(_) | Err(CapError::TimedOut { .. }) => {}
            Err(error) => panic!("usb-core: sleeping failed: {:?}", error.code()),
        }
    }

    fn speed_name(speed: Speed) -> &'static str {
        match speed {
            Speed::Low => "low",
            Speed::Full => "full",
            Speed::High => "high",
        }
    }

    /// What the component keeps while it runs.
    struct Core<'a> {
        init: &'static DriverInit,
        client: &'a Client,
        pipes: Pipes<'a>,
        trace: Trace,
        addresses: AddressPool,
        devices: DeviceTable,
        /// The controller's report channel, where the hubs' Status Change pipes
        /// report; without it the ports are looked at on a timer.
        reports: Option<Reports>,
        /// Hub requests that failed, for the trace's back-off.
        hub_failures: u32,
    }

    /// The client side of the controller's report channel.
    struct Reports {
        buffers: Producer<'static, SLOTS, NoEvent>,
        completed: Consumer<'static, SLOTS, EventCountKey>,
        data: libio_channel::DataArea,
        /// Advanced by the controller for every report it returns.
        event: EventCountKey,
        seen: u64,
    }

    impl Reports {
        /// The channel of `client`'s import, with its empty buffers posted.
        fn new(client: &Client) -> Option<Self> {
            let channel_area = client.channel().filter(|area| area.event != 0)?;
            // SAFETY: Kickstart mapped the channel's control page read/write.
            let channel = unsafe { ReportChannel::at(channel_area.control) };
            let event = |wire| EventCountKey::from_key(RawKey::from_wire(wire));
            let (mut buffers, completed) =
                channel.reports.client(NoEvent, event(channel_area.event));
            let data = report_data_area(channel_area.control);
            for index in 0..REPORT_BUFFERS {
                let rec = IoRec {
                    base: data.base + index * REPORT_BYTES,
                    len: REPORT_BYTES,
                };
                let packet = Packet::new(index, &[rec])?;
                buffers.put(&packet).ok()?;
            }
            Some(Self {
                buffers,
                completed,
                data,
                event: event(channel_area.event),
                seen: 0,
            })
        }
    }

    /// One downstream port of a hub, and when to look at it next.
    struct PortSlot {
        port: Port,
        /// Nanoseconds on the monotonic clock at which the port wants attention.
        wake_ns: u64,
        event: PortEvent,
        /// Whether the port is driven at all.
        active: bool,
    }

    /// A hub the core drives: what its transfers need, its control pipe and
    /// its ports.
    struct HubState {
        info: HubInfo,
        control: Pipe,
        /// The pipe of the hub's Status Change endpoint (USB 2.0 §11.12.1), whose
        /// bitmaps name the ports to look at; without it the ports are polled.
        status: Option<Pipe>,
        slots: [PortSlot; MAX_PORTS],
        ports: usize,
    }

    /// What the ports of a hub need to know about it.
    #[derive(Clone, Copy)]
    struct HubInfo {
        address: u8,
        speed: Speed,
        /// The hub and port the hub's own transfers take when it is a low- or
        /// full-speed hub behind a high-speed one; the devices behind it take
        /// the same route.
        route: Option<Route>,
        /// Whether the hub's Status Change pipe names the ports to look at, so a
        /// resting port waits for it instead of a timer.
        reports_changes: bool,
    }

    /// What running a port did beyond its own state.
    #[derive(Default)]
    struct Effects {
        /// A hub was enumerated on the port.
        started: Option<HubState>,
        /// The addresses of the devices that went away with the port's device.
        removed: Option<Removed>,
    }

    /// What a controller's `transfer_failed` status says. The bits are those of
    /// the DWC2's host channel interrupts (`HCINT`) of the last run of the
    /// failed transfer, with two marker bits above them for split transactions
    /// (see `usb-hcd-dwc2`'s `split` module).
    struct ChannelStatus(u32);

    impl core::fmt::Display for ChannelStatus {
        fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            const SPLIT: u32 = 1 << 31;
            const COMPLETE_SPLIT: u32 = 1 << 30;
            // Set on a control transfer's failure: bits 26 and 27 are then the
            // stage of the transfer that failed.
            const CONTROL_STAGE: u32 = 1 << 11;
            const EVENTS: [(u32, &str); 10] = [
                (1 << 0, "xfercompl"),
                (1 << 1, "halted"),
                (1 << 2, "ahberr"),
                (1 << 3, "stall"),
                (1 << 4, "nak"),
                (1 << 5, "ack"),
                (1 << 6, "nyet"),
                (1 << 7, "xacterr"),
                (1 << 8, "babble"),
                (1 << 9, "frmovrun"),
            ];
            write!(formatter, "{:#010x}", self.0)?;
            if self.0 & SPLIT != 0 {
                let phase = if self.0 & COMPLETE_SPLIT != 0 {
                    "complete"
                } else {
                    "start"
                };
                write!(formatter, " split/{phase}")?;
            }
            if self.0 & CONTROL_STAGE != 0 {
                let stage = match (self.0 >> 26) & 3 {
                    0 => "setup",
                    1 => "data in",
                    2 => "data out",
                    _ => "status",
                };
                write!(formatter, " {stage} stage")?;
            }
            if self.0 & SPLIT != 0 {
                write!(formatter, " after start[")?;
                for (bit, name) in EVENTS {
                    if self.0 & (bit << 12) != 0 {
                        write!(formatter, " {name}")?;
                    }
                }
                write!(
                    formatter,
                    " ] and {} nyet, {} restarts,",
                    (self.0 >> 22) & 0x7,
                    (self.0 >> 28) & 0x3
                )?;
            }
            for (bit, name) in EVENTS {
                if self.0 & bit != 0 {
                    write!(formatter, " {name}")?;
                }
            }
            if self.0 & (1 << 10) != 0 {
                write!(formatter, " datatgl")?;
            }
            Ok(())
        }
    }

    /// Trace one control transfer of an enumeration, marking those through a
    /// transaction translator (`routed`), so the first split transactions show
    /// up on the serial line.
    fn trace_transfer(
        trace: &mut Trace,
        place: core::fmt::Arguments<'_>,
        routed: bool,
        setup: SetupPacket,
        result: &Result<usize, usb_hcd_client::pipe_v1::Error>,
    ) {
        use usb_hcd_client::pipe_v1::{Error, Exception as PipeException};
        let bytes = setup.to_bytes();
        match result {
            Ok(moved) => trace!(
                trace,
                "usb-core: {place}: control {bytes:02x?}{}: {moved} bytes",
                if routed { " (split)" } else { "" }
            ),
            Err(Error::Exception(PipeException::TransferFailed { status })) => trace!(
                trace,
                "usb-core: {place}: control {bytes:02x?} failed: transfer_failed {}",
                ChannelStatus(*status)
            ),
            Err(error) => trace!(
                trace,
                "usb-core: {place}: control {bytes:02x?} failed: {error:?}"
            ),
        }
    }

    /// Where a device is attached, for the trace.
    struct Where(Attachment);

    impl core::fmt::Display for Where {
        fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            match self.0 {
                Attachment::Root => write!(formatter, "root port"),
                Attachment::Hub { hub, port } => write!(formatter, "hub {hub} port {port}"),
            }
        }
    }

    fn now_ns() -> u64 {
        libuser::clock::now_ns()
    }

    impl Core<'_> {
        /// Pause `milliseconds`.
        fn sleep(&self, milliseconds: u32) {
            sleep_ms(self.init, milliseconds);
        }

        /// Count one more failed hub request; the count when this one is
        /// traced (the 1st, 2nd, 4th, 8th...), so a request that fails on every
        /// poll adds a line per doubling.
        fn hub_failure(&mut self) -> Option<u32> {
            self.hub_failures = self.hub_failures.wrapping_add(1);
            self.hub_failures
                .is_power_of_two()
                .then_some(self.hub_failures)
        }

        /// Wait until a device is attached to the root port. The first status
        /// is traced, then the wait at 1, 2, 4, 8... seconds.
        fn wait_for_root_device(&mut self) {
            let mut looks: u32 = 0;
            loop {
                let status = self.client.port_status(0);
                if looks == 0
                    && let Ok(bits) = status
                {
                    trace!(self.trace, "usb-core: root port status {bits:#x}");
                }
                looks = looks.wrapping_add(1);
                match status {
                    Ok(bits) if bits & 1 != 0 => {
                        trace!(
                            self.trace,
                            "usb-core: root port: device connected (status {bits:#x}), enumerating"
                        );
                        return;
                    }
                    Ok(bits) => {
                        let waited_ms = looks.saturating_mul(ROOT_POLL_MS);
                        if waited_ms % 1000 == 0 && (waited_ms / 1000).is_power_of_two() {
                            trace!(
                                self.trace,
                                "usb-core: root port: no device yet after {} s (status {bits:#x})",
                                waited_ms / 1000
                            );
                        }
                        self.sleep(ROOT_POLL_MS);
                    }
                    Err(error) => {
                        trace!(
                            self.trace,
                            "usb-core: the host controller did not come up: {error:?}"
                        );
                        self.park()
                    }
                }
            }
        }

        /// Enumerate the device on the root port.
        fn enumerate_root(&mut self) -> Option<Enumerated> {
            let address = self.addresses.take()?;
            debug_assert_eq!(address, ROOT_ADDRESS);
            let client = self.client;
            let init = self.init;
            let trace = &mut self.trace;
            let routed = false;
            let result = enumerate(
                &mut self.pipes,
                address,
                None,
                || {
                    client
                        .reset_port(0)
                        .map(|reset| match reset {
                            HcdSpeed::Low => Speed::Low,
                            HcdSpeed::Full => Speed::Full,
                            HcdSpeed::High => Speed::High,
                        })
                        .map_err(|_error| EnumerateError::Reset)
                },
                |milliseconds| sleep_ms(init, milliseconds),
                |setup, result| trace_transfer(trace, format_args!("root"), routed, setup, result),
            );
            match result {
                Ok(device) => {
                    self.record(&device, Attachment::Root, None);
                    Some(device)
                }
                Err(error) => {
                    trace!(
                        self.trace,
                        "usb-core: the root device did not enumerate: {error:?}"
                    );
                    self.addresses.release(address);
                    None
                }
            }
        }

        /// Note an enumerated device in the table, offer its interfaces to
        /// claimants and trace it. `route` is the transaction translator route
        /// of a device reached with split transactions.
        fn record(&mut self, found: &Enumerated, attachment: Attachment, route: Option<Route>) {
            let configuration = found
                .configuration
                .get(..found.configuration_len)
                .unwrap_or_default();
            let route = route.map(|route| HubRoute {
                hub: route.hub_address,
                port: route.hub_port,
            });
            let (class_code, vendor, product) = match descriptor::walk(&found.device).next() {
                Some(Ok(Descriptor::Device(device))) => {
                    (device.class_code, device.vendor, device.product)
                }
                _ => (0, 0, 0),
            };
            {
                let mut turn = TURNSTILE.enter(self.init);
                let claims = claims(&mut turn);
                for offer in offers_of(found.address, found.speed, route, configuration) {
                    let offer = Offer {
                        vendor,
                        product,
                        ..offer
                    };
                    if !claims.offer(offer) {
                        trace!(self.trace, "usb-core: the interface table is full");
                        break;
                    }
                }
            }
            let interfaces = descriptor::walk(
                found
                    .configuration
                    .get(..found.configuration_len)
                    .unwrap_or_default(),
            )
            .filter(|found| matches!(found, Ok(Descriptor::Interface(_))))
            .count();
            let device = Device {
                address: found.address,
                speed: found.speed,
                attachment,
                class_code,
                vendor,
                product,
            };
            trace!(
                self.trace,
                "usb-core: {}: {} speed device at address {}: class {class_code:#04x}, \
                 vendor {vendor:#06x} product {product:#06x}, {interfaces} interfaces, \
                 control packets of {} bytes",
                Where(attachment),
                speed_name(found.speed),
                found.address,
                found.max_packet0,
            );
            if !self.devices.add(device) {
                trace!(self.trace, "usb-core: the device table is full");
            }
        }

        /// A control request to the hub through `pipe`; the data stage that came
        /// back is in `reply[..moved]`.
        fn hub_request(&self, pipe: &Pipe, setup: SetupPacket, reply: &mut [u8]) -> Option<usize> {
            self.pipes.control(pipe, setup, &[], reply).ok()
        }

        /// The hub's status for `port`.
        fn port_status(&mut self, pipe: &Pipe, port: u8) -> Option<PortStatus> {
            let mut reply = [0; 4];
            match self
                .pipes
                .control(pipe, get_port_status(port), &[], &mut reply)
            {
                Ok(4) => Some(PortStatus::from_bytes(reply)),
                Ok(moved) => {
                    if let Some(count) = self.hub_failure() {
                        trace!(
                            self.trace,
                            "usb-core: port {port}: no status ({moved} bytes; {count} hub request failures so far)"
                        );
                    }
                    None
                }
                Err(error) => {
                    if let Some(count) = self.hub_failure() {
                        trace!(
                            self.trace,
                            "usb-core: port {port}: no status: {error:?} ({count} hub request failures so far)"
                        );
                    }
                    None
                }
            }
        }

        /// Open the control pipe of the hub `info` describes.
        fn open_hub_control(&mut self, info: HubInfo, max_packet0: u16) -> Option<Pipe> {
            let pipe = match self.pipes.open(
                u32::from(info.address),
                0,
                match info.speed {
                    Speed::Low => HcdSpeed::Low,
                    Speed::Full => HcdSpeed::Full,
                    Speed::High => HcdSpeed::High,
                },
                usb_hcd_client::pipe_v1::TransferKind::Control,
                max_packet0,
            ) {
                Ok(pipe) => pipe,
                Err(error) => {
                    trace!(
                        self.trace,
                        "usb-core: no control pipe to the hub at address {}: {error:?}",
                        info.address
                    );
                    return None;
                }
            };
            if let Some(route) = info.route
                && let Err(error) = pipe
                    .client
                    .set_route(u32::from(route.hub_address), u32::from(route.hub_port))
            {
                trace!(
                    self.trace,
                    "usb-core: the hub at address {} cannot be routed: {error:?}", info.address
                );
                self.pipes.close(&pipe);
                return None;
            }
            Some(pipe)
        }

        /// Read the descriptor of the hub `hub` and take it over: its ports are
        /// powered and run from the hub loop. `route` is the route the hub's
        /// own transfers take.
        fn start_hub(&mut self, hub: &Enumerated, route: Option<Route>) -> Option<HubState> {
            let info = HubInfo {
                address: hub.address,
                speed: hub.speed,
                route,
                reports_changes: false,
            };
            let control = self.open_hub_control(info, hub.max_packet0)?;
            let mut reply = [0; MAX_HUB_DESCRIPTOR];
            let length = u16::try_from(MAX_HUB_DESCRIPTOR).unwrap_or(u16::MAX);
            let described = self
                .hub_request(&control, get_hub_descriptor(length), &mut reply)
                .and_then(|moved| HubDescriptor::parse(reply.get(..moved)?).ok());
            let Some(descriptor) = described else {
                trace!(
                    self.trace,
                    "usb-core: the descriptor of the hub at address {} could not be read",
                    hub.address
                );
                self.pipes.close(&control);
                return None;
            };
            trace!(
                self.trace,
                "usb-core: hub at address {}: {} ports, power good after {} ms, characteristics {:#06x}",
                hub.address,
                descriptor.ports,
                descriptor.power_on_to_good_ms,
                descriptor.characteristics
            );
            let ports = usize::from(descriptor.ports).min(MAX_PORTS);
            if usize::from(descriptor.ports) > MAX_PORTS {
                trace!(
                    self.trace,
                    "usb-core: only the first {MAX_PORTS} ports are driven"
                );
            }
            let status = self.open_status_pipe(info, hub);
            Some(HubState {
                info: HubInfo {
                    reports_changes: status.is_some(),
                    ..info
                },
                control,
                status,
                slots: core::array::from_fn(|index| PortSlot {
                    port: Port::new(descriptor.power_on_to_good_ms),
                    wake_ns: 0,
                    event: PortEvent::Start,
                    active: index < ports,
                }),
                ports,
            })
        }

        /// Open the pipe of `hub`'s Status Change endpoint, its interrupt-IN
        /// endpoint, when there is a report channel to read it on.
        fn open_status_pipe(&mut self, info: HubInfo, hub: &Enumerated) -> Option<Pipe> {
            self.reports.as_ref()?;
            let configuration = hub
                .configuration
                .get(..hub.configuration_len)
                .unwrap_or_default();
            let endpoint = descriptor::walk(configuration).find_map(|found| match found {
                Ok(Descriptor::Endpoint(endpoint))
                    if endpoint.is_in()
                        && endpoint.transfer_kind() == descriptor::TransferKind::Interrupt =>
                {
                    Some(endpoint)
                }
                _ => None,
            })?;
            let speed = match info.speed {
                Speed::Low => HcdSpeed::Low,
                Speed::Full => HcdSpeed::Full,
                Speed::High => HcdSpeed::High,
            };
            let pipe = self
                .pipes
                .open_unconfigured(u32::from(info.address), u32::from(endpoint.address), speed)
                .ok()?;
            let interval = interval_ms(info.speed, endpoint.interval);
            let configured = info.route.is_none_or(|route| {
                pipe.client
                    .set_route(u32::from(route.hub_address), u32::from(route.hub_port))
                    .is_ok()
            }) && pipe
                .client
                .configure(
                    usb_hcd_client::pipe_v1::TransferKind::Interrupt,
                    endpoint.max_packet,
                    interval,
                )
                .is_ok();
            if !configured {
                trace!(
                    self.trace,
                    "usb-core: hub at address {}: no Status Change pipe, its ports are polled",
                    info.address
                );
                self.pipes.close(&pipe);
                return None;
            }
            trace!(
                self.trace,
                "usb-core: hub at address {}: Status Change endpoint {:#04x} every {interval} ms",
                info.address,
                endpoint.address
            );
            Some(pipe)
        }

        /// Wake the resting ports the hubs' Status Change bitmaps name, and post
        /// their buffers again.
        fn wake_changed_ports(&mut self, hubs: &mut [Option<HubState>]) {
            let Some(reports) = self.reports.as_mut() else {
                return;
            };
            while let Ok(Some(packet)) = reports.completed.get() {
                let Some(rec) = packet.recs().first().copied() else {
                    continue;
                };
                let buffer = IoRec {
                    base: rec.base,
                    len: REPORT_BYTES,
                };
                if !reports.data.contains(&buffer) {
                    continue;
                }
                let handle = u32::try_from(packet.value()).unwrap_or(u32::MAX);
                if let Some(hub) = hubs.iter_mut().flatten().find(|hub| {
                    hub.status
                        .as_ref()
                        .is_some_and(|pipe| pipe.handle == handle)
                }) {
                    // Bit n of the bitmap is port n; bit 0 is the hub itself
                    // (USB 2.0 §11.12.4).
                    for offset in 0..rec.len.min(REPORT_BYTES) {
                        // SAFETY: the buffer lies in the channel's data page,
                        // mapped read/write; the controller gave it back.
                        let byte = unsafe {
                            core::ptr::with_exposed_provenance::<u8>(
                                usize::try_from(rec.base + offset).unwrap_or(0),
                            )
                            .read_volatile()
                        };
                        for bit in 0..8 {
                            let port = usize::try_from(offset * 8 + bit).unwrap_or(0);
                            if byte & (1 << bit) != 0
                                && (1..=hub.ports).contains(&port)
                                && hub.slots[port - 1].wake_ns == u64::MAX
                            {
                                hub.slots[port - 1].wake_ns = 0;
                            }
                        }
                    }
                }
                if let Some(again) = Packet::new(0, &[buffer]) {
                    let _posted = reports.buffers.put(&again);
                }
            }
        }

        /// Sleep until the next port's timer, or until a Status Change report
        /// arrives, whichever comes first.
        fn wait(&mut self, milliseconds: u64) {
            let Some(reports) = self.reports.as_mut() else {
                self.sleep(u32::try_from(milliseconds).unwrap_or(u32::MAX));
                return;
            };
            match reports
                .event
                .await_ge(reports.seen + 1, milliseconds.saturating_mul(1_000_000))
            {
                Ok(count) => reports.seen = count,
                Err(CapError::TimedOut { .. }) => {}
                Err(error) => panic!("usb-core: waiting for reports failed: {:?}", error.code()),
            }
        }

        /// Forget the hubs whose address is in `addresses`: their devices are gone.
        fn drop_hubs(&mut self, hubs: &mut [Option<HubState>], addresses: &[u8]) {
            for slot in hubs.iter_mut() {
                if slot
                    .as_ref()
                    .is_some_and(|hub| addresses.contains(&hub.info.address))
                    && let Some(hub) = slot.take()
                {
                    trace!(
                        self.trace,
                        "usb-core: hub at address {} is gone", hub.info.address
                    );
                    self.pipes.close(&hub.control);
                    if let Some(status) = &hub.status {
                        self.pipes.close(status);
                    }
                }
            }
        }

        /// Trace the lines the controller kept since the last look, which has no
        /// serial channel of its own.
        fn relay_controller_trace(&mut self) {
            let area = self.pipes.area();
            let Some(text) = area.reference(0, area.len) else {
                return;
            };
            let Ok(length) = self.client.trace(text) else {
                return;
            };
            // One line at a time from the area: a buffer of the whole area would
            // not fit the stack next to the hub loop.
            let length = usize::try_from(length).unwrap_or(0);
            let mut line = [0_u8; 256];
            let mut start = 0;
            while start < length {
                let take = (length - start).min(line.len());
                if !area.read_bytes(start as u64, &mut line[..take]) {
                    return;
                }
                let end = line[..take]
                    .iter()
                    .position(|&byte| byte == b'\n')
                    .unwrap_or(take);
                if end > 0 {
                    trace!(
                        self.trace,
                        "{}",
                        core::str::from_utf8(&line[..end]).unwrap_or("?")
                    );
                }
                start += end + 1;
            }
        }

        /// Drive the root hub and the hubs behind it.
        fn run_hubs(&mut self, root: HubState) -> ! {
            let mut hubs: [Option<HubState>; MAX_HUBS] = core::array::from_fn(|_| None);
            hubs[0] = Some(root);
            let mut reported = false;
            loop {
                self.relay_controller_trace();
                let now = now_ns();
                for which in 0..MAX_HUBS {
                    let Some(mut hub) = hubs[which].take() else {
                        continue;
                    };
                    let mut effects = Effects::default();
                    for index in 0..hub.ports {
                        if !hub.slots[index].active || hub.slots[index].wake_ns > now {
                            continue;
                        }
                        let number = u8::try_from(index + 1).unwrap_or(u8::MAX);
                        // Only one device on the whole bus may be waiting for its
                        // address at a time, behind whichever hub.
                        let default_address_taken =
                            hub.slots.iter().enumerate().any(|(other, slot)| {
                                other != index && slot.port.holds_default_address()
                            }) || hubs.iter().flatten().any(|other| {
                                other
                                    .slots
                                    .iter()
                                    .any(|slot| slot.port.holds_default_address())
                            });
                        let (info, control) = (hub.info, &hub.control);
                        self.step_port(
                            info,
                            control,
                            number,
                            &mut hub.slots[index],
                            default_address_taken,
                            &mut effects,
                        );
                    }
                    hubs[which] = Some(hub);
                    if let Some(removed) = effects.removed {
                        self.drop_hubs(&mut hubs, removed.addresses());
                    }
                    if let Some(started) = effects.started {
                        match hubs.iter_mut().find(|slot| slot.is_none()) {
                            Some(slot) => *slot = Some(started),
                            None => {
                                trace!(self.trace, "usb-core: too many hubs, not driving one");
                                self.pipes.close(&started.control);
                            }
                        }
                    }
                }
                if !reported
                    && hubs.iter().flatten().all(|hub| {
                        hub.slots
                            .iter()
                            .take(hub.ports)
                            .all(|slot| slot.port.settled())
                    })
                {
                    reported = true;
                    let behind = self.devices.len().saturating_sub(1);
                    trace!(
                        self.trace,
                        "usb-core: every port of the hub has settled: {behind} devices behind it"
                    );
                    if behind > 0 {
                        #[cfg(feature = "qemu")]
                        self.report();
                    }
                }
                let next = hubs
                    .iter()
                    .flatten()
                    .flat_map(|hub| hub.slots.iter())
                    .filter(|slot| slot.active)
                    .map(|slot| slot.wake_ns)
                    .min();
                // Resting ports wait for the hubs' reports, at most a second at
                // a time.
                let wait = next.map_or(u64::from(ROOT_POLL_MS), |wake| {
                    wake.saturating_sub(now_ns())
                        .div_ceil(1_000_000)
                        .clamp(1, 1000)
                });
                self.wait(wait);
                self.wake_changed_ports(&mut hubs);
            }
        }

        /// Run `slot`'s port until it waits.
        ///
        /// `default_address_taken` is whether another port of the hub has a device
        /// that may still answer at address 0; this port's reset waits for it.
        fn step_port(
            &mut self,
            hub: HubInfo,
            control: &Pipe,
            number: u8,
            slot: &mut PortSlot,
            default_address_taken: bool,
            effects: &mut Effects,
        ) {
            let mut event = slot.event;
            loop {
                let was_failed = slot.port.failed();
                let action = slot.port.next(event);
                if was_failed && !slot.port.failed() {
                    // A device that never enumerated holds nothing to release.
                    trace!(
                        self.trace,
                        "usb-core: hub {} port {number}: unusable device unplugged", hub.address
                    );
                }
                event = match action {
                    PortAction::Idle => {
                        slot.event = PortEvent::Delayed;
                        slot.wake_ns = now_ns();
                        slot.active = false;
                        return;
                    }
                    PortAction::Failed => {
                        trace!(
                            self.trace,
                            "usb-core: hub {} port {number}: unusable until the device is unplugged",
                            hub.address
                        );
                        // The port keeps being looked at, to see the unplugging.
                        slot.event = PortEvent::Delayed;
                        slot.wake_ns = now_ns() + FAILED_POLL_MS * 1_000_000;
                        return;
                    }
                    PortAction::Wait { milliseconds } => {
                        slot.event = PortEvent::Delayed;
                        // A resting port that was just looked at waits for the hub to
                        // name it; any other wait is a timer.
                        slot.wake_ns = if hub.reports_changes
                            && slot.port.waits_for_change()
                            && matches!(event, PortEvent::Status(_))
                        {
                            u64::MAX
                        } else {
                            now_ns() + u64::from(milliseconds) * 1_000_000
                        };
                        return;
                    }
                    PortAction::PowerOn => {
                        self.set_feature(control, number, PORT_POWER, "power");
                        PortEvent::Done
                    }
                    PortAction::PollStatus => match self.port_status(control, number) {
                        Some(status) => PortEvent::Status(status),
                        None => {
                            // Try again later; a Delayed event polls in every
                            // state that waits to poll.
                            slot.event = PortEvent::Delayed;
                            slot.wake_ns = now_ns() + 100_000_000;
                            return;
                        }
                    },
                    PortAction::ClearChange { feature } => {
                        let setup = clear_port_feature(number, feature);
                        if self.hub_request(control, setup, &mut []).is_none()
                            && let Some(count) = self.hub_failure()
                        {
                            trace!(
                                self.trace,
                                "usb-core: port {number}: clearing {feature} failed ({count} hub request failures so far)"
                            );
                        }
                        PortEvent::Done
                    }
                    PortAction::Reset if default_address_taken => {
                        slot.port.defer_reset();
                        slot.event = PortEvent::Delayed;
                        slot.wake_ns = now_ns() + 10_000_000;
                        return;
                    }
                    PortAction::Reset => {
                        trace!(
                            self.trace,
                            "usb-core: port {number}: device connected, resetting"
                        );
                        self.set_feature(control, number, PORT_RESET, "reset");
                        PortEvent::Done
                    }
                    PortAction::Enumerate { speed } => {
                        let ok = self.enumerate_behind(hub, number, speed, effects);
                        PortEvent::Enumerated { ok }
                    }
                    PortAction::Teardown => {
                        let removed = self.devices.remove_port(hub.address, number);
                        {
                            let mut turn = TURNSTILE.enter(self.init);
                            let claims = claims(&mut turn);
                            for &address in removed.addresses() {
                                claims.withdraw(address);
                            }
                        }
                        for &address in removed.addresses() {
                            self.addresses.release(address);
                        }
                        // The port status that ended the device, to tell an unplug from a
                        // device that dropped off the bus by itself.
                        let (status, change) = match event {
                            PortEvent::Status(seen) => (seen.status, seen.change),
                            _ => (0, 0),
                        };
                        trace!(
                            self.trace,
                            "usb-core: hub {} port {number}: device gone ({} addresses released, port status {status:#06x} change {change:#06x})",
                            hub.address,
                            removed.addresses().len()
                        );
                        effects.removed = Some(removed);
                        PortEvent::Done
                    }
                };
            }
        }

        /// `SET_FEATURE` on a port; a failure is traced.
        fn set_feature(&mut self, control: &Pipe, number: u8, feature: u16, what: &str) {
            let setup = set_port_feature(number, feature);
            if self.hub_request(control, setup, &mut []).is_none()
                && let Some(count) = self.hub_failure()
            {
                trace!(
                    self.trace,
                    "usb-core: port {number}: {what} failed ({count} hub request failures so far)"
                );
            }
        }

        /// Enumerate the device behind port `number` of `hub`, which is reset
        /// and runs at `speed`. A low- or full-speed device behind a
        /// high-speed hub is reached with split transactions.
        fn enumerate_behind(
            &mut self,
            hub: HubInfo,
            number: u8,
            speed: Speed,
            effects: &mut Effects,
        ) -> bool {
            let Some(address) = self.addresses.take() else {
                trace!(
                    self.trace,
                    "usb-core: hub {} port {number}: no device address left", hub.address
                );
                return false;
            };
            // The transaction translator of a high-speed hub serves the low- and
            // full-speed devices behind it; those behind a low- or full-speed hub
            // go through the translator that hub itself goes through.
            let route = if hub.speed == Speed::High {
                (speed != Speed::High).then_some(Route {
                    hub_address: hub.address,
                    hub_port: number,
                })
            } else {
                hub.route
            };
            trace!(
                self.trace,
                "usb-core: hub {} port {number}: enumerating a {} speed device as address {address}{}",
                hub.address,
                speed_name(speed),
                if route.is_some() {
                    " through the hub's transaction translator (split transactions)"
                } else {
                    ""
                }
            );
            let init = self.init;
            let trace = &mut self.trace;
            let routed = route.is_some();
            let result = enumerate(
                &mut self.pipes,
                address,
                route,
                || Ok(speed),
                |milliseconds| sleep_ms(init, milliseconds),
                |setup, result| {
                    trace_transfer(
                        trace,
                        format_args!("hub {} port {number}", hub.address),
                        routed,
                        setup,
                        result,
                    );
                },
            );
            match result {
                Ok(device) => {
                    self.record(
                        &device,
                        Attachment::Hub {
                            hub: hub.address,
                            port: number,
                        },
                        route,
                    );
                    let is_hub = descriptor::walk(&device.device).next().is_some_and(
                        |found| matches!(found, Ok(Descriptor::Device(d)) if d.class_code == CLASS_HUB),
                    );
                    if is_hub {
                        effects.started = self.start_hub(&device, route);
                    }
                    true
                }
                Err(error) => {
                    trace!(
                        self.trace,
                        "usb-core: hub {} port {number}: enumeration failed: {error:?}",
                        hub.address
                    );
                    self.addresses.release(address);
                    false
                }
            }
        }

        /// Signal the boot test's Notification. Only the QEMU boot tests install
        /// one, so only images built with the `qemu` feature have this.
        #[cfg(feature = "qemu")]
        fn report(&self) {
            let key = libobject::NotificationKey::from_key(libdevice_driver::local_key(
                self.init.table,
                REPORT_SLOT,
            ));
            let _signalled = key.signal(0);
        }

        /// Nothing more to do: sleep for good.
        fn park(&mut self) -> ! {
            loop {
                self.sleep(1000);
            }
        }
    }

    fn main(_init_va: u64) -> ! {
        let init = init();
        let mut trace = Trace::new(init);
        trace!(trace, "usb-core: starting");
        let bindings = Bindings::from_imports(init.imports());
        let client = Client::new(bindings);
        let area = client
            .data_area()
            .expect("usb-core shares a data area with the controller");
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
            .unwrap_or_else(|error| panic!("usb-core: turns Retype failed: {:?}", error.code()));
        TURNSTILE.open(turns);
        // Say the exports can be served.
        EventCountKey::from_key(RawKey::from_wire(init.event_count))
            .advance(1)
            .unwrap_or_else(|error| panic!("usb-core: ready advance failed: {:?}", error.code()));
        let pipes = Pipes::new(&client, area, table, turns_slot + 1);
        let mut core = Core {
            init,
            client: &client,
            pipes,
            trace,
            addresses: AddressPool::new(),
            devices: DeviceTable::new(),
            reports: Reports::new(&client),
            hub_failures: 0,
        };
        core.wait_for_root_device();
        let Some(root) = core.enumerate_root() else {
            core.park()
        };
        let is_hub = descriptor::walk(&root.device).next().is_some_and(
            |found| matches!(found, Ok(Descriptor::Device(d)) if d.class_code == CLASS_HUB),
        );
        if is_hub {
            let Some(hub) = core.start_hub(&root, None) else {
                core.park()
            };
            core.run_hubs(hub)
        }
        trace!(core.trace, "usb-core: the root device is not a hub");
        core.park()
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}

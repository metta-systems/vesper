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
//! enumeration, teardown), polled with `GET_STATUS` until interrupt pipes
//! exist. A low- or full-speed device behind a high-speed hub is reached with
//! split transactions: its pipes carry the hub and port as their route.
//!
//! Progress is traced on the serial channel when the component has one
//! (`serial = true` in its manifest entry), and on the semihosting console
//! under QEMU.
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
        libdevice_driver::{DriverInit, INIT_VA, Turn, Turnstile, driver_init, local_key},
        libmeddle_runtime::DataArea,
        libobject::{
            CapError, EventCountKey, KeyTableKey, NotificationKey, ObjectType, RawKey, Rights,
            UntypedKey, export::init_return_key, thread::ThreadReturnKey,
        },
        usb::usb_v1::{Exception, UsbV1},
        usb_core::{
            claims::{ClaimError, Claims, HubRoute, Offer, PipeParams, UseError, offers_of},
            devices::{AddressPool, Attachment, Device, DeviceTable},
        },
        usb_hcd_client::usb_hcd_v1::{Bindings, Client, Speed as HcdSpeed},
        usb_host::{
            enumerate::{EnumerateError, Enumerated, Route, enumerate},
            pipes::{Pipe, Pipes},
        },
        vesper_usb_protocol::{
            descriptor::{self, Descriptor},
            enumeration::Speed,
            hub::{
                CLASS_HUB, HubDescriptor, MAX_HUB_DESCRIPTOR, PORT_POWER, PORT_RESET, Port,
                PortAction, PortEvent, PortStatus, clear_port_feature, get_hub_descriptor,
                get_port_status, set_port_feature,
            },
            setup::SetupPacket,
        },
    };

    libuser::entry!(main);

    /// Address the root device is enumerated to.
    const ROOT_ADDRESS: u8 = 1;
    /// Ports of a hub the component drives.
    const MAX_PORTS: usize = 8;
    /// Slot of the component's table where a boot test installs a Notification;
    /// the component signals it once every port of the hub has settled and a
    /// device sits behind the hub.
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

    /// What a controller's `transfer_failed` status says. The bits are those of
    /// the DWC2's host channel interrupts (`HCINT`) of the last run of the
    /// failed transfer, with two marker bits above them for split transactions
    /// (see `usb-hcd-dwc2`'s `split` module).
    struct ChannelStatus(u32);

    impl core::fmt::Display for ChannelStatus {
        fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            const SPLIT: u32 = 1 << 31;
            const COMPLETE_SPLIT: u32 = 1 << 30;
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

        /// Wait until a device is attached to the root port.
        fn wait_for_root_device(&mut self) {
            while self.client.port_status(0).map_or(0, |bits| bits & 1) == 0 {
                self.sleep(ROOT_POLL_MS);
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
        fn port_status(&self, pipe: &Pipe, port: u8) -> Option<PortStatus> {
            let mut reply = [0; 4];
            match self.hub_request(pipe, get_port_status(port), &mut reply) {
                Some(4) => Some(PortStatus::from_bytes(reply)),
                _ => None,
            }
        }

        /// Run the hub driver for the hub at `hub`.
        fn run_hub(&mut self, hub: &Enumerated) -> ! {
            let control = match self.pipes.open(
                u32::from(hub.address),
                0,
                match hub.speed {
                    Speed::Low => HcdSpeed::Low,
                    Speed::Full => HcdSpeed::Full,
                    Speed::High => HcdSpeed::High,
                },
                usb_hcd_client::pipe_v1::TransferKind::Control,
                hub.max_packet0,
            ) {
                Ok(pipe) => pipe,
                Err(error) => {
                    trace!(
                        self.trace,
                        "usb-core: no control pipe to the hub: {error:?}"
                    );
                    self.park()
                }
            };
            let mut reply = [0; MAX_HUB_DESCRIPTOR];
            let length = u16::try_from(MAX_HUB_DESCRIPTOR).unwrap_or(u16::MAX);
            let described = self
                .hub_request(&control, get_hub_descriptor(length), &mut reply)
                .and_then(|moved| HubDescriptor::parse(reply.get(..moved)?).ok());
            let Some(descriptor) = described else {
                trace!(
                    self.trace,
                    "usb-core: the hub's descriptor could not be read"
                );
                self.park()
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
            let mut reported = false;
            let mut slots: [PortSlot; MAX_PORTS] = core::array::from_fn(|index| PortSlot {
                port: Port::new(descriptor.power_on_to_good_ms),
                wake_ns: 0,
                event: PortEvent::Start,
                active: index < ports,
            });
            loop {
                let now = now_ns();
                for (index, slot) in slots.iter_mut().enumerate() {
                    if slot.active && slot.wake_ns <= now {
                        let number = u8::try_from(index + 1).unwrap_or(u8::MAX);
                        self.step_port(hub, &control, number, slot);
                    }
                }
                if !reported && slots.iter().take(ports).all(|slot| slot.port.settled()) {
                    reported = true;
                    let behind = self.devices.len().saturating_sub(1);
                    trace!(
                        self.trace,
                        "usb-core: every port of the hub has settled: {behind} devices behind it"
                    );
                    if behind > 0 {
                        self.report();
                    }
                }
                let next = slots
                    .iter()
                    .filter(|slot| slot.active)
                    .map(|slot| slot.wake_ns)
                    .min();
                let wait = next.map_or(u64::from(ROOT_POLL_MS), |wake| {
                    wake.saturating_sub(now_ns()).div_ceil(1_000_000).max(1)
                });
                self.sleep(u32::try_from(wait).unwrap_or(u32::MAX));
            }
        }

        /// Run `slot`'s port until it waits.
        fn step_port(&mut self, hub: &Enumerated, control: &Pipe, number: u8, slot: &mut PortSlot) {
            let mut event = slot.event;
            loop {
                let action = slot.port.next(event);
                event = match action {
                    PortAction::Idle => {
                        slot.event = PortEvent::Delayed;
                        slot.wake_ns = now_ns();
                        slot.active = false;
                        return;
                    }
                    PortAction::Failed => {
                        trace!(self.trace, "usb-core: port {number}: unusable");
                        slot.active = false;
                        return;
                    }
                    PortAction::Wait { milliseconds } => {
                        slot.event = PortEvent::Delayed;
                        slot.wake_ns = now_ns() + u64::from(milliseconds) * 1_000_000;
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
                            trace!(self.trace, "usb-core: port {number}: no status");
                            slot.event = PortEvent::Delayed;
                            slot.wake_ns = now_ns() + 100_000_000;
                            return;
                        }
                    },
                    PortAction::ClearChange { feature } => {
                        let setup = clear_port_feature(number, feature);
                        if self.hub_request(control, setup, &mut []).is_none() {
                            trace!(
                                self.trace,
                                "usb-core: port {number}: clearing {feature} failed"
                            );
                        }
                        PortEvent::Done
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
                        let ok = self.enumerate_behind(hub, number, speed);
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
                        trace!(
                            self.trace,
                            "usb-core: port {number}: device gone ({} addresses released)",
                            removed.addresses().len()
                        );
                        PortEvent::Done
                    }
                };
            }
        }

        /// `SET_FEATURE` on a port; a failure is traced.
        fn set_feature(&mut self, control: &Pipe, number: u8, feature: u16, what: &str) {
            let setup = set_port_feature(number, feature);
            if self.hub_request(control, setup, &mut []).is_none() {
                trace!(self.trace, "usb-core: port {number}: {what} failed");
            }
        }

        /// Enumerate the device behind port `number` of `hub`, which is reset
        /// and runs at `speed`. A low- or full-speed device behind a
        /// high-speed hub is reached with split transactions.
        fn enumerate_behind(&mut self, hub: &Enumerated, number: u8, speed: Speed) -> bool {
            let Some(address) = self.addresses.take() else {
                trace!(
                    self.trace,
                    "usb-core: port {number}: no device address left"
                );
                return false;
            };
            let route = (hub.speed == Speed::High && speed != Speed::High).then_some(Route {
                hub_address: hub.address,
                hub_port: number,
            });
            trace!(
                self.trace,
                "usb-core: port {number}: enumerating a {} speed device as address {address}{}",
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
                    trace_transfer(trace, format_args!("port {number}"), routed, setup, result);
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
                    if device.configuration_len > 0
                        && descriptor::walk(&device.device)
                            .next()
                            .is_some_and(|found| {
                                matches!(found, Ok(Descriptor::Device(d)) if d.class_code == CLASS_HUB)
                            })
                    {
                        trace!(self.trace, "usb-core: port {number}: nested hubs are not driven yet");
                    }
                    true
                }
                Err(error) => {
                    trace!(
                        self.trace,
                        "usb-core: port {number}: enumeration failed: {error:?}"
                    );
                    self.addresses.release(address);
                    false
                }
            }
        }

        /// Signal the boot test's Notification, if it installed one.
        fn report(&self) {
            let key = NotificationKey::from_key(local_key(self.init.table, REPORT_SLOT));
            // Nobody installs a key outside a boot test.
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
        };
        core.wait_for_root_device();
        let Some(root) = core.enumerate_root() else {
            core.park()
        };
        let is_hub = descriptor::walk(&root.device).next().is_some_and(
            |found| matches!(found, Ok(Descriptor::Device(d)) if d.class_code == CLASS_HUB),
        );
        if is_hub {
            core.run_hub(&root)
        }
        trace!(core.trace, "usb-core: the root device is not a hub");
        core.park()
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}

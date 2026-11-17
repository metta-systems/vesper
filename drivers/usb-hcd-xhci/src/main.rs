//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

// Host tests build this binary natively too; it is only an empty `main` there.
#![cfg_attr(target_os = "vesper", no_std, no_main)]

//! EL0 driver component for an xHCI USB host controller behind PCIe: the
//! Raspberry Pi 4's VL805 (`1106:3483`), whose device-tree node (`usb@1,0`,
//! with no compatible string) the manifest names by path.
//!
//! Bring-up, in the order Linux's `xhci-pci` probe and the xHCI
//! specification's §4.2 have it:
//!
//! 1. Ask the PCIe host bridge driver for the function's address
//!    (`pci_v1.address`), and tell the firmware the controller was reset
//!    (`mailbox_v1.property`, tag `NOTIFY_XHCI_RESET`), which loads the
//!    VL805's firmware on boards without its SPI EEPROM; then give it a
//!    millisecond to start.
//! 2. Claim the function (`pci_v1.claim`): its BAR0 Frames, memory decoding,
//!    bus mastering, and its MSI pointed at the bridge, which signals this
//!    driver's Notification.
//! 3. Wait for `CNR` to clear, halt, reset, and set up the Device Context Base
//!    Address Array (with the scratchpad buffers `HCSPARAMS2` asks for), the
//!    command ring and one event ring on interrupter 0, then run.
//! 4. Send a No Op command and wait for its completion event, on the MSI.
//! 5. Power the USB 2 root ports, wait for a device to connect, and reset
//!    each connected one.
//!
//! Every step is traced as an `xhci:` line. The rings, the contexts, the
//! control transfers' data page and the scratchpad buffers are pages of the
//! driver's own memory, mapped cacheable: the driver cleans what it wrote
//! before the controller reads it, cleans a page the controller will write
//! before it does, and invalidates what the controller wrote before it reads
//! it.
//!
//! The component then serves `usb_hcd_v1` on the USB 2 root ports, in the
//! order the Supported Protocol capability lists them (port 0 is the first).
//! `address_device` takes a device slot (Enable Slot), gives it an Input
//! Context with the device's route string, root port and transaction
//! translator and a transfer ring for endpoint 0, and has the controller
//! address the device (Address Device); `mark_hub` records a hub's ports and
//! think time in its slot (Configure Endpoint, as Linux does on controllers
//! after version 0.95); `release_device` disables the slot. A control pipe
//! runs each transfer as a Setup, a Data and a Status Stage TRB on endpoint
//! 0's ring, the data stage through the driver's data page; configuring it
//! with a new packet size updates endpoint 0 (Evaluate Context). A stalled or
//! failed transfer leaves the endpoint halted, and the driver resets it and
//! moves its dequeue pointer past the transfer before it returns.
//!
//! A call runs on the caller's Thread, one at a time (a `Turnstile`), and
//! reads the event ring itself for the completion it waits on: it checks
//! every 100 µs for the first millisecond, then once a millisecond with the
//! Thread parked. The component's own Thread drains the event ring on each
//! MSI, under the same turnstile, and traces the port changes it reports.
//!
//! Implementation status: only endpoint 0 is served: `open_pipe` raises
//! `not_permitted` for any other endpoint, and `configure` for an interrupt
//! or bulk pipe (interrupt-IN is the next xHCI slice). `set_route` is
//! accepted and ignored: the slot carries the translator. A failed
//! transfer's `transfer_failed` status is the xHCI completion code.
//! `SuperSpeed` ports are not served.

/// `note!("text {value}")`: one trace line, on the semihosting console and on
/// the log channel when the image has a log server, else in the backlog
/// `usb_hcd_v1.trace` drains.
#[cfg(target_os = "vesper")]
macro_rules! note {
    ($($argument:tt)*) => {
        libserial_trace::note(format_args!($($argument)*))
    };
}

#[cfg(target_os = "vesper")]
mod component {
    use {
        aarch64_cpu::registers::{CNTFRQ_EL0, CNTVCT_EL0, Read},
        core::cell::UnsafeCell,
        libdevice_driver::{
            DEVICE_WINDOW_VA, DriverInit, INIT_VA, MAX_DEVICE_PAGES, Masters, NO_BUS_ADDRESS, Turn,
            Turnstile, device_page_va, driver_init,
        },
        libobject::{
            AddressSpaceKey, CapError, EventCountKey, FrameAttribute, FrameKey, KeySlot,
            KeyTableKey, NotificationKey, ObjectType, RawKey, Rights, StackPoolHandle, UntypedKey,
            export::init_return_key, thread::ThreadReturnKey,
        },
        mailbox_client::mailbox_v1::{Bindings as MailboxBindings, Client as Mailbox},
        pci::claimed,
        pci_client::pci_v1::{Bindings as PciBindings, Client as Pci},
        usb_hcd::{
            pipe_v1::{self, Exception as PipeException, PipeV1, TransferKind},
            usb_hcd_v1::{
                __runtime::{DataArea, IoRec},
                Exception, Speed, UsbHcdV1,
            },
        },
        usb_hcd_xhci::{
            context::{
                ADD_ENDPOINT0, ADD_SLOT, Hub, Position, Slot3, endpoint0_context, slot_context,
            },
            pipes::{Pipe, PipeError, PipeTable},
            regs::{
                Capabilities, Command, CommandRingControl, Configure, DequeueLow, INTERRUPTER_0,
                Interrupter, InterrupterManagement, MmioCapabilities, MmioInterrupter,
                MmioOperational, MmioPort, Operational, PAGE_SIZE_4K, Port, Ports, Status, neutral,
                port_offset, port_speed, port_status_bits, protocol_ports, split_address,
            },
            ring::{Consumer, Placement, Producer},
            trb::{
                ENDPOINT0, SUCCESS, TRB_BYTES, TransferType, Trb, code, kind, segment_table_entry,
            },
        },
        vesper_usb_protocol::{
            enumeration::{Speed as UsbSpeed, default_max_packet0},
            setup::SetupPacket,
        },
    };

    libuser::entry!(main);

    /// Bytes of a page, of the controller's (`PAGESIZE` must offer 4 KiB) and
    /// of the driver's.
    const PAGE: u64 = 4096;
    /// TRBs of a ring segment: one page.
    const RING_TRBS: usize = (PAGE / TRB_BYTES) as usize;
    /// Most scratchpad buffers the driver provides.
    const MAX_SCRATCHPADS: usize = 32;
    /// Devices the driver addresses at once.
    const MAX_DEVICES: usize = 8;
    /// Pages of each device: its Output Device Context, its Input Context
    /// and endpoint 0's transfer ring.
    const DEVICE_PAGES: usize = 3;
    /// The DMA pages, by index: the context page (the Device Context Base
    /// Address Array, the Event Ring Segment Table and the scratchpad buffer
    /// array), the command ring, the event ring, the control transfers' data
    /// page, each device's pages, then the scratchpad buffers.
    const CONTEXT_PAGE: usize = 0;
    const COMMAND_PAGE: usize = 1;
    const EVENT_PAGE: usize = 2;
    const DATA_PAGE: usize = 3;
    const FIRST_DEVICE_PAGE: usize = 4;
    const FIRST_SCRATCHPAD_PAGE: usize = FIRST_DEVICE_PAGE + MAX_DEVICES * DEVICE_PAGES;
    const DMA_PAGES: usize = FIRST_SCRATCHPAD_PAGE + MAX_SCRATCHPADS;
    /// Offsets in the context page. The array has room for 255 slots and
    /// the scratchpad buffer array (entry 0's target) for every buffer.
    const SEGMENT_TABLE_OFFSET: u64 = 2048;
    const SCRATCHPAD_ARRAY_OFFSET: u64 = 3072;
    const _: () = assert!(SCRATCHPAD_ARRAY_OFFSET + MAX_SCRATCHPADS as u64 * 8 <= PAGE);
    /// Where the DMA pages are mapped: past the device window's BAR pages.
    const DMA_VA: u64 = DEVICE_WINDOW_VA + MAX_DEVICE_PAGES as u64 * PAGE;
    /// Bytes of data stage a control transfer moves at most: what a stage in
    /// the client's data area holds after its setup packet.
    const DATA_CAPACITY: usize = 4032;
    /// Bytes copied at a time between a client's data area and the data page.
    const COPY_BYTES: usize = 64;

    /// Size and guard of the table the claim's BAR Frames go into.
    const FRAME_TABLE_BITS: u8 = 6;
    const FRAME_TABLE_GUARD: u32 = 0x0_BA25;
    /// Slots of the driver's own table, from its first free slot: the Frame
    /// table, the turnstile's `EventCount`, the DMA pages, the copies of the
    /// BAR Frames it maps, then the masters of the pipe keys.
    const FRAME_TABLE_SLOT: u64 = 0;
    const TURNS_SLOT: u64 = 1;
    const FIRST_DMA_SLOT: u64 = 2;
    const FIRST_BAR_SLOT: u64 = FIRST_DMA_SLOT + DMA_PAGES as u64;
    const MASTERS_SLOT: u64 = FIRST_BAR_SLOT + MAX_DEVICE_PAGES as u64;

    /// The firmware's property tag telling it the VL805 was reset, with the
    /// function's address as its one value word (Raspberry Pi kernel's
    /// `RPI_FIRMWARE_NOTIFY_XHCI_RESET`).
    const NOTIFY_XHCI_RESET: u32 = 0x0003_0058;
    /// How long the controller waits for a device after powering the ports.
    const CONNECT_MS: u64 = 2000;
    /// How long a command may take before the driver gives up on it.
    const COMMAND_MS: u64 = 100;
    /// How long a control transfer may take. A device may NAK a data stage
    /// for up to 5 s (USB 2.0 §9.2.6.4); enumeration and hub requests answer
    /// far sooner.
    const TRANSFER_MS: u64 = 1000;

    /// Ticks of the virtual counter, which EL0 may read.
    fn ticks() -> u64 {
        CNTVCT_EL0.get()
    }

    /// The frequency of the virtual counter in Hz, the Raspberry Pi's
    /// 19.2 MHz where the firmware left it zero.
    fn counter_frequency() -> u64 {
        match CNTFRQ_EL0.get() {
            0 => 19_200_000,
            frequency => frequency,
        }
    }

    fn now_us() -> u64 {
        ticks().saturating_mul(1_000_000) / counter_frequency()
    }

    /// Park the calling Thread for `milliseconds`: a timed wait on the
    /// component's own `EventCount` for a count it never reaches.
    fn delay_ms(milliseconds: u64) {
        EventCountKey::from_key(RawKey::from_wire(init().event_count))
            .sleep(milliseconds.max(1) * 1_000_000)
            .unwrap_or_else(|error| panic!("usb-hcd-xhci: sleep failed: {:?}", error.code()));
    }

    /// Whether `done` held within `milliseconds`, checked every 100 µs for
    /// the first millisecond, then once a millisecond with the Thread parked.
    fn wait_until(milliseconds: u64, mut done: impl FnMut() -> bool) -> bool {
        let start = now_us();
        loop {
            if done() {
                return true;
            }
            let elapsed = now_us() - start;
            if elapsed >= milliseconds * 1000 {
                return false;
            }
            if elapsed < 1000 {
                core::hint::spin_loop();
            } else {
                delay_ms(1);
            }
        }
    }

    /// A step of the bring-up. Its number is what `usb_hcd_v1`'s
    /// `controller_failed` reports when the step fails.
    #[derive(Clone, Copy, Debug)]
    enum BringUp {
        NoFunction = 1,
        FirmwareLoad = 2,
        Claim = 3,
        NotReady = 4,
        Halt = 5,
        Reset = 6,
        PageSize = 7,
        Scratchpads = 8,
        Run = 9,
    }

    fn init() -> &'static DriverInit {
        // SAFETY: Kickstart maps every driver's init record at `INIT_VA`; it
        // is read in this component's own `AddressSpace` only.
        unsafe { driver_init(INIT_VA) }
    }

    fn table(init: &DriverInit) -> KeyTableKey {
        KeyTableKey::from_key(RawKey::from_wire(init.table))
    }

    fn slot(init: &DriverInit, offset: u64) -> u32 {
        u32::try_from(init.first_free_slot + offset).expect("a slot fits in u32")
    }

    /// One page of the driver's memory the controller reads or writes.
    struct DmaPage {
        frame: FrameKey,
        va: u64,
        bus: u64,
    }

    impl DmaPage {
        /// Retype page `index` of the driver's memory, map it cacheable at
        /// its place after `DMA_VA` and zero it.
        fn new(init: &DriverInit, index: usize) -> Self {
            let frame = FrameKey::from_key(
                UntypedKey::from_key(RawKey::from_wire(init.memory))
                    .retype(
                        ObjectType::FRAME,
                        12,
                        0,
                        1,
                        &table(init),
                        slot(init, FIRST_DMA_SLOT + index as u64),
                        Rights(Rights::READ | Rights::WRITE | Rights::GRANT),
                    )
                    .unwrap_or_else(|error| {
                        panic!("usb-hcd-xhci: DMA frame Retype failed: {:?}", error.code())
                    }),
            );
            let (paddr, _size) = frame.get_extent().unwrap_or_else(|error| {
                panic!("usb-hcd-xhci: DMA frame extent: {:?}", error.code())
            });
            let va = DMA_VA + index as u64 * PAGE;
            frame
                .map(
                    RawKey::from_wire(init.address_space),
                    va,
                    Rights(Rights::READ | Rights::WRITE),
                    FrameAttribute::Default,
                )
                .unwrap_or_else(|error| {
                    panic!("usb-hcd-xhci: DMA frame Map failed: {:?}", error.code())
                });
            let bus = paddr
                .checked_sub(init.memory_paddr)
                .and_then(|offset| init.memory_bus.checked_add(offset))
                .expect("the DMA frame lies in the driver's memory");
            let page = Self { frame, va, bus };
            page.zero();
            page
        }

        fn zero(&self) {
            for offset in (0..PAGE).step_by(4) {
                self.write(offset, 0);
            }
        }

        fn word(&self, offset: u64) -> *mut u32 {
            core::ptr::with_exposed_provenance_mut::<u32>(
                usize::try_from(self.va + offset).expect("a 64-bit address"),
            )
        }

        fn byte(&self, offset: u64) -> *mut u8 {
            core::ptr::with_exposed_provenance_mut::<u8>(
                usize::try_from(self.va + offset).expect("a 64-bit address"),
            )
        }

        fn write(&self, offset: u64, value: u32) {
            // SAFETY: the page is mapped read/write for the driver's lifetime
            // and `offset` is a word inside it.
            unsafe { self.word(offset).write_volatile(value) }
        }

        fn read(&self, offset: u64) -> u32 {
            // SAFETY: as in `write`.
            unsafe { self.word(offset).read_volatile() }
        }

        fn write_words(&self, offset: u64, words: &[u32]) {
            for (index, &word) in words.iter().enumerate() {
                self.write(offset + index as u64 * 4, word);
            }
        }

        fn write_bytes(&self, offset: u64, bytes: &[u8]) {
            for (index, &byte) in bytes.iter().enumerate() {
                // SAFETY: as in `write`; the caller keeps `bytes` inside the
                // page.
                unsafe { self.byte(offset + index as u64).write_volatile(byte) }
            }
        }

        fn read_bytes(&self, offset: u64, bytes: &mut [u8]) {
            for (index, byte) in bytes.iter_mut().enumerate() {
                // SAFETY: as in `write_bytes`.
                *byte = unsafe { self.byte(offset + index as u64).read_volatile() };
            }
        }

        /// Write `trb` at TRB `index`, the control word (with the cycle
        /// bit) last.
        fn write_trb(&self, index: usize, trb: Trb) {
            self.write_words(index as u64 * TRB_BYTES, &trb.words());
        }

        fn read_trb(&self, index: usize) -> Trb {
            let offset = index as u64 * TRB_BYTES;
            Trb::from_words([
                self.read(offset),
                self.read(offset + 4),
                self.read(offset + 8),
                self.read(offset + 12),
            ])
        }

        /// Write the page back so the controller reads what the CPU wrote,
        /// and no dirty line is left to land over what it writes.
        fn clean(&self) {
            self.frame
                .clean_data(0, PAGE)
                .unwrap_or_else(|error| panic!("usb-hcd-xhci: clean failed: {:?}", error.code()));
        }

        /// Drop the page's cached lines so the CPU reads what the controller
        /// wrote.
        fn invalidate(&self) {
            self.frame.invalidate_data(0, PAGE).unwrap_or_else(|error| {
                panic!("usb-hcd-xhci: invalidate failed: {:?}", error.code())
            });
        }
    }

    /// The registers of the controller, at the BAR0 mapping `base`.
    struct Registers {
        base: usize,
        operational: usize,
        runtime: usize,
        doorbells: usize,
    }

    impl Registers {
        fn new(base: usize) -> Self {
            let capabilities = Self::capabilities_at(base);
            let length = usize::from(capabilities.read_length_version().length());
            let runtime = usize::try_from(capabilities.read_runtime_offset() & !0x1f)
                .expect("an offset fits in usize");
            let doorbells = usize::try_from(capabilities.read_doorbell_offset() & !0x3)
                .expect("an offset fits in usize");
            Self {
                base,
                operational: base + length,
                runtime: base + runtime,
                doorbells: base + doorbells,
            }
        }

        fn capabilities_at(base: usize) -> MmioCapabilities<'static> {
            // SAFETY: BAR0 is mapped read/write as device memory at `base` for
            // the driver's lifetime, and the capability registers start it.
            unsafe {
                Capabilities::new_mmio(core::ptr::with_exposed_provenance_mut::<Capabilities>(base))
            }
        }

        fn capabilities(&self) -> MmioCapabilities<'static> {
            Self::capabilities_at(self.base)
        }

        fn operational(&self) -> MmioOperational<'static> {
            // SAFETY: the operational registers are `CAPLENGTH` bytes into the
            // mapped BAR0.
            unsafe {
                Operational::new_mmio(core::ptr::with_exposed_provenance_mut::<Operational>(
                    self.operational,
                ))
            }
        }

        fn port(&self, port_id: u8) -> MmioPort<'static> {
            // SAFETY: the port register sets follow the operational registers
            // in the mapped BAR0; `port_id` is a port the controller has.
            unsafe {
                Port::new_mmio(core::ptr::with_exposed_provenance_mut::<Port>(
                    self.operational + port_offset(port_id),
                ))
            }
        }

        fn interrupter(&self) -> MmioInterrupter<'static> {
            // SAFETY: interrupter 0's registers are in the runtime registers
            // of the mapped BAR0.
            unsafe {
                Interrupter::new_mmio(core::ptr::with_exposed_provenance_mut::<Interrupter>(
                    self.runtime + INTERRUPTER_0,
                ))
            }
        }

        /// Ring doorbell `index` with `target`: doorbell 0 is the
        /// controller's own (the command ring has work), doorbell `slot` a
        /// device's, `target` the Device Context Index of the endpoint with
        /// work (§5.6).
        fn ring_doorbell(&self, index: u8, target: u8) {
            // SAFETY: the doorbell array in the mapped BAR0 has a doorbell
            // for every slot the controller enabled.
            unsafe {
                core::ptr::with_exposed_provenance_mut::<u32>(
                    self.doorbells + usize::from(index) * 4,
                )
                .write_volatile(u32::from(target));
            }
        }

        /// The 32-bit word `offset` bytes into BAR0, `None` past `size`.
        fn bar_word(&self, size: usize, offset: usize) -> Option<u32> {
            (offset + 4 <= size).then(|| {
                // SAFETY: the word is inside the mapped BAR0.
                unsafe {
                    core::ptr::with_exposed_provenance::<u32>(self.base + offset).read_volatile()
                }
            })
        }
    }

    /// The event ring and interrupter 0.
    struct EventRing {
        page: DmaPage,
        consumer: Consumer,
        interrupter: MmioInterrupter<'static>,
        operational: MmioOperational<'static>,
    }

    impl EventRing {
        /// Clear the interrupter's pending bit and the controller's event
        /// interrupt, before the events are read: an event that arrives
        /// meanwhile raises the MSI again.
        fn acknowledge(&mut self) {
            self.interrupter.write_management(
                InterrupterManagement::ZERO
                    .with_enable(true)
                    .with_pending(true),
            );
            self.operational
                .write_status(Status::ZERO.with_event_interrupt(true));
        }

        /// Hand every new event to `each`, then move `ERDP` past them and
        /// clear Event Handler Busy. Returns how many there were.
        fn drain(&mut self, mut each: impl FnMut(Trb)) -> usize {
            self.page.invalidate();
            let mut count = 0;
            while count < RING_TRBS {
                let trb = self.page.read_trb(self.consumer.index());
                if !self.consumer.owns(&trb) {
                    break;
                }
                each(trb);
                self.consumer.advance();
                count += 1;
            }
            let (low, high) = split_address(self.consumer.dequeue_pointer());
            self.interrupter.write_dequeue_high(high);
            self.interrupter
                .write_dequeue_low(DequeueLow::new_with_raw_value(low).with_handler_busy(true));
            count
        }
    }

    /// The pages of one device, kept for the next device in the same place
    /// once it is released.
    struct DevicePages {
        output: DmaPage,
        input: DmaPage,
        ring: DmaPage,
    }

    impl DevicePages {
        fn new(index: usize) -> Self {
            let first = FIRST_DEVICE_PAGE + index * DEVICE_PAGES;
            Self {
                output: DmaPage::new(init(), first),
                input: DmaPage::new(init(), first + 1),
                ring: DmaPage::new(init(), first + 2),
            }
        }
    }

    /// An addressed device.
    struct Device {
        slot: u8,
        address: u8,
        speed: UsbSpeed,
        position: Position,
        /// Endpoint 0's packet size, as its context has it.
        max_packet0: u16,
        /// Endpoint 0's transfer ring.
        ring: Producer,
        /// What `mark_hub` recorded of a hub.
        hub: Option<Hub>,
    }

    /// How a control transfer ended, from its events.
    #[derive(Clone, Copy)]
    enum Outcome {
        /// The status stage completed; the data stage left `residual` bytes
        /// unmoved.
        Done { residual: u32 },
        /// A stage failed with this completion code; the endpoint halted.
        Failed { code: u8 },
    }

    /// The translator of a device behind a high-speed hub, for the trace:
    /// ` through the translator of slot 1 port 3` or nothing.
    struct Translated(Option<(u8, u8)>);

    impl core::fmt::Display for Translated {
        fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            match self.0 {
                Some((hub_slot, port)) => write!(
                    formatter,
                    " through the translator of slot {hub_slot} port {port}"
                ),
                None => Ok(()),
            }
        }
    }

    /// The controller, for the holder of a turn.
    struct Controller {
        registers: Registers,
        /// The USB 2 root ports, which `usb_hcd_v1` numbers from 0.
        usb2: Option<Ports>,
        /// Bytes of one context (`HCCPARAMS1.CSZ`).
        context_bytes: u64,
        /// The context page: entry `n` of the Device Context Base Address
        /// Array points at slot `n`'s Output Device Context.
        context: DmaPage,
        command: DmaPage,
        commands: Producer,
        events: EventRing,
        /// The data stage of a control transfer.
        data: DmaPage,
        devices: [Option<Device>; MAX_DEVICES],
        pages: [Option<DevicePages>; MAX_DEVICES],
        pipes: PipeTable,
        /// The keys of `pipe_v1` each client gets for a pipe; `None` when
        /// the component has no clients (no stack pool for calls).
        masters: Option<Masters<{ pipe_v1::FUNCTION_COUNT }>>,
        /// The pages handed to the controller as scratchpad buffers.
        _scratchpads: [Option<DmaPage>; MAX_SCRATCHPADS],
    }

    impl Controller {
        /// Place `trb` on the command ring and ring the doorbell. Returns
        /// the TRB's bus address, which its completion event names.
        fn issue(&mut self, trb: Trb) -> u64 {
            let (placed, link): (Placement, Option<Placement>) = self.commands.push(trb);
            self.command.write_trb(placed.index, placed.trb);
            if let Some(link) = link {
                self.command.write_trb(link.index, link.trb);
            }
            self.command.clean();
            self.registers.ring_doorbell(0, 0);
            placed.bus
        }

        /// Read the event ring until `each` says the event it was given ends
        /// the wait (`Some(true)`), for at most `milliseconds`. Events `each`
        /// does not claim (`None`) are traced. Whether the wait ended.
        fn wait_events(
            &mut self,
            milliseconds: u64,
            mut each: impl FnMut(&Trb) -> Option<bool>,
        ) -> bool {
            let registers = &self.registers;
            let events = &mut self.events;
            let mut done = false;
            wait_until(milliseconds, || {
                events.drain(|trb| match each(&trb) {
                    Some(ends) => done |= ends,
                    None => trace_event(registers, trb),
                });
                done
            })
        }

        /// Trace the events nobody waits for.
        fn drain_unclaimed(&mut self) {
            let registers = &self.registers;
            self.events.drain(|trb| trace_event(registers, trb));
        }

        /// Run command `trb` and wait for its completion: the event, or the
        /// completion code it failed with (0 when none came).
        fn command(&mut self, trb: Trb) -> Result<Trb, u8> {
            let address = self.issue(trb);
            let mut completion = None;
            self.wait_events(COMMAND_MS, |event| {
                (event.kind() == kind::COMMAND_COMPLETION && event.parameter == address).then(
                    || {
                        completion = Some(*event);
                        true
                    },
                )
            });
            match completion {
                Some(event) if event.completion_code() == SUCCESS => Ok(event),
                Some(event) => Err(event.completion_code()),
                None => Err(0),
            }
        }

        /// Power the USB 2 root ports if the controller leaves that to
        /// software, and give a device time to connect. Traces what
        /// connected.
        fn power_ports(&mut self) {
            let Some(usb2) = self.usb2 else {
                note!("xhci: no USB 2 root ports");
                return;
            };
            let switched = self
                .registers
                .capabilities()
                .read_capability1()
                .port_power_control();
            for index in 0..u32::from(usb2.count) {
                let Some(port_id) = usb2.port_id(index) else {
                    break;
                };
                let mut port = self.registers.port(port_id);
                let status = port.read_status_control();
                if switched && !status.power() {
                    port.write_status_control(neutral(status).with_power(true));
                }
            }
            let started = now_us();
            let connected = wait_until(CONNECT_MS, || {
                (0..u32::from(usb2.count))
                    .filter_map(|index| usb2.port_id(index))
                    .any(|port_id| {
                        self.registers
                            .port(port_id)
                            .read_status_control()
                            .connected()
                    })
            });
            for index in 0..u32::from(usb2.count) {
                let Some(port_id) = usb2.port_id(index) else {
                    break;
                };
                let status = self.registers.port(port_id).read_status_control();
                note!(
                    "xhci: USB 2 port {index} (port ID {port_id}): {} (PORTSC {:#010x})",
                    if status.connected() {
                        "connected"
                    } else {
                        "nothing connected"
                    },
                    status.raw_value()
                );
            }
            if connected {
                note!(
                    "xhci: a device connected {} ms after power-up",
                    (now_us() - started) / 1000
                );
            } else {
                note!("xhci: nothing connected within {CONNECT_MS} ms");
            }
        }

        /// Reset USB 2 root port `index` and report the speed of the device on
        /// it. The controller holds the reset as long as USB 2.0 §7.1.7.5 asks
        /// and enables the port (xHCI §4.19.1.1).
        fn reset_port(&mut self, index: u32) -> Result<Speed, Exception> {
            let port_id = self
                .usb2
                .and_then(|usb2| usb2.port_id(index))
                .ok_or(Exception::NoDevice)?;
            let mut port = self.registers.port(port_id);
            let status = port.read_status_control();
            if !status.connected() {
                note!(
                    "xhci: port {index} reset: no device connected (PORTSC {:#010x})",
                    status.raw_value()
                );
                return Err(Exception::NoDevice);
            }
            port.write_status_control(neutral(status).with_reset(true));
            if !wait_until(500, || port.read_status_control().reset_changed()) {
                note!(
                    "xhci: port {index} reset did not finish within 500 ms (PORTSC {:#010x})",
                    port.read_status_control().raw_value()
                );
                return Err(Exception::NoDevice);
            }
            let after = port.read_status_control();
            port.write_status_control(neutral(after).with_reset_changed(true));
            let found = port_speed(after).filter(|_| after.enabled());
            match found {
                Some(speed) => note!(
                    "xhci: port {index} reset: enabled, {speed:?} speed (PORTSC {:#010x})",
                    after.raw_value()
                ),
                None => note!(
                    "xhci: port {index} reset: not enabled at a USB 2 speed (PORTSC {:#010x})",
                    after.raw_value()
                ),
            }
            match found {
                Some(UsbSpeed::Low) => Ok(Speed::Low),
                Some(UsbSpeed::Full) => Ok(Speed::Full),
                Some(UsbSpeed::High) => Ok(Speed::High),
                None => Err(Exception::NoDevice),
            }
        }

        /// The status bits of USB 2 root port `index`; a port that does not
        /// exist reports nothing.
        fn port_status(&self, index: u32) -> u32 {
            self.usb2
                .and_then(|usb2| usb2.port_id(index))
                .map_or(0, |port_id| {
                    port_status_bits(self.registers.port(port_id).read_status_control())
                })
        }

        /// The index of the device at `address`.
        fn device_index(&self, address: u8) -> Option<usize> {
            self.devices.iter().position(|device| {
                device
                    .as_ref()
                    .is_some_and(|device| device.address == address)
            })
        }

        /// Point entry `slot` of the Device Context Base Address Array at
        /// bus address `output` (zero: no context).
        fn set_context_pointer(&self, slot: u8, output: u64) {
            let (low, high) = split_address(output);
            self.context.write_words(u64::from(slot) * 8, &[low, high]);
            self.context.clean();
        }

        /// Fill the Input Context of the device at `index`: the Input Control
        /// Context's add flags `add`, then the Slot Context and endpoint 0's
        /// context where given. Returns its bus address.
        fn write_input(
            &self,
            index: usize,
            add: u32,
            slot: Option<[u32; 4]>,
            endpoint0: Option<[u32; 5]>,
        ) -> u64 {
            let input = &self.pages[index]
                .as_ref()
                .expect("the device's pages")
                .input;
            input.zero();
            // The add flags are word 1 of the Input Control Context.
            input.write(4, add);
            if let Some(words) = slot {
                input.write_words(self.context_bytes, &words);
            }
            if let Some(words) = endpoint0 {
                input.write_words(2 * self.context_bytes, &words);
            }
            input.clean();
            input.bus
        }

        /// Give the `speed` device on port `port` of the hub at `hub_address`
        /// (root port `port` when 0) a slot and an address (§4.3.2, §4.3.3).
        fn address_device(
            &mut self,
            hub_address: u32,
            port: u32,
            speed: UsbSpeed,
        ) -> Result<u8, Exception> {
            let position = if hub_address == 0 {
                let port_id = self
                    .usb2
                    .and_then(|usb2| usb2.port_id(port))
                    .ok_or(Exception::NoDevice)?;
                Position::root(port_id)
            } else {
                let hub = u8::try_from(hub_address)
                    .ok()
                    .and_then(|address| self.device_index(address))
                    .and_then(|index| self.devices[index].as_ref())
                    .filter(|hub| hub.hub.is_some())
                    .ok_or(Exception::NotPermitted)?;
                let port = u8::try_from(port).map_err(|_overflow| Exception::NotPermitted)?;
                hub.position
                    .behind(hub.slot, hub.speed, port, speed)
                    .ok_or(Exception::NotPermitted)?
            };
            let index = self
                .devices
                .iter()
                .position(Option::is_none)
                .ok_or(Exception::Busy)?;
            let slot = match self.command(Trb::enable_slot()) {
                Ok(event) => event.slot(),
                Err(failure) => {
                    note!("xhci: Enable Slot failed with code {failure}");
                    return Err(Exception::Busy);
                }
            };
            let pages = self.pages[index].get_or_insert_with(|| DevicePages::new(index));
            pages.output.zero();
            pages.output.clean();
            pages.ring.zero();
            pages.ring.clean();
            let output = pages.output.bus;
            let ring = Producer::new(pages.ring.bus, RING_TRBS);
            self.set_context_pointer(slot, output);
            let max_packet0 = default_max_packet0(speed);
            let input = self.write_input(
                index,
                ADD_SLOT | ADD_ENDPOINT0,
                Some(slot_context(position, speed, None)),
                Some(endpoint0_context(max_packet0, ring.base())),
            );
            if let Err(failure) = self.command(Trb::address_device(input, slot)) {
                note!(
                    "xhci: Address Device of the {speed:?} speed device at root port ID {} route {:#x} failed with code {failure}",
                    position.root_port,
                    position.route
                );
                self.disable_slot(slot);
                return Err(Exception::NoDevice);
            }
            let pages = self.pages[index].as_ref().expect("the device's pages");
            pages.output.invalidate();
            let address = Slot3::new_with_raw_value(pages.output.read(12)).address();
            note!(
                "xhci: slot {slot}: {speed:?} speed device at root port ID {} route {:#x}{} took address {address}",
                position.root_port,
                position.route,
                Translated(
                    position
                        .translator
                        .map(|translator| (translator.hub_slot, translator.port))
                )
            );
            self.devices[index] = Some(Device {
                slot,
                address,
                speed,
                position,
                max_packet0,
                ring,
                hub: None,
            });
            Ok(address)
        }

        /// Disable `slot` and clear its context pointer; a failure is traced.
        fn disable_slot(&mut self, slot: u8) {
            if let Err(failure) = self.command(Trb::disable_slot(slot)) {
                note!("xhci: Disable Slot {slot} failed with code {failure}");
            }
            self.set_context_pointer(slot, 0);
        }

        /// Forget the device at `address`, closing its pipes.
        fn release_device(&mut self, address: u8) -> Result<(), Exception> {
            let index = self.device_index(address).ok_or(Exception::NotPermitted)?;
            let device = self.devices[index].take().expect("the device");
            self.disable_slot(device.slot);
            self.pipes.close_device(address);
            note!("xhci: slot {}: address {address} released", device.slot);
            Ok(())
        }

        /// Record in the slot of the device at `address` that it is `hub`
        /// (Configure Endpoint with the Slot Context only, as Linux's
        /// `xhci_update_hub_device`).
        fn mark_hub(&mut self, address: u8, hub: Hub) -> Result<(), Exception> {
            let index = self.device_index(address).ok_or(Exception::NotPermitted)?;
            let (slot, position, speed) = {
                let device = self.devices[index].as_ref().expect("the device");
                (device.slot, device.position, device.speed)
            };
            let input = self.write_input(
                index,
                ADD_SLOT,
                Some(slot_context(position, speed, Some(hub))),
                None,
            );
            if let Err(failure) = self.command(Trb::configure_endpoint(input, slot)) {
                note!(
                    "xhci: slot {slot}: Configure Endpoint for the hub failed with code {failure}"
                );
                return Err(Exception::NotPermitted);
            }
            note!(
                "xhci: slot {slot}: address {address} is a hub with {} ports, think time {} FS bits",
                hub.ports,
                hub.think_time
            );
            if let Some(device) = self.devices[index].as_mut() {
                device.hub = Some(hub);
            }
            Ok(())
        }

        /// Give endpoint 0 of the device at `index` the packet size
        /// `max_packet` (Evaluate Context).
        fn set_max_packet0(&mut self, index: usize, max_packet: u16) -> Result<(), PipeException> {
            let (slot, ring, current) = {
                let device = self.devices[index]
                    .as_ref()
                    .ok_or(PipeException::InvalidPipe)?;
                (device.slot, device.ring.base(), device.max_packet0)
            };
            if current == max_packet {
                return Ok(());
            }
            let input = self.write_input(
                index,
                ADD_ENDPOINT0,
                None,
                Some(endpoint0_context(max_packet, ring)),
            );
            if let Err(failure) = self.command(Trb::evaluate_context(input, slot)) {
                note!(
                    "xhci: slot {slot}: Evaluate Context for {max_packet}-byte packets failed with code {failure}"
                );
                return Err(PipeException::Busy);
            }
            if let Some(device) = self.devices[index].as_mut() {
                device.max_packet0 = max_packet;
            }
            Ok(())
        }

        /// Run one control transfer on endpoint 0 of the device at `index`.
        /// The stage is `stage` bytes of `area` from byte `offset`: the setup
        /// packet, then the data stage, read (OUT) or written (IN) in place.
        /// Returns the bytes of the data stage that moved.
        fn control(
            &mut self,
            index: usize,
            area: &DataArea,
            offset: u64,
            stage: usize,
        ) -> Result<usize, PipeException> {
            let mut packet = [0_u8; 8];
            if stage < packet.len() || !area.read_bytes(offset, &mut packet) {
                return Err(PipeException::NotPermitted);
            }
            let setup = SetupPacket::from_bytes(packet);
            let length = usize::from(setup.length);
            if packet.len() + length > stage || length > DATA_CAPACITY {
                return Err(PipeException::NotPermitted);
            }
            // Bit 7 of `bmRequestType` is the data stage's direction.
            let input = setup.request_type & 0x80 != 0;
            let data_offset = offset + packet.len() as u64;
            if !input && !copy_in(area, data_offset, &self.data, length) {
                return Err(PipeException::NotPermitted);
            }
            self.data.clean();
            let transfer = match (length, input) {
                (0, _) => TransferType::NoData,
                (_, true) => TransferType::In,
                (_, false) => TransferType::Out,
            };
            let length_word = u32::try_from(length).unwrap_or(u32::MAX);
            let data_bus = self.data.bus;
            let device = self.devices[index]
                .as_mut()
                .ok_or(PipeException::InvalidPipe)?;
            let slot = device.slot;
            let ring = &self.pages[index].as_ref().expect("the device's pages").ring;
            let mut place = |trb: Trb| {
                let (placed, link) = device.ring.push(trb);
                ring.write_trb(placed.index, placed.trb);
                if let Some(link) = link {
                    ring.write_trb(link.index, link.trb);
                }
                placed.bus
            };
            place(Trb::setup_stage(packet, transfer));
            let data_trb =
                (length > 0).then(|| place(Trb::data_stage(data_bus, length_word, input)));
            // The status stage runs the other way from the data stage, IN
            // when there is none (USB 2.0 §8.5.3).
            let status_trb = place(Trb::status_stage(!(length > 0 && input)));
            ring.clean();
            self.registers.ring_doorbell(slot, ENDPOINT0);

            let mut outcome = None;
            let mut residual = 0;
            self.wait_events(TRANSFER_MS, |event| {
                if event.kind() != kind::TRANSFER_EVENT || event.slot() != slot {
                    return None;
                }
                let completion = event.completion_code();
                if completion == code::SHORT_PACKET && Some(event.parameter) == data_trb {
                    residual = event.residual();
                    return Some(false);
                }
                if completion == SUCCESS {
                    let ends = event.parameter == status_trb;
                    if ends {
                        outcome = Some(Outcome::Done { residual });
                    }
                    return Some(ends);
                }
                outcome = Some(Outcome::Failed { code: completion });
                Some(true)
            });
            match outcome {
                Some(Outcome::Done { residual }) => {
                    let moved = length.saturating_sub(usize::try_from(residual).unwrap_or(0));
                    if input {
                        self.data.invalidate();
                        if !copy_out(&self.data, area, data_offset, moved) {
                            return Err(PipeException::NotPermitted);
                        }
                    }
                    Ok(moved)
                }
                Some(Outcome::Failed { code: failure }) => {
                    self.recover_endpoint0(index, true);
                    if failure == code::STALL {
                        Err(PipeException::Stalled)
                    } else {
                        note!(
                            "xhci: slot {slot}: control {packet:02x?} failed with code {failure}"
                        );
                        Err(PipeException::TransferFailed {
                            status: u32::from(failure),
                        })
                    }
                }
                None => {
                    note!(
                        "xhci: slot {slot}: control {packet:02x?} did not complete within {TRANSFER_MS} ms"
                    );
                    self.recover_endpoint0(index, false);
                    Err(PipeException::TransferFailed { status: 0 })
                }
            }
        }

        /// Get endpoint 0 of the device at `index` going again after a failed
        /// transfer: reset it when it halted, stop it when the transfer is
        /// still pending, then move its dequeue pointer past every TRB placed
        /// (§4.6.8, §4.6.9, §4.6.10).
        fn recover_endpoint0(&mut self, index: usize, halted: bool) {
            let Some((slot, (dequeue, cycle))) = self.devices[index]
                .as_ref()
                .map(|device| (device.slot, device.ring.enqueue_pointer()))
            else {
                return;
            };
            let stop = if halted {
                Trb::reset_endpoint(slot, ENDPOINT0)
            } else {
                Trb::stop_endpoint(slot, ENDPOINT0)
            };
            if let Err(failure) = self.command(stop) {
                note!("xhci: slot {slot}: endpoint 0 did not stop or reset: code {failure}");
            }
            if let Err(failure) = self.command(Trb::set_dequeue(dequeue, cycle, slot, ENDPOINT0)) {
                note!(
                    "xhci: slot {slot}: endpoint 0's dequeue pointer did not move: code {failure}"
                );
            }
        }
    }

    /// Copy `length` bytes of `area` from `from` to the start of `page`.
    fn copy_in(area: &DataArea, from: u64, page: &DmaPage, length: usize) -> bool {
        let mut chunk = [0_u8; COPY_BYTES];
        let mut done = 0;
        while done < length {
            let count = COPY_BYTES.min(length - done);
            let step = done as u64;
            if !area.read_bytes(from + step, &mut chunk[..count]) {
                return false;
            }
            page.write_bytes(step, &chunk[..count]);
            done += count;
        }
        true
    }

    /// Copy `length` bytes from the start of `page` to `area` at `to`.
    fn copy_out(page: &DmaPage, area: &DataArea, to: u64, length: usize) -> bool {
        let mut chunk = [0_u8; COPY_BYTES];
        let mut done = 0;
        while done < length {
            let count = COPY_BYTES.min(length - done);
            let step = done as u64;
            page.read_bytes(step, &mut chunk[..count]);
            if !area.write_bytes(to + step, &chunk[..count]) {
                return false;
            }
            done += count;
        }
        true
    }

    /// What the calls see: the controller, or the step its bring-up failed.
    struct Hcd {
        controller: Option<Controller>,
        failed: Option<BringUp>,
    }

    struct Shared(UnsafeCell<Option<Hcd>>);

    // SAFETY: the cell is written once, before the component says it is ready,
    // and then used only by the holder of a `Turnstile` turn.
    unsafe impl Sync for Shared {}

    static HCD: Shared = Shared(UnsafeCell::new(None));
    static TURNSTILE: Turnstile = Turnstile::new();

    /// The controller, for the holder of `turn`.
    #[allow(
        clippy::needless_pass_by_ref_mut,
        reason = "the exclusive borrow of the turn proves the caller holds it"
    )]
    fn hcd<'turn>(_turn: &'turn mut Turn<'_>) -> &'turn mut Hcd {
        // SAFETY: only the holder of a turn gets here, one at a time, and the
        // cell was filled before the component became ready.
        unsafe { (*HCD.0.get()).as_mut() }.expect("the controller is set up before it is served")
    }

    /// The controller of a call, or the step that failed.
    fn controller(hcd: &mut Hcd) -> Result<&mut Controller, Exception> {
        match (&mut hcd.controller, hcd.failed) {
            (Some(controller), _) => Ok(controller),
            (None, step) => Err(Exception::ControllerFailed {
                step: step.map_or(0, |step| step as u32),
            }),
        }
    }

    /// Tell the firmware the controller at `routing` (`bus << 8 | device << 3
    /// | function`) was reset, so it loads the VL805's firmware, and give the
    /// controller time to start (Linux waits 200 µs to 1 ms).
    fn load_firmware(init: &DriverInit, routing: u32) -> Result<(), BringUp> {
        let mailbox = Mailbox::new(MailboxBindings::from_imports(init.imports()));
        let address = routing << 12;
        // Header (size, request), the tag (id, value bytes, request code,
        // value), the end tag.
        let request = [7 * 4, 0, NOTIFY_XHCI_RESET, 4, 0, address, 0];
        let area = mailbox.data_area().ok_or(BringUp::FirmwareLoad)?;
        let reference = area
            .reference(0, request.len() as u64 * 4)
            .ok_or(BringUp::FirmwareLoad)?;
        if !area.write_words(0, &request) {
            return Err(BringUp::FirmwareLoad);
        }
        mailbox.property(reference).map_err(|error| {
            note!("xhci: firmware notify for {address:#x} failed: {error:?}");
            BringUp::FirmwareLoad
        })?;
        delay_ms(1);
        note!("xhci: firmware told the controller at {address:#x} was reset");
        Ok(())
    }

    /// The claimed function's MSI bits and BAR0's CPU address and size.
    struct Claimed {
        msi_bits: u64,
        bar0: u64,
        bar0_size: u64,
        installed: u32,
    }

    /// Claim the function, lending the Frame table, and read the record.
    fn claim(pci: &Pci, frames: &KeyTableKey) -> Result<Claimed, BringUp> {
        let found = pci
            .data_area()
            .and_then(|area| area.reference(0, claimed::BYTES))
            .ok_or(BringUp::Claim)?;
        let installed = pci.claim(frames, found, 0).map_err(|error| {
            note!("xhci: claim failed: {error:?}");
            BringUp::Claim
        })?;
        let area = pci.data_area().ok_or(BringUp::Claim)?;
        let mut header = [0; 4];
        if !area.read_u64s(claimed::IDENTITY, &mut header) {
            return Err(BringUp::Claim);
        }
        let [identity, class, msi_bits, count] = header;
        let count = usize::try_from(count).map_or(0, |count| count.min(claimed::MAX_BARS));
        let mut bar0 = None;
        for index in 0..count {
            let mut bar = [0; claimed::BAR_WORDS];
            let offset = claimed::BARS + (index * claimed::BAR_WORDS * 8) as u64;
            if !area.read_u64s(offset, &mut bar) {
                return Err(BringUp::Claim);
            }
            let [number, bus, cpu, size] = bar;
            note!("xhci: BAR{number} {bus:#x}+{size:#x}, CPU {cpu:#x}");
            if number == 0 {
                bar0 = Some((cpu, size));
            }
        }
        let (bar0, bar0_size) = bar0.ok_or_else(|| {
            note!("xhci: the function has no BAR0");
            BringUp::Claim
        })?;
        note!(
            "xhci: claimed {:04x}:{:04x} class {class:#08x}, {installed} Frames, MSI bits {msi_bits:#x}",
            identity & 0xffff,
            identity >> 16 & 0xffff
        );
        Ok(Claimed {
            msi_bits,
            bar0,
            bar0_size,
            installed,
        })
    }

    /// Map the claimed Frames of BAR0 at their places in the device window;
    /// returns BAR0's address there.
    fn map_bar0(
        init: &DriverInit,
        frames: &KeyTableKey,
        claimed: &Claimed,
    ) -> Result<usize, BringUp> {
        let own = table(init);
        for index in 0..claimed.installed {
            let key = RawKey::new(KeySlot((FRAME_TABLE_GUARD << FRAME_TABLE_BITS) | index), 1);
            let frame = FrameKey::from_key(
                frames
                    .copy_derive(
                        key,
                        &own,
                        slot(init, FIRST_BAR_SLOT + u64::from(index)),
                        // `GetExtent` below needs `GRANT`.
                        Rights(Rights::READ | Rights::WRITE | Rights::GRANT),
                    )
                    .map_err(|error| {
                        note!("xhci: BAR Frame copy failed: {:?}", error.code());
                        BringUp::Claim
                    })?,
            );
            let (paddr, size) = frame.get_extent().map_err(|error| {
                note!("xhci: BAR Frame GetExtent failed: {:?}", error.code());
                BringUp::Claim
            })?;
            let Some(offset) = paddr
                .checked_sub(claimed.bar0)
                .filter(|&offset| offset + size <= claimed.bar0_size)
            else {
                continue;
            };
            let page = usize::try_from(offset / PAGE).map_err(|_overflow| BringUp::Claim)?;
            if page + usize::try_from(size / PAGE).unwrap_or(usize::MAX) > MAX_DEVICE_PAGES {
                note!("xhci: BAR0 is larger than the device window");
                return Err(BringUp::Claim);
            }
            frame
                .map(
                    RawKey::from_wire(init.address_space),
                    device_page_va(page),
                    Rights(Rights::READ | Rights::WRITE),
                    FrameAttribute::DeviceStrict,
                )
                .map_err(|error| {
                    note!("xhci: BAR Frame Map failed: {:?}", error.code());
                    BringUp::Claim
                })?;
        }
        usize::try_from(device_page_va(0)).map_err(|_overflow| BringUp::Claim)
    }

    /// Halt and reset the controller, set up its context array, scratchpad
    /// buffers, command ring and event ring, and run it (§4.2).
    fn start(init: &DriverInit, base: usize, bar0_size: usize) -> Result<Controller, BringUp> {
        let registers = Registers::new(base);
        let capabilities = registers.capabilities();
        let version = capabilities.read_length_version();
        let structural1 = capabilities.read_structural1();
        let structural2 = capabilities.read_structural2();
        let capability1 = capabilities.read_capability1();
        let mut operational = registers.operational();
        note!(
            "xhci: version {:x}.{:02x}, {} slots, {} ports, {} interrupters, {} scratchpads, {}-bit addresses, {}-byte contexts",
            version.version() >> 8,
            version.version() & 0xff,
            structural1.max_slots(),
            structural1.max_ports(),
            structural1.max_interrupters().value(),
            structural2.scratchpad_buffers(),
            if capability1.addressing_64() { 64 } else { 32 },
            if capability1.context_64() { 64 } else { 32 }
        );

        if !wait_until(1000, || !operational.read_status().not_ready()) {
            return Err(BringUp::NotReady);
        }
        if !operational.read_status().halted() {
            operational.modify_command(|command| command.with_run(false));
            // §5.4.1: the controller halts within 16 microframes.
            if !wait_until(100, || operational.read_status().halted()) {
                return Err(BringUp::Halt);
            }
        }
        operational.write_command(Command::ZERO.with_reset(true));
        // Some controllers must not be read for a moment after the reset is
        // set (Linux's `xhci_reset` waits 1 ms).
        delay_ms(1);
        if !wait_until(1000, || {
            let command = operational.read_command();
            let status = operational.read_status();
            !command.reset() && !status.not_ready()
        }) {
            return Err(BringUp::Reset);
        }
        if operational.read_page_size() & PAGE_SIZE_4K == 0 {
            return Err(BringUp::PageSize);
        }
        let scratchpad_count = structural2.scratchpad_buffers();
        if scratchpad_count > MAX_SCRATCHPADS {
            note!(
                "xhci: {scratchpad_count} scratchpad buffers asked for, {MAX_SCRATCHPADS} at most"
            );
            return Err(BringUp::Scratchpads);
        }

        let usb2 = protocol_ports(
            |offset| registers.bar_word(bar0_size, offset),
            capability1.extended_capabilities(),
            2,
        );
        if let Some(ports) = usb2 {
            note!(
                "xhci: USB 2 ports are port IDs {} to {}",
                ports.first,
                u32::from(ports.first) + u32::from(ports.count).saturating_sub(1)
            );
        }

        assert_ne!(
            init.memory_bus, NO_BUS_ADDRESS,
            "usb-hcd-xhci: the function cannot reach the driver's memory by DMA"
        );
        let context = DmaPage::new(init, CONTEXT_PAGE);
        let command = DmaPage::new(init, COMMAND_PAGE);
        let events = DmaPage::new(init, EVENT_PAGE);
        let data = DmaPage::new(init, DATA_PAGE);
        let mut scratchpads = [const { None }; MAX_SCRATCHPADS];
        for (index, scratchpad) in scratchpads.iter_mut().take(scratchpad_count).enumerate() {
            let page = DmaPage::new(init, FIRST_SCRATCHPAD_PAGE + index);
            page.clean();
            let (low, high) = split_address(page.bus);
            context.write_words(SCRATCHPAD_ARRAY_OFFSET + index as u64 * 8, &[low, high]);
            *scratchpad = Some(page);
        }
        if scratchpad_count > 0 {
            // Entry 0 of the context array points at the scratchpad buffer
            // array (§6.1).
            let (low, high) = split_address(context.bus + SCRATCHPAD_ARRAY_OFFSET);
            context.write_words(0, &[low, high]);
        }
        let segment = segment_table_entry(
            events.bus,
            u16::try_from(RING_TRBS).expect("a segment's TRBs fit in u16"),
        );
        context.write_words(SEGMENT_TABLE_OFFSET, &segment);
        context.clean();
        command.clean();
        events.clean();
        data.clean();

        operational.write_configure(Configure::ZERO.with_slots_enabled(structural1.max_slots()));
        let (low, high) = split_address(context.bus);
        operational.write_context_array_low(low);
        operational.write_context_array_high(high);
        let commands = Producer::new(command.bus, RING_TRBS);
        let (low, high) = split_address(commands.base());
        operational
            .write_command_ring_low(CommandRingControl::new_with_raw_value(low).with_cycle(true));
        operational.write_command_ring_high(high);

        let mut interrupter = registers.interrupter();
        let consumer = Consumer::new(events.bus, RING_TRBS);
        interrupter.write_table_size(1);
        let (low, high) = split_address(consumer.dequeue_pointer());
        interrupter.write_dequeue_high(high);
        interrupter.write_dequeue_low(DequeueLow::new_with_raw_value(low));
        // Writing the table's base starts the event ring, so it goes last.
        let (low, high) = split_address(context.bus + SEGMENT_TABLE_OFFSET);
        interrupter.write_table_base_low(low);
        interrupter.write_table_base_high(high);
        interrupter.write_management(
            InterrupterManagement::ZERO
                .with_enable(true)
                .with_pending(true),
        );

        operational.write_command(Command::ZERO.with_interrupts(true).with_run(true));
        if !wait_until(100, || !operational.read_status().halted()) {
            return Err(BringUp::Run);
        }
        note!("xhci: running, context array at bus {:#x}", context.bus);
        let events = EventRing {
            page: events,
            consumer,
            interrupter,
            operational: registers.operational(),
        };
        // The masters of the pipe keys need the stack pool Kickstart makes
        // for an exporter with clients.
        let masters = StackPoolHandle::from_wire(init.export_pool).map(|pool| {
            Masters::new(
                AddressSpaceKey::from_key(RawKey::from_wire(init.address_space)),
                table(init),
                pool,
                slot(init, MASTERS_SLOT),
                pipe_v1_entries(),
            )
        });
        Ok(Controller {
            registers,
            usb2,
            context_bytes: if capability1.context_64() { 64 } else { 32 },
            context,
            command,
            commands,
            events,
            data,
            devices: [const { None }; MAX_DEVICES],
            pages: [const { None }; MAX_DEVICES],
            pipes: PipeTable::new(),
            masters,
            _scratchpads: scratchpads,
        })
    }

    /// Trace an event the service loop does not act on.
    fn trace_event(registers: &Registers, trb: Trb) {
        match trb.kind() {
            kind::PORT_STATUS_CHANGE => note!(
                "xhci: port ID {} changed (PORTSC {:#010x})",
                trb.port(),
                registers.port(trb.port()).read_status_control().raw_value()
            ),
            kind::HOST_CONTROLLER => {
                note!(
                    "xhci: host controller event, code {}",
                    trb.completion_code()
                );
            }
            other => note!(
                "xhci: event type {other}, code {}, parameter {:#x}",
                trb.completion_code(),
                trb.parameter
            ),
        }
    }

    /// Send a No Op command and wait for its completion event, on the MSI
    /// when there is one. The event ring is read even when no MSI arrives,
    /// so a missing interrupt and a missing completion tell apart.
    fn no_op(controller: &mut Controller, notification: &NotificationKey, msi_bits: u64) {
        let started = now_us();
        let address = controller.issue(Trb::no_op_command());
        let on_msi = msi_bits != 0
            && match notification.await_bits(COMMAND_MS * 1_000_000) {
                Ok(bits) => bits & msi_bits != 0,
                Err(CapError::TimedOut { .. }) => false,
                Err(error) => panic!("usb-hcd-xhci: MSI wait failed: {:?}", error.code()),
            };
        let took = now_us() - started;
        controller.events.acknowledge();
        let mut completion = None;
        let done = controller.wait_events(COMMAND_MS, |trb| {
            (trb.kind() == kind::COMMAND_COMPLETION && trb.parameter == address).then(|| {
                completion = Some(*trb);
                true
            })
        });
        match (done, completion) {
            (true, Some(event)) if event.completion_code() == SUCCESS => note!(
                "xhci: No Op completed in {took} us, {}",
                if on_msi { "on MSI" } else { "without MSI" }
            ),
            (true, Some(event)) => note!(
                "xhci: No Op completed with code {}, {}",
                event.completion_code(),
                if on_msi { "on MSI" } else { "without MSI" }
            ),
            _ => note!(
                "xhci: No Op did not complete within {COMMAND_MS} ms ({}, USBSTS {:#010x}, CRCR {:#010x})",
                if on_msi { "an MSI arrived" } else { "no MSI" },
                controller.registers.operational().read_status().raw_value(),
                controller
                    .registers
                    .operational()
                    .read_command_ring_low()
                    .raw_value()
            ),
        }
    }

    /// Bring the controller up, as the module documentation lists.
    fn bring_up(init: &DriverInit, notification: &NotificationKey) -> Result<Controller, BringUp> {
        let pci = Pci::new(PciBindings::from_imports(init.imports()));
        let routing = pci.address().map_err(|error| {
            note!("xhci: no function behind the bridge: {error:?}");
            BringUp::NoFunction
        })?;
        load_firmware(init, routing)?;

        let frames = UntypedKey::from_key(RawKey::from_wire(init.memory))
            .retype(
                ObjectType::KEY_TABLE,
                FRAME_TABLE_BITS,
                FRAME_TABLE_GUARD,
                1,
                &table(init),
                slot(init, FRAME_TABLE_SLOT),
                Rights::all(),
            )
            .map_or_else(
                |error| panic!("usb-hcd-xhci: KeyTable Retype failed: {:?}", error.code()),
                KeyTableKey::from_key,
            );
        let claimed = claim(&pci, &frames)?;
        let base = map_bar0(init, &frames, &claimed)?;
        let bar0_size = usize::try_from(claimed.bar0_size).unwrap_or(usize::MAX);
        let mut controller = start(init, base, bar0_size)?;
        if claimed.msi_bits == 0 {
            note!("xhci: the function has no MSI: events are not serviced");
        }
        no_op(&mut controller, notification, claimed.msi_bits);
        controller.power_ports();
        if let Some(usb2) = controller.usb2 {
            for index in 0..u32::from(usb2.count) {
                if controller.port_status(index) & 1 != 0 {
                    // The outcome is traced; usb-core resets the port again.
                    let _speed = controller.reset_port(index);
                }
            }
        }
        Ok(controller)
    }

    /// The host controller.
    struct Xhci;

    fn record_return_key() {
        init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
            init().return_key,
        )));
    }

    /// The client whose key carries `badge`: its position among the
    /// clients and its data area.
    fn caller(badge: u64) -> Option<(usize, DataArea)> {
        let (client, area) = init().area_for_badge(badge)?;
        Some((
            client,
            DataArea {
                base: area.base,
                len: area.len,
            },
        ))
    }

    const fn usb_speed(speed: Speed) -> UsbSpeed {
        match speed {
            Speed::Low => UsbSpeed::Low,
            Speed::Full => UsbSpeed::Full,
            Speed::High => UsbSpeed::High,
        }
    }

    impl UsbHcdV1 for Xhci {
        fn enter(_badge: u64) {
            record_return_key();
        }

        fn reset_port(_badge: u64, port: u32) -> Result<Speed, Exception> {
            let mut turn = TURNSTILE.enter(init());
            controller(hcd(&mut turn))?.reset_port(port)
        }

        fn port_status(_badge: u64, port: u32) -> Result<u32, Exception> {
            let mut turn = TURNSTILE.enter(init());
            Ok(controller(hcd(&mut turn))?.port_status(port))
        }

        fn address_device(
            _badge: u64,
            hub_address: u32,
            port: u32,
            speed: Speed,
        ) -> Result<u32, Exception> {
            let mut turn = TURNSTILE.enter(init());
            controller(hcd(&mut turn))?
                .address_device(hub_address, port, usb_speed(speed))
                .map(u32::from)
        }

        fn release_device(_badge: u64, address: u32) -> Result<(), Exception> {
            let address = u8::try_from(address).map_err(|_overflow| Exception::NotPermitted)?;
            let mut turn = TURNSTILE.enter(init());
            controller(hcd(&mut turn))?.release_device(address)
        }

        fn mark_hub(
            _badge: u64,
            address: u32,
            ports: u32,
            think_time: u32,
            multi_tt: bool,
        ) -> Result<(), Exception> {
            let (Ok(address), Ok(ports), Ok(think_time)) = (
                u8::try_from(address),
                u8::try_from(ports),
                u8::try_from(think_time),
            ) else {
                return Err(Exception::NotPermitted);
            };
            if ports == 0 || !matches!(think_time, 8 | 16 | 24 | 32) {
                return Err(Exception::NotPermitted);
            }
            let mut turn = TURNSTILE.enter(init());
            controller(hcd(&mut turn))?.mark_hub(
                address,
                Hub {
                    ports,
                    think_time,
                    multi_tt,
                },
            )
        }

        fn trace(badge: u64, text: IoRec) -> Result<u32, Exception> {
            let (_client, data) = caller(badge).ok_or(Exception::NotPermitted)?;
            if !data.contains(&text) {
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
                let offset = text.base - data.base + written as u64;
                if !data.write_bytes(offset, &chunk[..count]) {
                    return Err(Exception::NotPermitted);
                }
                written += count;
            }
            Ok(u32::try_from(written).unwrap_or(u32::MAX))
        }

        fn open_pipe(
            badge: u64,
            into: &KeyTableKey,
            first_slot: u32,
            device_address: u32,
            endpoint: u32,
            _speed: Speed,
        ) -> Result<(), Exception> {
            let (client, data) = caller(badge).ok_or(Exception::NotPermitted)?;
            let device_address =
                u8::try_from(device_address).map_err(|_overflow| Exception::NotPermitted)?;
            // shortcut: endpoint 0 only; interrupt-IN endpoints are the next
            // xHCI slice.
            if endpoint != 0 {
                return Err(Exception::NotPermitted);
            }
            let mut turn = TURNSTILE.enter(init());
            let controller = controller(hcd(&mut turn))?;
            controller
                .device_index(device_address)
                .ok_or(Exception::NotPermitted)?;
            let handle = controller
                .pipes
                .open(Pipe {
                    device_address,
                    endpoint: 0,
                    max_packet: None,
                    owner: client,
                })
                .map_err(|_full| Exception::Busy)?;
            // The pipe's keys carry its handle as their badge. A pipe whose keys
            // could not be installed or reported is closed again; keys already
            // installed fail on the closed handle.
            let installed = controller
                .masters
                .as_mut()
                .and_then(|masters| masters.install(into, first_slot, u64::from(handle)).ok())
                // The keys' values, then the pipe's handle.
                .filter(|keys| {
                    data.write_u64s(0, keys)
                        && data.write_u64s(8 * keys.len() as u64, &[u64::from(handle)])
                });
            if installed.is_none() {
                controller.pipes.close(handle).ok();
                return Err(Exception::NotPermitted);
            }
            Ok(())
        }
    }

    /// The pipe handle a key's badge names.
    fn handle(badge: u64) -> Result<u32, PipeException> {
        u32::try_from(badge).map_err(|_overflow| PipeException::InvalidPipe)
    }

    /// The controller of a pipe call: a failed bring-up opened no pipes.
    fn pipe_controller(hcd: &mut Hcd) -> Result<&mut Controller, PipeException> {
        hcd.controller.as_mut().ok_or(PipeException::InvalidPipe)
    }

    const fn pipe_error(_error: PipeError) -> PipeException {
        PipeException::InvalidPipe
    }

    impl PipeV1 for Xhci {
        fn enter(_badge: u64) {
            record_return_key();
        }

        fn configure(
            badge: u64,
            kind: TransferKind,
            max_packet: u16,
            _interval_ms: u32,
        ) -> Result<(), PipeException> {
            let handle = handle(badge)?;
            let mut turn = TURNSTILE.enter(init());
            let controller = pipe_controller(hcd(&mut turn))?;
            let pipe = *controller.pipes.get_mut(handle).map_err(pipe_error)?;
            // shortcut: control pipes only until interrupt-IN (the next xHCI
            // slice).
            if kind != TransferKind::Control || !matches!(max_packet, 8 | 16 | 32 | 64) {
                return Err(PipeException::NotPermitted);
            }
            let index = controller
                .device_index(pipe.device_address)
                .ok_or(PipeException::InvalidPipe)?;
            controller.set_max_packet0(index, max_packet)?;
            controller
                .pipes
                .get_mut(handle)
                .map_err(pipe_error)?
                .max_packet = Some(max_packet);
            Ok(())
        }

        fn set_route(badge: u64, _hub_address: u32, _hub_port: u32) -> Result<(), PipeException> {
            // The device's slot names its translator already.
            let handle = handle(badge)?;
            let mut turn = TURNSTILE.enter(init());
            pipe_controller(hcd(&mut turn))?
                .pipes
                .get_mut(handle)
                .map(|_| ())
                .map_err(pipe_error)
        }

        fn close_pipe(badge: u64) -> Result<(), PipeException> {
            let handle = handle(badge)?;
            let mut turn = TURNSTILE.enter(init());
            pipe_controller(hcd(&mut turn))?
                .pipes
                .close(handle)
                .map_err(pipe_error)
        }

        fn clear_halt(badge: u64) -> Result<(), PipeException> {
            // A control pipe's endpoint is reset after the transfer that
            // stalled it, and every transfer starts with a SETUP.
            let handle = handle(badge)?;
            let mut turn = TURNSTILE.enter(init());
            pipe_controller(hcd(&mut turn))?
                .pipes
                .get_mut(handle)
                .map(|_| ())
                .map_err(pipe_error)
        }

        fn control(badge: u64, stage: IoRec) -> Result<u32, PipeException> {
            let handle = handle(badge)?;
            let mut turn = TURNSTILE.enter(init());
            let controller = pipe_controller(hcd(&mut turn))?;
            let pipe = *controller.pipes.get_mut(handle).map_err(pipe_error)?;
            if pipe.max_packet.is_none() {
                return Err(PipeException::NotPermitted);
            }
            // The stage lies in the data area of the client that opened the pipe.
            let area = init()
                .areas()
                .get(pipe.owner)
                .map(|owner| DataArea {
                    base: owner.base,
                    len: owner.len,
                })
                .ok_or(PipeException::NotPermitted)?;
            if !area.contains(&stage) {
                return Err(PipeException::NotPermitted);
            }
            let stage_len =
                usize::try_from(stage.len).map_err(|_overflow| PipeException::NotPermitted)?;
            let index = controller
                .device_index(pipe.device_address)
                .ok_or(PipeException::InvalidPipe)?;
            controller
                .control(index, &area, stage.base - area.base, stage_len)
                .map(|moved| u32::try_from(moved).unwrap_or(u32::MAX))
        }
    }

    usb_hcd::usb_hcd_v1_export!(Xhci);
    usb_hcd::pipe_v1_export!(Xhci);

    /// Bring the controller up, open the turnstile, then service the event
    /// ring on each MSI: calls run on their callers' Threads.
    fn main(init_va: u64) -> ! {
        // SAFETY: Kickstart passes the init page's address in x0.
        let init = unsafe { driver_init(init_va) };
        let notification = NotificationKey::from_key(RawKey::from_wire(init.notification));
        let (controller, failed) = match bring_up(init, &notification) {
            Ok(controller) => (Some(controller), None),
            Err(step) => {
                note!(
                    "xhci: bring-up failed at {step:?} (step {}): every call raises controller_failed",
                    step as u32
                );
                (None, Some(step))
            }
        };
        let serviced = controller.is_some();

        let turns = UntypedKey::from_key(RawKey::from_wire(init.memory))
            .retype(
                ObjectType::EVENT_COUNT,
                0,
                0,
                1,
                &table(init),
                slot(init, TURNS_SLOT),
                Rights::all(),
            )
            .unwrap_or_else(|error| {
                panic!("usb-hcd-xhci: turns Retype failed: {:?}", error.code())
            });
        TURNSTILE.open(turns);
        // SAFETY: nothing else touches the cell until the component says it is
        // ready, just below.
        unsafe {
            *HCD.0.get() = Some(Hcd { controller, failed });
        }
        EventCountKey::from_key(RawKey::from_wire(init.event_count))
            .advance(1)
            .unwrap_or_else(|error| {
                panic!("usb-hcd-xhci: ready advance failed: {:?}", error.code())
            });
        note!(
            "xhci: serving usb_hcd_v1: {} data areas",
            init.areas().len()
        );

        // The service Thread: this one. Each MSI is acknowledged, then the
        // event ring drained under the turnstile, so it never takes the
        // completion a call waits for; without a controller it only waits.
        loop {
            notification
                .await_bits(NotificationKey::WAIT_INFINITE)
                .unwrap_or_else(|error| {
                    panic!("usb-hcd-xhci: event wait failed: {:?}", error.code())
                });
            if serviced {
                let mut turn = TURNSTILE.enter(init);
                if let Some(controller) = hcd(&mut turn).controller.as_mut() {
                    controller.events.acknowledge();
                    controller.drain_unclaimed();
                }
            }
        }
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}

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
//! Every step is traced as an `xhci:` line. The rings, the context array and
//! the scratchpad buffers are pages of the driver's own memory, mapped
//! cacheable: the driver cleans what it wrote before the controller reads it
//! and invalidates the event ring before it reads events.
//!
//! The component then serves `usb_hcd_v1`: `port_status` and `reset_port` on
//! the USB 2 root ports, in the order the Supported Protocol capability lists
//! them (port 0 is the first). A call runs on the caller's Thread, one at a
//! time (a `Turnstile`). The component's own Thread services the event ring
//! on each MSI and traces the port changes it reports.
//!
//! Implementation status: pipes are not served yet: `open_pipe` raises
//! `not_permitted` and every `pipe_v1` function `invalid_pipe` (device
//! addressing and control transfers are the next xHCI slice). `SuperSpeed`
//! ports are not served.

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
            DEVICE_WINDOW_VA, DriverInit, INIT_VA, MAX_DEVICE_PAGES, NO_BUS_ADDRESS, Turn,
            Turnstile, device_page_va, driver_init,
        },
        libobject::{
            CapError, EventCountKey, FrameAttribute, FrameKey, KeySlot, KeyTableKey,
            NotificationKey, ObjectType, RawKey, Rights, UntypedKey, export::init_return_key,
            thread::ThreadReturnKey,
        },
        mailbox_client::mailbox_v1::{Bindings as MailboxBindings, Client as Mailbox},
        pci::claimed,
        pci_client::pci_v1::{Bindings as PciBindings, Client as Pci},
        usb_hcd::{
            pipe_v1::{Exception as PipeException, PipeV1, TransferKind},
            usb_hcd_v1::{
                __runtime::{DataArea, IoRec},
                Exception, Speed, UsbHcdV1,
            },
        },
        usb_hcd_xhci::{
            regs::{
                Capabilities, Command, CommandRingControl, Configure, DequeueLow, INTERRUPTER_0,
                Interrupter, InterrupterManagement, MmioCapabilities, MmioInterrupter,
                MmioOperational, MmioPort, Operational, PAGE_SIZE_4K, Port, Ports, Status, neutral,
                port_offset, port_speed, port_status_bits, protocol_ports, split_address,
            },
            ring::{Consumer, Placement, Producer},
            trb::{SUCCESS, TRB_BYTES, Trb, kind, segment_table_entry},
        },
        vesper_usb_protocol::enumeration::Speed as UsbSpeed,
    };

    libuser::entry!(main);

    /// Bytes of a page, of the controller's (`PAGESIZE` must offer 4 KiB) and
    /// of the driver's.
    const PAGE: u64 = 4096;
    /// TRBs of a ring segment: one page.
    const RING_TRBS: usize = (PAGE / TRB_BYTES) as usize;
    /// Most scratchpad buffers the driver provides.
    const MAX_SCRATCHPADS: usize = 32;
    /// The DMA pages, by index: the context page (the Device Context Base
    /// Address Array, the Event Ring Segment Table and the scratchpad buffer
    /// array), the command ring, the event ring, then the scratchpad buffers.
    const CONTEXT_PAGE: usize = 0;
    const COMMAND_PAGE: usize = 1;
    const EVENT_PAGE: usize = 2;
    const FIRST_SCRATCHPAD_PAGE: usize = 3;
    const DMA_PAGES: usize = FIRST_SCRATCHPAD_PAGE + MAX_SCRATCHPADS;
    /// Offsets in the context page. The array has room for 255 slots and
    /// the scratchpad buffer array (entry 0's target) for every buffer.
    const SEGMENT_TABLE_OFFSET: u64 = 2048;
    const SCRATCHPAD_ARRAY_OFFSET: u64 = 3072;
    const _: () = assert!(SCRATCHPAD_ARRAY_OFFSET + MAX_SCRATCHPADS as u64 * 8 <= PAGE);
    /// Where the DMA pages are mapped: past the device window's BAR pages.
    const DMA_VA: u64 = DEVICE_WINDOW_VA + MAX_DEVICE_PAGES as u64 * PAGE;

    /// Size and guard of the table the claim's BAR Frames go into.
    const FRAME_TABLE_BITS: u8 = 6;
    const FRAME_TABLE_GUARD: u32 = 0x0_BA25;
    /// Slots of the driver's own table, from its first free slot: the Frame
    /// table, the turnstile's `EventCount`, the DMA pages, then the copies of
    /// the BAR Frames it maps.
    const FRAME_TABLE_SLOT: u64 = 0;
    const TURNS_SLOT: u64 = 1;
    const FIRST_DMA_SLOT: u64 = 2;
    const FIRST_BAR_SLOT: u64 = FIRST_DMA_SLOT + DMA_PAGES as u64;

    /// The firmware's property tag telling it the VL805 was reset, with the
    /// function's address as its one value word (Raspberry Pi kernel's
    /// `RPI_FIRMWARE_NOTIFY_XHCI_RESET`).
    const NOTIFY_XHCI_RESET: u32 = 0x0003_0058;
    /// How long the controller waits for a device after powering the ports.
    const CONNECT_MS: u64 = 2000;
    /// How long a command may take before the driver gives up on it.
    const COMMAND_MS: u64 = 100;

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
            for offset in (0..PAGE).step_by(4) {
                page.write(offset, 0);
            }
            page
        }

        fn word(&self, offset: u64) -> *mut u32 {
            core::ptr::with_exposed_provenance_mut::<u32>(
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

        /// Write the page back so the controller reads what the CPU wrote.
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

        /// Ring doorbell 0, the controller's own: the command ring has work.
        fn ring_command_doorbell(&self) {
            // SAFETY: doorbell 0 is the first word of the doorbell array in
            // the mapped BAR0.
            unsafe {
                core::ptr::with_exposed_provenance_mut::<u32>(self.doorbells).write_volatile(0);
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

    /// The event ring and interrupter 0, which only the component's own
    /// Thread touches.
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

    /// The controller, for the holder of a turn.
    struct Controller {
        registers: Registers,
        /// The USB 2 root ports, which `usb_hcd_v1` numbers from 0.
        usb2: Option<Ports>,
        /// The context page, kept for the controller: the device contexts
        /// it will point to come with device addressing.
        _context: DmaPage,
        command: DmaPage,
        commands: Producer,
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
            self.registers.ring_command_doorbell();
            placed.bus
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
                        Rights(Rights::READ | Rights::WRITE),
                    )
                    .map_err(|error| {
                        note!("xhci: BAR Frame copy failed: {:?}", error.code());
                        BringUp::Claim
                    })?,
            );
            let (paddr, size) = frame.get_extent().map_err(|_error| BringUp::Claim)?;
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
    fn start(
        init: &DriverInit,
        base: usize,
        bar0_size: usize,
    ) -> Result<(Controller, EventRing), BringUp> {
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
        let event_ring = EventRing {
            page: events,
            consumer,
            interrupter,
            operational: registers.operational(),
        };
        Ok((
            Controller {
                registers,
                usb2,
                _context: context,
                command,
                commands,
                _scratchpads: scratchpads,
            },
            event_ring,
        ))
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
    fn no_op(
        controller: &mut Controller,
        events: &mut EventRing,
        notification: &NotificationKey,
        msi_bits: u64,
    ) {
        let started = now_us();
        let address = controller.issue(Trb::no_op_command());
        let on_msi = msi_bits != 0
            && match notification.await_bits(COMMAND_MS * 1_000_000) {
                Ok(bits) => bits & msi_bits != 0,
                Err(CapError::TimedOut { .. }) => false,
                Err(error) => panic!("usb-hcd-xhci: MSI wait failed: {:?}", error.code()),
            };
        let took = now_us() - started;
        events.acknowledge();
        let mut completion = None;
        let registers = &controller.registers;
        let done = wait_until(COMMAND_MS, || {
            events.drain(|trb| {
                if trb.kind() == kind::COMMAND_COMPLETION && trb.parameter == address {
                    completion = Some(trb);
                } else {
                    trace_event(registers, trb);
                }
            });
            completion.is_some()
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
    fn bring_up(
        init: &DriverInit,
        notification: &NotificationKey,
    ) -> Result<(Controller, EventRing), BringUp> {
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
        let (mut controller, mut events) = start(init, base, bar0_size)?;
        if claimed.msi_bits == 0 {
            note!("xhci: the function has no MSI: events are not serviced");
        }
        no_op(&mut controller, &mut events, notification, claimed.msi_bits);
        controller.power_ports();
        if let Some(usb2) = controller.usb2 {
            for index in 0..u32::from(usb2.count) {
                if controller.port_status(index) & 1 != 0 {
                    // The outcome is traced; usb-core resets the port again.
                    let _speed = controller.reset_port(index);
                }
            }
        }
        Ok((controller, events))
    }

    /// The host controller.
    struct Xhci;

    fn record_return_key() {
        init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
            init().return_key,
        )));
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

        fn trace(badge: u64, text: IoRec) -> Result<u32, Exception> {
            let (_client, area) = init()
                .area_for_badge(badge)
                .ok_or(Exception::NotPermitted)?;
            let data = DataArea {
                base: area.base,
                len: area.len,
            };
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
            _badge: u64,
            _into: &KeyTableKey,
            _first_slot: u32,
            _device_address: u32,
            _endpoint: u32,
            _speed: Speed,
        ) -> Result<(), Exception> {
            // shortcut: no pipes before device addressing and control
            // transfers (the next xHCI slice).
            Err(Exception::NotPermitted)
        }
    }

    impl PipeV1 for Xhci {
        fn enter(_badge: u64) {
            record_return_key();
        }

        fn configure(
            _badge: u64,
            _kind: TransferKind,
            _max_packet: u16,
            _interval_ms: u32,
        ) -> Result<(), PipeException> {
            Err(PipeException::InvalidPipe)
        }

        fn set_route(_badge: u64, _hub_address: u32, _hub_port: u32) -> Result<(), PipeException> {
            Err(PipeException::InvalidPipe)
        }

        fn close_pipe(_badge: u64) -> Result<(), PipeException> {
            Err(PipeException::InvalidPipe)
        }

        fn clear_halt(_badge: u64) -> Result<(), PipeException> {
            Err(PipeException::InvalidPipe)
        }

        fn control(_badge: u64, _stage: IoRec) -> Result<u32, PipeException> {
            Err(PipeException::InvalidPipe)
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
        let (controller, events, failed) = match bring_up(init, &notification) {
            Ok((controller, events)) => (Some(controller), Some(events), None),
            Err(step) => {
                note!(
                    "xhci: bring-up failed at {step:?} (step {}): every call raises controller_failed",
                    step as u32
                );
                (None, None, Some(step))
            }
        };
        let service_registers = controller
            .as_ref()
            .map(|controller| Registers::new(controller.registers.base));

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
        // event ring drained; without a controller or an MSI it only waits.
        let mut events = events;
        loop {
            notification
                .await_bits(NotificationKey::WAIT_INFINITE)
                .unwrap_or_else(|error| {
                    panic!("usb-hcd-xhci: event wait failed: {:?}", error.code())
                });
            if let (Some(events), Some(registers)) = (events.as_mut(), service_registers.as_ref()) {
                events.acknowledge();
                events.drain(|trb| trace_event(registers, trb));
            }
        }
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}

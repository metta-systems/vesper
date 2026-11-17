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
//!    command ring and one event ring on interrupter 0, with its interrupt
//!    moderation set to 40 µs as Linux sets it, then run.
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
//! An interrupt pipe is an IN endpoint added to its device's slot
//! (Configure Endpoint, the interval rounded down to a power of two of
//! 125 µs), with a transfer ring and a report buffer in a page of its own.
//! While its client has an empty buffer posted on the report channel, one
//! Normal TRB is out and the controller polls the endpoint itself; the
//! completion's report goes into the client's buffer, tagged with the
//! pipe's handle, and the next TRB goes out. A stall stops the polls until
//! `clear_halt` resets the endpoint; any other failure resets it at once and
//! polls on, the trace backing off. `close_pipe` drops the endpoint.
//!
//! After 2 s without an MSI while a TRB is out, a watchdog on the service
//! Thread drains any events no MSI brought in, and traces that it found some.
//! In a `debug_kernel` image the service Thread also traces, every 10 s, the
//! MSIs and the events read since the last summary, the events counted by
//! kind, slot, endpoint and completion code.
//!
//! A call runs on the caller's Thread, one at a time (a `Turnstile`), and
//! reads the event ring itself for the completion it waits on: it checks
//! every 100 µs for the first millisecond, then once a millisecond with the
//! Thread parked. The interrupt pipes' events it reads meanwhile are set
//! aside. The component's own Thread drains the event ring on each MSI,
//! under the same turnstile, traces the port changes, hands on the reports
//! and puts out the next polls; while a pipe waits for its client's buffer
//! it looks at the channels every 2 ms, since a client posts a buffer
//! without a signal.
//!
//! Implementation status: bulk pipes and OUT endpoints are not served:
//! `open_pipe` raises `not_permitted` for an OUT endpoint past 0. `set_route`
//! is accepted and ignored: the slot carries the translator. A failed
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
        core::{
            cell::UnsafeCell,
            sync::atomic::{AtomicU32, Ordering},
        },
        libdevice_driver::{
            DEVICE_WINDOW_VA, DriverInit, INIT_VA, MAX_DEVICE_PAGES, Masters, NO_BUS_ADDRESS, Turn,
            Turnstile, device_page_va, driver_init,
        },
        libio_channel::{Cursor, NoEvent, Packet},
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
                ADD_ENDPOINT0, ADD_SLOT, Hub, Position, Slot3, context_flag, context_index,
                endpoint0_context, interrupt_in_context, interval_exponent, slot_context,
            },
            pipes::{Interrupt, MAX_PIPES, Pipe, PipeError, PipeTable, index_of},
            regs::{
                Capabilities, Command, CommandRingControl, Configure, DequeueLow, INTERRUPTER_0,
                Interrupter, InterrupterManagement, MmioCapabilities, MmioInterrupter,
                MmioOperational, MmioPort, Moderation, Operational, PAGE_SIZE_4K, Port, Ports,
                Status, neutral, port_offset, port_speed, port_status_bits, protocol_ports,
                split_address,
            },
            ring::{Consumer, Placement, Producer},
            trb::{
                ENDPOINT0, SUCCESS, TRB_BYTES, TransferType, Trb, code, kind, segment_table_entry,
            },
        },
        vesper_usb_protocol::{
            enumeration::{Speed as UsbSpeed, default_max_packet0},
            report_channel::{ReportChannel, data_area as report_data_area},
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
    /// page, each device's pages, one page for each interrupt pipe, then the
    /// scratchpad buffers.
    const CONTEXT_PAGE: usize = 0;
    const COMMAND_PAGE: usize = 1;
    const EVENT_PAGE: usize = 2;
    const DATA_PAGE: usize = 3;
    const FIRST_DEVICE_PAGE: usize = 4;
    const FIRST_PIPE_PAGE: usize = FIRST_DEVICE_PAGE + MAX_DEVICES * DEVICE_PAGES;
    const FIRST_SCRATCHPAD_PAGE: usize = FIRST_PIPE_PAGE + MAX_PIPES;
    const DMA_PAGES: usize = FIRST_SCRATCHPAD_PAGE + MAX_SCRATCHPADS;
    /// An interrupt pipe's page: its transfer ring of `PIPE_RING_TRBS` TRBs
    /// from the start, the buffer its reports land in from
    /// `REPORT_OFFSET`, in cache lines of their own.
    const PIPE_RING_TRBS: usize = 64;
    const REPORT_OFFSET: u64 = 2048;
    /// Bytes of a report at most: an interrupt endpoint's largest packet.
    const REPORT_BYTES: usize = 1024;
    const _: () = assert!(PIPE_RING_TRBS as u64 * TRB_BYTES <= REPORT_OFFSET);
    const _: () = assert!(REPORT_OFFSET + REPORT_BYTES as u64 <= PAGE);
    /// Events of interrupt pipes a call's wait set aside for the service
    /// Thread, at most.
    const DEFERRED_EVENTS: usize = 32;
    /// Milliseconds between two looks at the report channels while an
    /// interrupt pipe waits for its client's buffer.
    const BUFFER_POLL_MS: u64 = 2;
    /// The bit a call signals on the service Thread's Notification when it
    /// leaves a pipe waiting for its client's buffer, so the Thread starts
    /// looking at the channels; above any MSI's bits.
    const WAKE_BIT: u64 = 1 << 63;
    /// Milliseconds without an MSI, while an interrupt pipe has a TRB out,
    /// before the service Thread's watchdog looks for events no MSI brought
    /// in ([`Controller::watchdog`]).
    const WATCHDOG_MS: u64 = 2000;
    /// The least time between two interrupts, in `IMOD`'s 250 ns units:
    /// 40 µs, Linux's default (`xhci-hcd`'s `imod_interval`).
    const MODERATION_INTERVAL: u16 = 160;
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
            self.clean_range(0, PAGE);
        }

        /// Like [`clean`](Self::clean), for `length` bytes from `offset`.
        fn clean_range(&self, offset: u64, length: u64) {
            self.frame
                .clean_data(offset, offset + length)
                .unwrap_or_else(|error| panic!("usb-hcd-xhci: clean failed: {:?}", error.code()));
        }

        /// Drop the page's cached lines so the CPU reads what the controller
        /// wrote.
        fn invalidate(&self) {
            self.invalidate_range(0, PAGE);
        }

        /// Like [`invalidate`](Self::invalidate), for `length` bytes from
        /// `offset`.
        fn invalidate_range(&self, offset: u64, length: u64) {
            self.frame
                .invalidate_data(offset, offset + length)
                .unwrap_or_else(|error| {
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
            // Read a register of the controller first. On the Raspberry Pi
            // 4 the MSI is a write to the PCIe bridge's own MSI register and
            // can be seen before the event TRBs the controller wrote ahead
            // of it reach memory; the read's completion cannot pass those
            // writes, so once it returns the events are in memory. Without
            // it a drain could find nothing, and the events would wait for
            // the next MSI.
            let _status = self.operational.read_status();
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
                debug::event(&trb);
                each(trb);
                self.consumer.advance();
                count += 1;
            }
            // The low word first, then the high, as xHCI §5.1 orders a
            // 64-bit register's writes. Given the high word first, the VL805
            // keeps Event Handler Busy set and raises no interrupt for new
            // events (`doc/usb.md`).
            let (low, high) = split_address(self.consumer.dequeue_pointer());
            self.interrupter
                .write_dequeue_low(DequeueLow::new_with_raw_value(low).with_handler_busy(true));
            self.interrupter.write_dequeue_high(high);
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
        /// The Device Context Index of the last endpoint context the slot
        /// has (its Slot Context's Context Entries).
        entries: u8,
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
        /// Each interrupt pipe's page, by its table index, kept for the next
        /// pipe there once it closes.
        pipe_pages: [Option<DmaPage>; MAX_PIPES],
        /// Transfer events of interrupt pipes a wait for something else
        /// read, for [`service`](Self::service) to hand on.
        deferred: [Trb; DEFERRED_EVENTS],
        deferred_count: usize,
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
        /// does not claim (`None`) are set aside when they are transfer
        /// events (an interrupt pipe's report, for [`service`](Self::service))
        /// and traced otherwise. Whether the wait ended.
        fn wait_events(
            &mut self,
            milliseconds: u64,
            mut each: impl FnMut(&Trb) -> Option<bool>,
        ) -> bool {
            let registers = &self.registers;
            let events = &mut self.events;
            let deferred = &mut self.deferred;
            let count = &mut self.deferred_count;
            let mut done = false;
            wait_until(milliseconds, || {
                events.drain(|trb| match each(&trb) {
                    Some(ends) => done |= ends,
                    None => {
                        if trb.kind() == kind::TRANSFER_EVENT {
                            debug::wait_transfer();
                        }
                        set_aside(registers, deferred, count, trb);
                    }
                });
                done
            })
        }

        /// Set aside the events nobody waits for, as [`wait_events`] does.
        /// Returns how many there were.
        fn drain_unclaimed(&mut self) -> usize {
            let registers = &self.registers;
            let deferred = &mut self.deferred;
            let count = &mut self.deferred_count;
            self.events
                .drain(|trb| set_aside(registers, deferred, count, trb))
        }

        /// Whether an interrupt pipe has a TRB out.
        fn polling(&self) -> bool {
            self.pipes.open_pipes().any(|(_, pipe)| {
                pipe.interrupt
                    .is_some_and(|interrupt| interrupt.in_flight.is_some())
            })
        }

        /// The watchdog, when no MSI came for `WATCHDOG_MS` while an
        /// interrupt pipe has a TRB out: drain any events the controller
        /// wrote without raising an MSI, as an MSI would, and trace that it
        /// found some, with back-off. A device with nothing to report keeps
        /// its TRB out for good, so this runs every `WATCHDOG_MS` while every
        /// device is quiet.
        fn watchdog(&mut self) {
            debug::watchdog_run();
            self.events.acknowledge();
            let missed = self.drain_unclaimed();
            if missed > 0 {
                let (count, traced) = counted(&MISSED_INTERRUPTS);
                if traced {
                    note!(
                        "xhci: watchdog: {missed} events in the ring without an MSI ({count} times so far)"
                    );
                }
            }
        }

        /// The slot of the device at `address`.
        fn slot_of(&self, address: u8) -> Option<u8> {
            self.device_index(address)
                .and_then(|index| self.devices[index].as_ref())
                .map(|device| device.slot)
        }

        /// Hand on the interrupt pipes' reports the event ring brought, and
        /// put a Normal TRB on every interrupt pipe whose client has an empty
        /// buffer posted and that has none out. Whether a pipe is left
        /// waiting for its client's buffer, so its channel is looked at again
        /// soon.
        fn service(&mut self) -> bool {
            // In the order they came: a pipe's events name its TRBs in turn.
            // Completing one may run a command, whose wait sets more aside.
            while self.deferred_count > 0 {
                let pending = self.deferred;
                let count = core::mem::take(&mut self.deferred_count);
                for &event in &pending[..count] {
                    self.complete_interrupt(event);
                }
            }
            let mut starved = false;
            let pipes: [Option<(u32, Pipe)>; MAX_PIPES] = {
                let mut open = self.pipes.open_pipes();
                core::array::from_fn(|_| open.next())
            };
            for (handle, pipe) in pipes.into_iter().flatten() {
                let Some(interrupt) = pipe.interrupt.filter(Interrupt::wants_buffer) else {
                    continue;
                };
                if buffer_waiting(pipe.owner) {
                    self.poll(handle, pipe, interrupt);
                } else {
                    starved = true;
                }
            }
            starved
        }

        /// Put a Normal TRB for one report on the interrupt pipe `handle`
        /// and ring its endpoint's doorbell: the controller polls the
        /// endpoint until the device answers with data.
        fn poll(&mut self, handle: u32, pipe: Pipe, mut interrupt: Interrupt) {
            let Some(slot) = self.slot_of(pipe.device_address) else {
                return;
            };
            let Some(page) = self.pipe_pages[index_of(handle)].as_ref() else {
                return;
            };
            let (placed, link) = interrupt.ring.push(Trb::normal(
                page.bus + REPORT_OFFSET,
                u32::from(interrupt.max_packet),
            ));
            page.write_trb(placed.index, placed.trb);
            if let Some(link) = link {
                page.write_trb(link.index, link.trb);
            }
            page.clean_range(0, REPORT_OFFSET);
            // A dirty line of the buffer must not land over the report.
            page.clean_range(REPORT_OFFSET, REPORT_BYTES as u64);
            interrupt.in_flight = Some(placed.bus);
            if let Ok(open) = self.pipes.get_mut(handle) {
                open.interrupt = Some(interrupt);
            }
            self.registers.ring_doorbell(slot, pipe.context_index);
        }

        /// Take the transfer event `event` for the interrupt pipe whose TRB
        /// it names: give its client the report, or note the failure.
        fn complete_interrupt(&mut self, event: Trb) {
            let found = self.pipes.open_pipes().find(|(_, pipe)| {
                pipe.interrupt
                    .is_some_and(|interrupt| interrupt.in_flight == Some(event.parameter))
            });
            let Some((handle, pipe)) = found else {
                note!(
                    "xhci: transfer event for no pipe: slot {} endpoint {}, code {}",
                    event.slot(),
                    event.endpoint(),
                    event.completion_code()
                );
                return;
            };
            let Some(mut interrupt) = pipe.interrupt else {
                return;
            };
            let completion = event.completion_code();
            interrupt.in_flight = None;
            match completion {
                SUCCESS | code::SHORT_PACKET => {
                    let length = usize::from(interrupt.max_packet)
                        .saturating_sub(usize::try_from(event.residual()).unwrap_or(0));
                    if let Some(page) = self.pipe_pages[index_of(handle)].as_ref() {
                        page.invalidate_range(REPORT_OFFSET, REPORT_BYTES as u64);
                        let mut report = [0_u8; REPORT_BYTES];
                        let report = &mut report[..length.min(REPORT_BYTES)];
                        page.read_bytes(REPORT_OFFSET, report);
                        deliver(pipe.owner, handle, report);
                        debug::report(report);
                    }
                }
                code::STALL => {
                    interrupt.halted = true;
                    note!(
                        "xhci: pipe {handle}: endpoint {} of device {} stalled; polls stop until clear_halt",
                        pipe.context_index,
                        pipe.device_address
                    );
                }
                failure => {
                    let (count, traced) = counted(&POLL_FAILURES);
                    if traced {
                        note!(
                            "xhci: pipe {handle}: poll of endpoint {} of device {} failed with code {failure} ({count} so far)",
                            pipe.context_index,
                            pipe.device_address
                        );
                    }
                }
            }
            if let Ok(open) = self.pipes.get_mut(handle) {
                open.interrupt = Some(interrupt);
            }
            // Any failure but a stall halted the endpoint too: get it going
            // again, past the failed TRB, and poll on.
            if !matches!(completion, SUCCESS | code::SHORT_PACKET | code::STALL) {
                self.recover_endpoint(handle);
            }
        }

        /// Reset the halted endpoint of the interrupt pipe `handle` and move
        /// its dequeue pointer past every TRB placed (§4.6.8, §4.6.10).
        fn recover_endpoint(&mut self, handle: u32) {
            let Ok(pipe) = self.pipes.get_mut(handle).map(|pipe| *pipe) else {
                return;
            };
            let (Some(interrupt), Some(slot)) = (
                pipe.interrupt,
                self.device_index(pipe.device_address)
                    .and_then(|index| self.devices[index].as_ref())
                    .map(|device| device.slot),
            ) else {
                return;
            };
            if let Err(failure) = self.command(Trb::reset_endpoint(slot, pipe.context_index)) {
                note!("xhci: pipe {handle}: Reset Endpoint failed with code {failure}");
            }
            let (dequeue, cycle) = interrupt.ring.enqueue_pointer();
            if let Err(failure) =
                self.command(Trb::set_dequeue(dequeue, cycle, slot, pipe.context_index))
            {
                note!("xhci: pipe {handle}: Set TR Dequeue Pointer failed with code {failure}");
            }
            if let Ok(open) = self.pipes.get_mut(handle)
                && let Some(open) = open.interrupt.as_mut()
            {
                open.in_flight = None;
                open.halted = false;
            }
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
        /// Context's drop flags `drop` and add flags `add`, then the Slot
        /// Context and one endpoint's context (by its Device Context Index)
        /// where given. Returns its bus address.
        fn write_input(
            &self,
            index: usize,
            drop: u32,
            add: u32,
            slot: Option<[u32; 4]>,
            endpoint: Option<(u8, [u32; 5])>,
        ) -> u64 {
            let input = &self.pages[index]
                .as_ref()
                .expect("the device's pages")
                .input;
            input.zero();
            // The drop and add flags are words 0 and 1 of the Input Control
            // Context; context `n` follows at `n + 1` contexts in.
            input.write(0, drop);
            input.write(4, add);
            if let Some(words) = slot {
                input.write_words(self.context_bytes, &words);
            }
            if let Some((context, words)) = endpoint {
                input.write_words((u64::from(context) + 1) * self.context_bytes, &words);
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
                    .filter(|&port_id| {
                        self.registers
                            .port(port_id)
                            .read_status_control()
                            .connected()
                    })
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
                0,
                ADD_SLOT | ADD_ENDPOINT0,
                Some(slot_context(position, speed, None, 1)),
                Some((ENDPOINT0, endpoint0_context(max_packet0, ring.base()))),
            );
            if let Err(failure) = self.command(Trb::address_device(input, slot)) {
                note!(
                    "xhci: Address Device of the {speed:?} speed device at root port ID {} route {:#x} failed with code {failure}",
                    position.root_port,
                    position.route
                );
                self.disable_slot(slot);
                return Err(Exception::TransferFailed {
                    status: u32::from(failure),
                });
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
                entries: 1,
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
            let (slot, position, speed, entries) = {
                let device = self.devices[index].as_ref().expect("the device");
                (device.slot, device.position, device.speed, device.entries)
            };
            let input = self.write_input(
                index,
                0,
                ADD_SLOT,
                Some(slot_context(position, speed, Some(hub), entries)),
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
                0,
                ADD_ENDPOINT0,
                None,
                Some((ENDPOINT0, endpoint0_context(max_packet, ring))),
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

        /// Add the interrupt IN endpoint of pipe `handle` to its device's slot
        /// (Configure Endpoint): `max_packet` bytes every `interval_ms`, on a
        /// fresh transfer ring in the pipe's page. A pipe configured before
        /// has its endpoint dropped and added again.
        fn configure_interrupt(
            &mut self,
            handle: u32,
            max_packet: u16,
            interval_ms: u32,
        ) -> Result<(), PipeException> {
            let pipe = *self.pipes.get_mut(handle).map_err(pipe_error)?;
            // Another configured pipe on the same endpoint would share its
            // ring.
            if self.pipes.open_pipes().any(|(other, open)| {
                other != handle
                    && open.device_address == pipe.device_address
                    && open.context_index == pipe.context_index
                    && open.interrupt.is_some()
            }) {
                return Err(PipeException::Busy);
            }
            let index = self
                .device_index(pipe.device_address)
                .ok_or(PipeException::InvalidPipe)?;
            let (slot, position, speed, hub, entries) = {
                let device = self.devices[index].as_ref().expect("the device");
                (
                    device.slot,
                    device.position,
                    device.speed,
                    device.hub,
                    device.entries.max(pipe.context_index),
                )
            };
            let page_index = index_of(handle);
            let page = self.pipe_pages[page_index]
                .get_or_insert_with(|| DmaPage::new(init(), FIRST_PIPE_PAGE + page_index));
            page.zero();
            page.clean();
            let ring = Producer::new(page.bus, PIPE_RING_TRBS);
            let flag = context_flag(pipe.context_index);
            let input = self.write_input(
                index,
                if pipe.interrupt.is_some() { flag } else { 0 },
                ADD_SLOT | flag,
                Some(slot_context(position, speed, hub, entries)),
                Some((
                    pipe.context_index,
                    interrupt_in_context(
                        max_packet,
                        interval_exponent(speed, interval_ms),
                        ring.base(),
                    ),
                )),
            );
            if let Err(failure) = self.command(Trb::configure_endpoint(input, slot)) {
                note!(
                    "xhci: pipe {handle}: Configure Endpoint for endpoint {} of device {} failed with code {failure}",
                    pipe.context_index,
                    pipe.device_address
                );
                return Err(match failure {
                    code::BANDWIDTH_ERROR | code::RESOURCE_ERROR => PipeException::NoBandwidth,
                    _ => PipeException::Busy,
                });
            }
            if let Some(device) = self.devices[index].as_mut() {
                device.entries = entries;
            }
            if let Ok(open) = self.pipes.get_mut(handle) {
                open.interrupt = Some(Interrupt {
                    ring,
                    max_packet,
                    in_flight: None,
                    halted: false,
                });
            }
            note!(
                "xhci: pipe {handle}: polling endpoint {} of device {} ({speed:?} speed) every {interval_ms} ms, {max_packet} bytes",
                pipe.context_index,
                pipe.device_address
            );
            Ok(())
        }

        /// Drop the interrupt endpoint of pipe `handle` from its device's
        /// slot (Configure Endpoint), which stops its polls; a device
        /// already released dropped it with its slot.
        fn drop_interrupt(&mut self, handle: u32) {
            let Ok(pipe) = self.pipes.get_mut(handle).map(|pipe| *pipe) else {
                return;
            };
            let Some(index) = pipe
                .interrupt
                .and_then(|_| self.device_index(pipe.device_address))
            else {
                return;
            };
            let (slot, position, speed, hub, entries) = {
                let device = self.devices[index].as_ref().expect("the device");
                (
                    device.slot,
                    device.position,
                    device.speed,
                    device.hub,
                    device.entries,
                )
            };
            let input = self.write_input(
                index,
                context_flag(pipe.context_index),
                ADD_SLOT,
                Some(slot_context(position, speed, hub, entries)),
                None,
            );
            if let Err(failure) = self.command(Trb::configure_endpoint(input, slot)) {
                note!("xhci: pipe {handle}: dropping its endpoint failed with code {failure}");
            }
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
            // Only endpoint 0's events: an interrupt endpoint of the same
            // device may complete meanwhile, and its report is set aside.
            self.wait_events(TRANSFER_MS, |event| {
                if event.kind() != kind::TRANSFER_EVENT
                    || event.slot() != slot
                    || event.endpoint() != ENDPOINT0
                {
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

    /// Polls of interrupt pipes that failed, for the trace's back-off.
    static POLL_FAILURES: AtomicU32 = AtomicU32::new(0);
    /// Watchdog runs that found events no MSI brought, for the trace's
    /// back-off.
    static MISSED_INTERRUPTS: AtomicU32 = AtomicU32::new(0);

    /// Diagnostics of a `debug_kernel` image: what the event ring brought
    /// and how, counted and traced every `SUMMARY_MS` while the service
    /// Thread runs.
    #[cfg(debug_kernel)]
    mod debug {
        use {
            super::{EventRing, now_us},
            core::{
                fmt,
                sync::atomic::{AtomicU32, AtomicU64, Ordering},
            },
            usb_hcd_xhci::trb::{Trb, kind},
        };

        /// Milliseconds between two summaries: under the console's 30 s.
        const SUMMARY_MS: u64 = 10_000;
        /// Kinds of event told apart; the rest are counted together.
        const CLASSES: usize = 12;

        static LAST_SUMMARY_US: AtomicU64 = AtomicU64::new(0);
        static MSI_WAKES: AtomicU32 = AtomicU32::new(0);
        static MSI_EVENTS: AtomicU32 = AtomicU32::new(0);
        static WAIT_TRANSFERS: AtomicU32 = AtomicU32::new(0);
        static WATCHDOG_RUNS: AtomicU32 = AtomicU32::new(0);
        static REPORTS: AtomicU32 = AtomicU32::new(0);
        static ZERO_REPORTS: AtomicU32 = AtomicU32::new(0);
        /// Each class's key (`class_key`, 0 for a free entry) and count.
        static CLASS_KEYS: [AtomicU32; CLASSES] = [const { AtomicU32::new(0) }; CLASSES];
        static CLASS_COUNTS: [AtomicU32; CLASSES] = [const { AtomicU32::new(0) }; CLASSES];
        static UNCLASSED: AtomicU32 = AtomicU32::new(0);

        /// An event's class: its kind, completion code and, for a transfer
        /// event, its slot and endpoint. Never zero: no event has kind 0.
        fn class_key(trb: &Trb) -> u32 {
            let (slot, endpoint) = if trb.kind() == kind::TRANSFER_EVENT {
                (trb.slot(), trb.endpoint())
            } else {
                (0, 0)
            };
            u32::from_be_bytes([trb.kind(), slot, endpoint, trb.completion_code()])
        }

        /// Count `trb`, an event read from the ring, by its class.
        pub(super) fn event(trb: &Trb) {
            let key = class_key(trb);
            for (stored, count) in CLASS_KEYS.iter().zip(&CLASS_COUNTS) {
                let found = stored.load(Ordering::Relaxed);
                if found == key
                    || (found == 0
                        && stored
                            .compare_exchange(0, key, Ordering::Relaxed, Ordering::Relaxed)
                            .is_ok())
                {
                    count.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            }
            UNCLASSED.fetch_add(1, Ordering::Relaxed);
        }

        /// An MSI woke the service Thread, and `drained` events were read on it.
        pub(super) fn msi(drained: usize) {
            MSI_WAKES.fetch_add(1, Ordering::Relaxed);
            MSI_EVENTS.fetch_add(
                u32::try_from(drained).unwrap_or(u32::MAX),
                Ordering::Relaxed,
            );
        }

        /// A command's wait read an interrupt pipe's transfer event.
        pub(super) fn wait_transfer() {
            WAIT_TRANSFERS.fetch_add(1, Ordering::Relaxed);
        }

        /// The watchdog ran.
        pub(super) fn watchdog_run() {
            WATCHDOG_RUNS.fetch_add(1, Ordering::Relaxed);
        }

        /// `report` went to its client.
        pub(super) fn report(report: &[u8]) {
            REPORTS.fetch_add(1, Ordering::Relaxed);
            if report.iter().all(|&byte| byte == 0) {
                ZERO_REPORTS.fetch_add(1, Ordering::Relaxed);
            }
        }

        /// One class of event, by its key: `transfer slot 4 endpoint 3
        /// code 1`, `port change code 1`.
        struct Class(u32);

        impl fmt::Display for Class {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                let [event_kind, slot, endpoint, code] = self.0.to_be_bytes();
                match event_kind {
                    kind::TRANSFER_EVENT => {
                        write!(formatter, "transfer slot {slot} endpoint {endpoint}")?;
                    }
                    kind::COMMAND_COMPLETION => formatter.write_str("command")?,
                    kind::PORT_STATUS_CHANGE => formatter.write_str("port change")?,
                    kind::HOST_CONTROLLER => formatter.write_str("host controller")?,
                    other => write!(formatter, "type {other}")?,
                }
                write!(formatter, " code {code}")
            }
        }

        /// Trace what came since the last summary, every `SUMMARY_MS`: the
        /// MSI wakes and the events read on them, the transfer events read
        /// in command waits, the watchdog runs, the reports handed on (and
        /// how many were all zeros), and the interrupter's `IMAN` and `ERDP`
        /// (Event Handler Busy is bit 3); then one line per class of event
        /// read, with its count.
        pub(super) fn summary(events: &EventRing) {
            let now = now_us();
            let last = LAST_SUMMARY_US.load(Ordering::Relaxed);
            if now.saturating_sub(last) < SUMMARY_MS * 1000 {
                return;
            }
            LAST_SUMMARY_US.store(now, Ordering::Relaxed);
            note!(
                "xhci: debug: {} MSI wakes ({} events), {} transfer events in command waits, {} watchdog runs, {} reports ({} all zeros); IMAN {:#x}, ERDP low {:#x}",
                MSI_WAKES.swap(0, Ordering::Relaxed),
                MSI_EVENTS.swap(0, Ordering::Relaxed),
                WAIT_TRANSFERS.swap(0, Ordering::Relaxed),
                WATCHDOG_RUNS.swap(0, Ordering::Relaxed),
                REPORTS.swap(0, Ordering::Relaxed),
                ZERO_REPORTS.swap(0, Ordering::Relaxed),
                events.interrupter.read_management().raw_value(),
                events.interrupter.read_dequeue_low().raw_value()
            );
            for (stored, count) in CLASS_KEYS.iter().zip(&CLASS_COUNTS) {
                let key = stored.swap(0, Ordering::Relaxed);
                let count = count.swap(0, Ordering::Relaxed);
                if key != 0 {
                    note!("xhci: debug:   {}: {count}", Class(key));
                }
            }
            let unclassed = UNCLASSED.swap(0, Ordering::Relaxed);
            if unclassed != 0 {
                note!("xhci: debug:   other events: {unclassed}");
            }
        }
    }

    /// The same hooks in every other image, doing nothing.
    #[cfg(not(debug_kernel))]
    mod debug {
        use {super::EventRing, usb_hcd_xhci::trb::Trb};

        pub(super) const fn event(_trb: &Trb) {}
        pub(super) const fn msi(_drained: usize) {}
        pub(super) const fn wait_transfer() {}
        pub(super) const fn watchdog_run() {}
        pub(super) const fn report(_report: &[u8]) {}
        pub(super) const fn summary(_events: &EventRing) {}
    }

    /// Count one more event on `counter`: its count, and whether to trace it
    /// (the 1st, 2nd, 4th, 8th...), so a failure that repeats cannot flood
    /// the log.
    fn counted(counter: &AtomicU32) -> (u32, bool) {
        let count = counter.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        (count, count.is_power_of_two())
    }

    /// Keep a transfer event nobody waited for in `deferred` for
    /// [`Controller::service`]; trace any other event, and a transfer event
    /// that finds no room.
    fn set_aside(
        registers: &Registers,
        deferred: &mut [Trb; DEFERRED_EVENTS],
        count: &mut usize,
        trb: Trb,
    ) {
        if trb.kind() == kind::TRANSFER_EVENT
            && let Some(free) = deferred.get_mut(*count)
        {
            *free = trb;
            *count += 1;
        } else {
            trace_event(registers, trb);
        }
    }

    /// Wake the service Thread from a call that left a pipe waiting for its
    /// client's buffer: the Thread may be waiting for an MSI only, and none
    /// comes for a pipe with no TRB out.
    fn wake_service() {
        if let Err(error) =
            NotificationKey::from_key(RawKey::from_wire(init().notification)).signal(WAKE_BIT)
        {
            note!("xhci: waking the service Thread failed: {:?}", error.code());
        }
    }

    /// Whether the client at position `owner` has an empty buffer posted on
    /// its report channel.
    fn buffer_waiting(owner: usize) -> bool {
        let Some(area) = init()
            .areas()
            .get(owner)
            .filter(|area| area.channel_base != 0)
        else {
            return false;
        };
        // SAFETY: Kickstart mapped the channel's control page read/write at
        // `channel_base` for the driver's lifetime.
        let channel = unsafe { ReportChannel::at(area.channel_base) };
        !channel.reports.submit.is_empty()
    }

    /// Give the client at position `owner` `report`, in the first empty
    /// buffer it posted, tagged with the pipe `handle`. A report longer than
    /// the buffer is cut.
    fn deliver(owner: usize, handle: u32, report: &[u8]) {
        let Some(area) = init()
            .areas()
            .get(owner)
            .filter(|area| area.channel_base != 0)
        else {
            return;
        };
        // SAFETY: as in `buffer_waiting`.
        let channel = unsafe { ReportChannel::at(area.channel_base) };
        let data = report_data_area(area.channel_base);
        let (mut buffers, mut reports) = channel.reports.server(
            NoEvent,
            EventCountKey::from_key(RawKey::from_wire(area.channel_event)),
        );
        let Ok(Some(mut packet)) = buffers.get() else {
            return;
        };
        if data.holds(&packet) {
            let mut cursor = Cursor::START;
            for &byte in report {
                let Some(address) = cursor.next_address(&packet) else {
                    break;
                };
                // SAFETY: the data area holds the packet and is mapped
                // read/write; the client gave the buffer to the driver until
                // it comes back.
                unsafe {
                    core::ptr::with_exposed_provenance_mut::<u8>(
                        usize::try_from(address).unwrap_or(usize::MAX),
                    )
                    .write_volatile(byte);
                }
            }
            cursor.cut(&mut packet);
        } else {
            // A buffer outside the data area comes back empty.
            Cursor::START.cut(&mut packet);
        }
        let Some(report) = Packet::new(u64::from(handle), packet.recs()) else {
            return;
        };
        // A client with more buffers out than the FIFO holds loses the report.
        let _lost = reports.put(&report);
        // Only a client's EventCount can fail to advance.
        let _signalled = reports.signal();
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
        let vendor = u16::try_from(identity & 0xffff).unwrap_or(0);
        let device = u16::try_from(identity >> 16 & 0xffff).unwrap_or(0);
        note!(
            "xhci: claimed {vendor:04x}:{device:04x} class {class:#08x}, {installed} Frames, MSI bits {msi_bits:#x}"
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
        // Low word first, as in `EventRing::drain`.
        interrupter.write_dequeue_low(DequeueLow::new_with_raw_value(low));
        interrupter.write_dequeue_high(high);
        // Writing the table's base starts the event ring, so it goes last.
        let (low, high) = split_address(context.bus + SEGMENT_TABLE_OFFSET);
        interrupter.write_table_base_low(low);
        interrupter.write_table_base_high(high);
        // Whatever the firmware left there is replaced.
        let found = interrupter.read_moderation();
        interrupter.write_moderation(Moderation::ZERO.with_interval(MODERATION_INTERVAL));
        note!(
            "xhci: interrupt moderation every {} x 250 ns (IMOD was {:#x})",
            MODERATION_INTERVAL,
            found.raw_value()
        );
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
            pipe_pages: [const { None }; MAX_PIPES],
            deferred: [Trb::default(); DEFERRED_EVENTS],
            deferred_count: 0,
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
            // `endpoint` is an endpoint address: the number in bits 0 to 3,
            // bit 7 the direction. Endpoint 0 and IN endpoints are served;
            // OUT endpoints carry bulk transfers, which are not.
            let address = u8::try_from(endpoint)
                .ok()
                .filter(|&address| address & 0x70 == 0)
                .filter(|&address| context_index(address) == ENDPOINT0 || address & 0x80 != 0)
                .ok_or(Exception::NotPermitted)?;
            let mut turn = TURNSTILE.enter(init());
            let controller = controller(hcd(&mut turn))?;
            controller
                .device_index(device_address)
                .ok_or(Exception::NotPermitted)?;
            let handle = controller
                .pipes
                .open(Pipe {
                    device_address,
                    context_index: context_index(address),
                    max_packet: None,
                    interrupt: None,
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
            interval_ms: u32,
        ) -> Result<(), PipeException> {
            let handle = handle(badge)?;
            let mut turn = TURNSTILE.enter(init());
            let controller = pipe_controller(hcd(&mut turn))?;
            let pipe = *controller.pipes.get_mut(handle).map_err(pipe_error)?;
            match kind {
                // Endpoint 0 carries control transfers.
                TransferKind::Control
                    if pipe.context_index == ENDPOINT0
                        && matches!(max_packet, 8 | 16 | 32 | 64) =>
                {
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
                // An IN endpoint past 0 is an interrupt endpoint.
                TransferKind::Interrupt
                    if pipe.context_index != ENDPOINT0
                        && (1..=1024).contains(&max_packet)
                        && (1..=255).contains(&interval_ms) =>
                {
                    controller.configure_interrupt(handle, max_packet, interval_ms)?;
                    // Its client may have a buffer posted already.
                    if controller.service() {
                        wake_service();
                    }
                    Ok(())
                }
                // Bulk pipes are not served.
                _ => Err(PipeException::NotPermitted),
            }
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
            let controller = pipe_controller(hcd(&mut turn))?;
            // An interrupt pipe's endpoint leaves the slot, which stops its
            // polls.
            controller.drop_interrupt(handle);
            controller.pipes.close(handle).map_err(pipe_error)
        }

        fn clear_halt(badge: u64) -> Result<(), PipeException> {
            // A control pipe's endpoint is reset after the transfer that
            // stalled it, and every transfer starts with a SETUP. A stalled
            // interrupt pipe's endpoint is reset now, and polled again.
            let handle = handle(badge)?;
            let mut turn = TURNSTILE.enter(init());
            let controller = pipe_controller(hcd(&mut turn))?;
            let pipe = *controller.pipes.get_mut(handle).map_err(pipe_error)?;
            if pipe.interrupt.is_some_and(|interrupt| interrupt.halted) {
                controller.recover_endpoint(handle);
                if controller.service() {
                    wake_service();
                }
            }
            Ok(())
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
        // completion a call waits for; the interrupt pipes' reports go to
        // their clients and the next polls go out. While a pipe waits for
        // its client's buffer, the channels are looked at every
        // `BUFFER_POLL_MS` (a client posts without a signal); a call that
        // leaves a pipe so signals `WAKE_BIT` to start that. While a pipe has
        // a TRB out, `WATCHDOG_MS` without an MSI runs the watchdog. Without
        // a controller it only waits.
        let mut starved = false;
        let mut polling = false;
        loop {
            let timeout = if starved {
                BUFFER_POLL_MS * 1_000_000
            } else if polling {
                WATCHDOG_MS * 1_000_000
            } else {
                NotificationKey::WAIT_INFINITE
            };
            let interrupted = match notification.await_bits(timeout) {
                Ok(bits) => bits & !WAKE_BIT != 0,
                Err(CapError::TimedOut { .. }) => false,
                Err(error) => panic!("usb-hcd-xhci: event wait failed: {:?}", error.code()),
            };
            if serviced {
                let mut turn = TURNSTILE.enter(init);
                if let Some(controller) = hcd(&mut turn).controller.as_mut() {
                    if interrupted {
                        controller.events.acknowledge();
                        let drained = controller.drain_unclaimed();
                        debug::msi(drained);
                    } else if !starved && polling {
                        controller.watchdog();
                    }
                    starved = controller.service();
                    polling = controller.polling();
                    debug::summary(&controller.events);
                }
            }
        }
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}

//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

// Host tests build this binary natively too; it is only an empty `main` there.
#![cfg_attr(target_os = "vesper", no_std, no_main)]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(all(target_os = "vesper", feature = "qemu"), feature(format_args_nl))]

//! EL0 driver component for the `DesignWare` DWC2 USB host controller.
//!
//! Kickstart loads one instance per enabled `brcm,bcm2708-usb` node, which is
//! the Raspberry Pi 3's controller. The instance maps its register page, puts
//! the core into host mode, powers the root port, and serves `usb_hcd_v1` (see
//! `interfaces/usb_hcd_v1.if`) to its clients. It does not enumerate: a client
//! (`usbd`) resets the port, opens a control pipe to address 0 and runs the
//! enumeration through `vesper_usb_protocol`.
//!
//! A call runs on the caller's Thread, one at a time (a `Turnstile`). Each
//! control transfer runs on host channel 0 by DMA, one endpoint 0 stage at a
//! time. The client stages the setup packet and the data stage in the data area
//! it shares with the component; the component finds the caller's area from the
//! stack the call runs on (see `libdevice_driver::calling_area`), and copies
//! between the area and its DMA page.
//!
//! Packet data moves by DMA through one page the driver owns: a frame retyped
//! from its own memory. The page is cacheable, so the driver cleans it before
//! the controller reads an OUT packet and invalidates it before the CPU reads
//! an IN reply. QEMU's model supports DMA only.
//!
//! Only control pipes are served. Interrupt and bulk pipes, and the split
//! transactions a routed pipe needs, come with the hub driver (USB3) and HID (USB4).
//! Completion is interrupt driven: the component's own Thread services the
//! controller's interrupt, and a transfer sleeps on an `EventCount` it advances.
//!
//! A pipe belongs to the client that opened it, identified by the base of its
//! data area. This identity stands in until Invocation badges exist.

#[cfg(target_os = "vesper")]
mod component {
    use {
        aarch64_cpu::registers::{CNTFRQ_EL0, CNTVCT_EL0, Readable},
        core::{
            cell::UnsafeCell,
            sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        },
        libdevice_driver::{
            DEVICE_WINDOW_VA, DriverInit, INIT_VA, MAX_DEVICE_PAGES, NO_BUS_ADDRESS, Turn,
            Turnstile, calling_area, device_address, driver_compatible, driver_init,
            map_device_pages,
        },
        libobject::{
            CapError, EventCountKey, FrameAttribute, FrameKey, IrqHandlerKey, KeyTableKey,
            NotificationKey, ObjectType, RawKey, Rights, UntypedKey, export::init_return_key,
            thread::ThreadReturnKey,
        },
        libuser::semihosting as semi,
        usb_hcd::usb_hcd_v1::{
            __runtime::{DataArea, IoRec},
            Exception, Speed, TransferKind, UsbHcdV1,
        },
        usb_hcd_dwc2::{
            control::{self, StageError},
            pipes::{Pipe, PipeError, PipeTable, Route},
            regs::{
                Channel, ChannelInterrupts, ChannelOutcome, GLOBAL_HOST_CHANNEL, MmioChannel,
                MmioRegisterBlock, Pid, RegisterBlock, RxFifoSize, Target, TxFifoSize,
                channel_event_mask, channel_offset, channel_outcome, characteristics,
                host_channel_bit, port_speed, port_status_bits, port_with_reset_and_power,
                transfer_size,
            },
        },
        vesper_usb_protocol::{
            descriptor::TransferKind as UsbKind, enumeration::Speed as UsbSpeed, setup::SetupPacket,
        },
    };

    libuser::entry!(main);

    driver_compatible!("brcm,bcm2708-usb");

    /// Host channel that runs every control transfer.
    const CHANNEL: usize = 0;
    /// Time one transfer may take before it is abandoned, in milliseconds.
    const TRANSFER_MS: u64 = 500;
    /// Times a transfer is issued again after a NAK or a transaction error.
    const ATTEMPTS: u32 = 3;
    /// Where the DMA page is mapped: the first device window page past the
    /// register pages.
    const DMA_VA: u64 = DEVICE_WINDOW_VA + MAX_DEVICE_PAGES as u64 * 4096;
    /// Offset in the DMA page of the SETUP packet.
    const SETUP_OFFSET: u64 = 0;
    /// Offset in the DMA page of the data stage and of the status stage.
    const DATA_OFFSET: u64 = 64;
    /// Bytes of data stage the DMA page holds.
    const DATA_CAPACITY: usize = 4032;
    const _: () = assert!(DATA_OFFSET + DATA_CAPACITY as u64 == 4096);
    /// Bytes copied at a time between the data area and the DMA page, which
    /// keeps the buffer small on the caller's stack.
    const COPY_BYTES: usize = 64;
    /// The one root port.
    const ROOT_PORT: u32 = 0;

    /// Ticks of the virtual counter, which EL0 may read (the physical counter
    /// is trapped for EL0 in `kickstart`).
    fn ticks() -> u64 {
        CNTVCT_EL0.get()
    }

    fn ticks_per_ms() -> u64 {
        CNTFRQ_EL0.get() / 1000
    }

    /// Nanoseconds in `ticks` ticks of the virtual counter.
    fn ticks_to_ns(ticks: u64) -> u64 {
        ticks.saturating_mul(1_000_000_000) / CNTFRQ_EL0.get().max(1)
    }

    /// Spin for `milliseconds`.
    fn delay_ms(milliseconds: u32) {
        let end = ticks() + ticks_per_ms() * u64::from(milliseconds);
        while ticks() < end {
            core::hint::spin_loop();
        }
    }

    /// Whether `done` held within `milliseconds`.
    fn wait_until(milliseconds: u64, mut done: impl FnMut() -> bool) -> bool {
        let end = ticks() + ticks_per_ms() * milliseconds;
        while !done() {
            if ticks() >= end {
                return false;
            }
            core::hint::spin_loop();
        }
        true
    }

    /// Wait until `done` holds, or panic after `milliseconds`. For bring-up,
    /// where there is no client to report to.
    fn wait_for(milliseconds: u64, what: &str, done: impl FnMut() -> bool) {
        assert!(
            wait_until(milliseconds, done),
            "usb-hcd-dwc2: timed out waiting for {what}"
        );
    }

    /// Host channel `index`'s registers, inside the register page at `base`.
    fn channel<'a>(base: usize, index: usize) -> MmioChannel<'a> {
        // SAFETY: the register page is mapped for the driver's lifetime, and
        // `channel_offset` names host channel `index`'s block inside it.
        unsafe {
            Channel::new_mmio(core::ptr::with_exposed_provenance_mut::<Channel>(
                base + channel_offset(index),
            ))
        }
    }

    /// Map the page the controller moves packet data through: a frame retyped
    /// from the driver's own memory. Returns the frame and the bus address
    /// `HCDMA` takes for its first byte.
    fn dma_page(init: &DriverInit, slot: u64) -> (FrameKey, u32) {
        assert_ne!(
            init.memory_bus, NO_BUS_ADDRESS,
            "usb-hcd-dwc2: the node's bus has no DMA addresses for its memory"
        );
        let table = KeyTableKey::from_key(RawKey::from_wire(init.table));
        let slot = u32::try_from(slot).expect("a slot fits in u32");
        let frame = FrameKey::from_key(
            UntypedKey::from_key(RawKey::from_wire(init.memory))
                .retype(
                    ObjectType::FRAME,
                    12,
                    0,
                    1,
                    &table,
                    slot,
                    Rights(Rights::READ | Rights::WRITE | Rights::GRANT),
                )
                .unwrap_or_else(|error| {
                    panic!("usb-hcd-dwc2: DMA frame Retype failed: {:?}", error.code())
                }),
        );
        let (paddr, _size) = frame
            .get_extent()
            .unwrap_or_else(|error| panic!("usb-hcd-dwc2: DMA frame extent: {:?}", error.code()));
        frame
            .map(
                RawKey::from_wire(init.address_space),
                DMA_VA,
                Rights(Rights::READ | Rights::WRITE),
                FrameAttribute::Default,
            )
            .unwrap_or_else(|error| {
                panic!("usb-hcd-dwc2: DMA frame Map failed: {:?}", error.code())
            });
        let offset = paddr
            .checked_sub(init.memory_paddr)
            .expect("the DMA frame lies in the driver's memory");
        let bus = init
            .memory_bus
            .checked_add(offset)
            .and_then(|bus| u32::try_from(bus).ok())
            .expect("the DMA frame has a 32-bit bus address");
        (frame, bus)
    }

    /// The controller, through its register page and the DMA page.
    struct Controller<'a> {
        registers: MmioRegisterBlock<'a>,
        /// The register page's base; host channels sit at offsets from it.
        base: usize,
        /// The DMA page, and its bus address.
        dma: FrameKey,
        dma_bus: u32,
        /// Counts the interrupts the interrupt Thread has serviced; a transfer
        /// sleeps on it.
        completions: EventCountKey,
    }

    impl Controller<'_> {
        /// Bring the core into host mode and DMA mode, with sized FIFOs.
        fn init_core(&mut self) {
            wait_for(100, "the AHB master to go idle", || {
                self.registers.read_reset().ahb_master_idle()
            });
            self.registers
                .modify_reset(|reset| reset.with_core_soft_reset(true));
            wait_for(100, "the core soft reset", || {
                !self.registers.read_reset().core_soft_reset()
            });

            self.registers.modify_usb_config(|config| {
                config
                    .with_force_device_mode(false)
                    .with_force_host_mode(true)
            });
            // `CurMod` is set in host mode.
            wait_for(500, "host mode", || {
                self.registers.read_interrupt_status().host_mode()
            });

            self.registers
                .modify_ahb_config(|ahb| ahb.with_dma_enable(true));
            self.registers
                .write_rx_fifo_size(RxFifoSize::ZERO.with_depth(0x100));
            self.registers
                .write_tx_fifo_size(TxFifoSize::ZERO.with_start(0x100).with_depth(0x100));

            // Flush every transmit FIFO, then the receive FIFO.
            self.registers.modify_reset(|reset| {
                reset
                    .with_tx_fifo_flush(true)
                    .with_tx_fifo_number(arbitrary_int::u5::new(0x10))
            });
            wait_for(100, "the transmit FIFO flush", || {
                !self.registers.read_reset().tx_fifo_flush()
            });
            self.registers
                .modify_reset(|reset| reset.with_rx_fifo_flush(true));
            wait_for(100, "the receive FIFO flush", || {
                !self.registers.read_reset().rx_fifo_flush()
            });

            // Host channel events reach the CPU through the controller's
            // interrupt line, which the interrupt Thread services. Port events
            // stay masked.
            self.registers
                .write_host_channel_interrupt_mask(host_channel_bit(CHANNEL));
            self.registers.write_interrupt_mask(GLOBAL_HOST_CHANNEL);
            self.registers
                .modify_ahb_config(|ahb| ahb.with_global_interrupt_mask(true));
        }

        /// Power the root port and wait for a device to connect.
        fn power_port(&mut self) {
            let port = self.registers.read_port();
            self.registers
                .write_port(port_with_reset_and_power(port, false, true));
            wait_for(500, "a device on the root port", || {
                self.registers.read_port().connected()
            });
        }

        /// Reset the root port `port` and report the speed of the device on it.
        fn reset_port(&mut self, port: u32) -> Result<Speed, Exception> {
            if port != ROOT_PORT || !self.registers.read_port().connected() {
                return Err(Exception::NoDevice);
            }
            // USB 2.0 §7.1.7.5: hold reset for at least 50 ms.
            let control = self.registers.read_port();
            self.registers
                .write_port(port_with_reset_and_power(control, true, true));
            delay_ms(60);

            let control = self.registers.read_port();
            self.registers
                .write_port(port_with_reset_and_power(control, false, true));
            if !wait_until(100, || self.registers.read_port().enabled()) {
                return Err(Exception::NoDevice);
            }

            // Acknowledge the enable change the reset caused.
            let control = self.registers.read_port();
            self.registers
                .write_port(control.without_events().with_enable_changed(true));
            match port_speed(self.registers.read_port()) {
                Some(UsbSpeed::Low) => Ok(Speed::Low),
                Some(UsbSpeed::Full) => Ok(Speed::Full),
                Some(UsbSpeed::High) => Ok(Speed::High),
                None => Err(Exception::NoDevice),
            }
        }

        /// The status bits of root port `port` (see `port_status_bits`). A port
        /// that does not exist reports nothing.
        fn port_status(&self, port: u32) -> u32 {
            if port == ROOT_PORT {
                port_status_bits(self.registers.read_port())
            } else {
                0
            }
        }

        /// Halt host channel `index` and wait until it has.
        fn halt(&mut self, index: usize, target: Target) {
            let mut channel = channel(self.base, index);
            // Nothing the halt raises is of interest: the interrupt Thread stops
            // recording, and the channel stops raising.
            ARMED.store(false, Ordering::Release);
            channel.write_interrupt_mask(ChannelInterrupts::ZERO);
            channel.write_characteristics(characteristics(target, true).with_disable(true));
            wait_for(10, "the channel to halt", || {
                channel.read_interrupt().halted()
            });
            channel.write_interrupt(ChannelInterrupts::new_with_raw_value(u32::MAX));
        }

        /// Clean (`write_back`) or invalidate the DMA page's bytes `start..end`,
        /// as byte offsets into the frame. An empty range needs no maintenance.
        fn maintain(&self, write_back: bool, start: u64, end: u64) {
            if start >= end {
                return;
            }
            let result = if write_back {
                self.dma.clean_data(start, end)
            } else {
                self.dma.invalidate_data(start, end)
            };
            result.unwrap_or_else(|error| {
                panic!("usb-hcd-dwc2: cache maintenance failed: {:?}", error.code())
            });
        }

        /// The DMA page's byte at `offset`.
        fn dma_byte(offset: u64) -> *mut u8 {
            core::ptr::with_exposed_provenance_mut::<u8>(
                usize::try_from(DMA_VA + offset).expect("a 64-bit address"),
            )
        }

        /// Put `bytes` in the DMA page at `offset`.
        fn dma_write(offset: u64, bytes: &[u8]) {
            for (index, &byte) in bytes.iter().enumerate() {
                // SAFETY: the DMA page is mapped read/write and only this
                // driver uses it; callers keep `offset + bytes.len()` inside it.
                unsafe {
                    Self::dma_byte(offset).add(index).write_volatile(byte);
                }
            }
        }

        /// Fill `bytes` from the DMA page at `offset`.
        fn dma_read(offset: u64, bytes: &mut [u8]) {
            for (index, byte) in bytes.iter_mut().enumerate() {
                // SAFETY: as in `dma_write`.
                *byte = unsafe { Self::dma_byte(offset).add(index).read_volatile() };
            }
        }

        /// Run one transfer of `bytes` on host channel `index` to `target`, in
        /// packets of `target.max_packet`, by DMA. The bytes sit in the DMA page
        /// at `offset`: an OUT transfer sends what the caller put there, an IN
        /// transfer leaves the reply there. Returns the bytes moved.
        fn run_channel(
            &mut self,
            index: usize,
            target: Target,
            pid: Pid,
            offset: u64,
            bytes: usize,
        ) -> Result<usize, Exception> {
            let packets = u16::try_from(control::packet_count(
                u32::try_from(bytes).unwrap_or(u32::MAX),
                u32::from(target.max_packet),
            ))
            .unwrap_or(u16::MAX);
            let length = u64::try_from(bytes).unwrap_or(u64::MAX);
            if target.input {
                // The controller writes the page behind the CPU's back.
                self.maintain(false, offset, offset + length);
            } else {
                // The controller reads the page from memory, not the cache.
                self.maintain(true, offset, offset + length);
            }
            let dma_address = self
                .dma_bus
                .wrapping_add(u32::try_from(offset).unwrap_or(u32::MAX));

            let mut channel = channel(self.base, index);
            let mut status = 0;
            for _attempt in 0..ATTEMPTS {
                // Quiesce the channel, then arm the interrupt Thread to record its
                // events for this attempt only.
                channel.write_interrupt_mask(ChannelInterrupts::ZERO);
                channel.write_interrupt(ChannelInterrupts::new_with_raw_value(u32::MAX));
                STATUS.store(0, Ordering::Release);
                channel.write_dma_address(dma_address);
                channel.write_transfer_size(transfer_size(
                    u32::try_from(bytes).unwrap_or(u32::MAX),
                    packets,
                    pid,
                ));
                // Interrupts the interrupt Thread has counted so far; the next one
                // may belong to this attempt.
                let mut seen = INTERRUPTS.load(Ordering::Acquire);
                ARMED.store(true, Ordering::Release);
                channel.write_interrupt_mask(channel_event_mask());
                channel.write_characteristics(characteristics(target, true));

                let deadline = ticks() + ticks_per_ms() * TRANSFER_MS;
                loop {
                    let interrupts =
                        ChannelInterrupts::new_with_raw_value(STATUS.load(Ordering::Acquire));
                    status = interrupts.raw_value();
                    match channel_outcome(interrupts) {
                        Some(ChannelOutcome::Complete) => {
                            ARMED.store(false, Ordering::Release);
                            channel.write_interrupt_mask(ChannelInterrupts::ZERO);
                            channel
                                .write_interrupt(ChannelInterrupts::new_with_raw_value(u32::MAX));
                            let remaining = channel.read_transfer_size().bytes().value();
                            let moved =
                                bytes.saturating_sub(usize::try_from(remaining).unwrap_or(bytes));
                            if target.input {
                                // The reply is in memory now; drop stale lines.
                                self.maintain(
                                    false,
                                    offset,
                                    offset + u64::try_from(moved).unwrap_or(u64::MAX),
                                );
                            }
                            return Ok(moved);
                        }
                        Some(ChannelOutcome::Stalled) => {
                            self.halt(index, target);
                            return Err(Exception::Stalled);
                        }
                        Some(ChannelOutcome::Failed | ChannelOutcome::Halted) => {
                            self.halt(index, target);
                            return Err(Exception::TransferFailed { status });
                        }
                        Some(ChannelOutcome::Retry) => {
                            self.halt(index, target);
                            break;
                        }
                        None => {}
                    }
                    // Sleep until the interrupt Thread counts another interrupt or
                    // the transfer's deadline passes.
                    let now = ticks();
                    if now >= deadline {
                        self.halt(index, target);
                        return Err(Exception::TransferFailed { status });
                    }
                    match self
                        .completions
                        .await_ge(seen + 1, ticks_to_ns(deadline - now).max(1))
                    {
                        Ok(count) => seen = count,
                        Err(CapError::TimedOut { .. }) => {
                            self.halt(index, target);
                            return Err(Exception::TransferFailed { status });
                        }
                        Err(error) => {
                            panic!("usb-hcd-dwc2: completion wait failed: {:?}", error.code())
                        }
                    }
                }
            }
            Err(Exception::TransferFailed { status })
        }

        /// Copy `length` bytes from `area` at `from` to the DMA page at `to`.
        fn copy_in(area: &DataArea, from: u64, to: u64, length: usize) -> bool {
            let mut chunk = [0_u8; COPY_BYTES];
            let mut done = 0;
            while done < length {
                let count = COPY_BYTES.min(length - done);
                let step = u64::try_from(done).unwrap_or(u64::MAX);
                if !area.read_bytes(from + step, &mut chunk[..count]) {
                    return false;
                }
                Self::dma_write(to + step, &chunk[..count]);
                done += count;
            }
            true
        }

        /// Copy `length` bytes from the DMA page at `from` to `area` at `to`.
        fn copy_out(area: &DataArea, from: u64, to: u64, length: usize) -> bool {
            let mut chunk = [0_u8; COPY_BYTES];
            let mut done = 0;
            while done < length {
                let count = COPY_BYTES.min(length - done);
                let step = u64::try_from(done).unwrap_or(u64::MAX);
                Self::dma_read(from + step, &mut chunk[..count]);
                if !area.write_bytes(to + step, &chunk[..count]) {
                    return false;
                }
                done += count;
            }
            true
        }

        /// Run one control transfer on `pipe`: SETUP, the data stage if the
        /// request has one, then the status stage. The stage buffer is `stage`
        /// bytes of `area` from byte `offset`: the setup packet, then the data
        /// stage, which is read (OUT) or written (IN) in place. Returns the
        /// bytes of the data stage that moved.
        fn control(
            &mut self,
            pipe: &Pipe,
            area: &DataArea,
            offset: u64,
            stage: usize,
        ) -> Result<usize, Exception> {
            if stage < control::SETUP_BYTES {
                return Err(Exception::NotPermitted);
            }
            let mut packet = [0_u8; control::SETUP_BYTES];
            if !area.read_bytes(offset, &mut packet) {
                return Err(Exception::NotPermitted);
            }
            let setup = SetupPacket::from_bytes(packet);
            let length = match control::data_length(stage, setup, DATA_CAPACITY) {
                Ok(length) => length,
                Err(StageError::TooShort | StageError::TooLong) => {
                    return Err(Exception::NotPermitted);
                }
            };
            if !self.registers.read_port().connected() {
                return Err(Exception::NoDevice);
            }

            let stages = control::plan(setup);
            let target = |input: bool| Target {
                device_address: pipe.device_address,
                endpoint: pipe.endpoint,
                input,
                low_speed: pipe.speed == UsbSpeed::Low,
                kind: UsbKind::Control,
                max_packet: pipe.max_packet,
            };
            let data_offset = offset + control::SETUP_BYTES as u64;

            Self::dma_write(SETUP_OFFSET, &setup.to_bytes());
            self.run_channel(
                CHANNEL,
                target(false),
                Pid::Setup,
                SETUP_OFFSET,
                control::SETUP_BYTES,
            )?;

            let mut moved = 0;
            if stages.has_data {
                if stages.data_input {
                    moved =
                        self.run_channel(CHANNEL, target(true), Pid::Data1, DATA_OFFSET, length)?;
                    if !Self::copy_out(area, DATA_OFFSET, data_offset, moved) {
                        return Err(Exception::NotPermitted);
                    }
                } else {
                    if !Self::copy_in(area, data_offset, DATA_OFFSET, length) {
                        return Err(Exception::NotPermitted);
                    }
                    self.run_channel(CHANNEL, target(false), Pid::Data1, DATA_OFFSET, length)?;
                    moved = length;
                }
            }
            self.run_channel(
                CHANNEL,
                target(stages.status_input),
                Pid::Data1,
                DATA_OFFSET,
                0,
            )?;
            Ok(moved)
        }
    }

    /// What the calls share, used only while holding a turn.
    struct Hcd {
        controller: Controller<'static>,
        pipes: PipeTable,
    }

    struct Shared(UnsafeCell<Option<Hcd>>);

    // SAFETY: the cell is written once, before the component says it is ready,
    // and then used only by the holder of a `Turnstile` turn.
    unsafe impl Sync for Shared {}

    static HCD: Shared = Shared(UnsafeCell::new(None));
    static TURNSTILE: Turnstile = Turnstile::new();

    /// Whether the interrupt Thread records channel events: set while a
    /// transfer attempt is armed, clear otherwise, so a late interrupt of a
    /// finished attempt cannot reach the next one.
    static ARMED: AtomicBool = AtomicBool::new(false);
    /// The channel events recorded since the attempt was armed, as a
    /// `ChannelInterrupts` value.
    static STATUS: AtomicU32 = AtomicU32::new(0);
    /// Interrupts the interrupt Thread has serviced. The `completions`
    /// `EventCount` counts the same, one `Advance` each, after `STATUS` is
    /// updated.
    static INTERRUPTS: AtomicU64 = AtomicU64::new(0);

    fn init() -> &'static DriverInit {
        // SAFETY: Kickstart maps every driver's init record at `INIT_VA`; it
        // is read in this component's own `AddressSpace` only.
        unsafe { driver_init(INIT_VA) }
    }

    /// The controller and pipes, for the holder of `turn`.
    #[allow(
        clippy::needless_pass_by_ref_mut,
        reason = "the exclusive borrow of the turn proves the caller holds it"
    )]
    fn hcd<'turn>(_turn: &'turn mut Turn<'_>) -> &'turn mut Hcd {
        // SAFETY: only the holder of a turn gets here, one at a time, and the
        // cell was filled before the component became ready.
        unsafe { (*HCD.0.get()).as_mut() }.expect("the controller is set up before it is served")
    }

    /// The caller's data area, and the identity the pipes it opens belong to.
    fn caller() -> Result<(DataArea, u64), Exception> {
        let area = calling_area(init()).ok_or(Exception::NotPermitted)?;
        Ok((
            DataArea {
                base: area.base,
                len: area.len,
            },
            area.base,
        ))
    }

    fn pipe_error(error: PipeError) -> Exception {
        match error {
            PipeError::Invalid => Exception::InvalidPipe,
            PipeError::Full => Exception::Busy,
        }
    }

    /// The host controller.
    struct Dwc2;

    impl UsbHcdV1 for Dwc2 {
        fn enter(_badge: u64) {
            init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
                init().return_key,
            )));
        }

        fn reset_port(port: u32) -> Result<Speed, Exception> {
            let mut turn = TURNSTILE.enter(init());
            hcd(&mut turn).controller.reset_port(port)
        }

        fn port_status(port: u32) -> u32 {
            let mut turn = TURNSTILE.enter(init());
            hcd(&mut turn).controller.port_status(port)
        }

        fn open_pipe(
            device_address: u32,
            endpoint: u32,
            speed: Speed,
            kind: TransferKind,
            max_packet: u16,
            _interval_ms: u32,
        ) -> Result<u32, Exception> {
            let (_area, owner) = caller()?;
            // Interrupt and bulk pipes come with HID and mass storage.
            if !matches!(kind, TransferKind::Control) {
                return Err(Exception::NotPermitted);
            }
            // A control endpoint's packet is 8, 16, 32 or 64 bytes.
            let address = u8::try_from(device_address).ok().filter(|&a| a <= 127);
            let endpoint = u8::try_from(endpoint).ok().filter(|&e| e <= 15);
            let (Some(device_address), Some(endpoint), true) =
                (address, endpoint, matches!(max_packet, 8 | 16 | 32 | 64))
            else {
                return Err(Exception::NotPermitted);
            };
            let speed = match speed {
                Speed::Low => UsbSpeed::Low,
                Speed::Full => UsbSpeed::Full,
                Speed::High => UsbSpeed::High,
            };
            let mut turn = TURNSTILE.enter(init());
            hcd(&mut turn)
                .pipes
                .open(Pipe {
                    device_address,
                    endpoint,
                    speed,
                    kind: UsbKind::Control,
                    max_packet,
                    owner,
                    route: None,
                })
                .map_err(pipe_error)
        }

        fn set_route(pipe: u32, hub_address: u32, hub_port: u32) -> Result<(), Exception> {
            let (_area, owner) = caller()?;
            let (Some(hub_address), Some(hub_port)) = (
                u8::try_from(hub_address)
                    .ok()
                    .filter(|a| (1..=127).contains(a)),
                u8::try_from(hub_port).ok().filter(|&p| p >= 1),
            ) else {
                return Err(Exception::NotPermitted);
            };
            let mut turn = TURNSTILE.enter(init());
            let pipe = hcd(&mut turn)
                .pipes
                .get_mut(pipe, owner)
                .map_err(pipe_error)?;
            // Split transactions serve devices slower than the hub.
            if pipe.speed == UsbSpeed::High {
                return Err(Exception::NotPermitted);
            }
            pipe.route = Some(Route {
                hub_address,
                hub_port,
            });
            Ok(())
        }

        fn close_pipe(pipe: u32) -> Result<(), Exception> {
            let (_area, owner) = caller()?;
            let mut turn = TURNSTILE.enter(init());
            hcd(&mut turn).pipes.close(pipe, owner).map_err(pipe_error)
        }

        fn clear_halt(pipe: u32) -> Result<(), Exception> {
            let (_area, owner) = caller()?;
            let mut turn = TURNSTILE.enter(init());
            // A control pipe starts every transfer with DATA0 SETUP, so a stall
            // leaves no toggle to reset; the halt itself is cleared by the
            // device's CLEAR_FEATURE request through `control`.
            hcd(&mut turn)
                .pipes
                .get(pipe, owner)
                .map(|_pipe| ())
                .map_err(pipe_error)
        }

        fn control(pipe: u32, stage: IoRec) -> Result<u32, Exception> {
            let (area, owner) = caller()?;
            if !area.contains(&stage) {
                return Err(Exception::NotPermitted);
            }
            let stage_len =
                usize::try_from(stage.len).map_err(|_overflow| Exception::NotPermitted)?;
            let mut turn = TURNSTILE.enter(init());
            let hcd = hcd(&mut turn);
            let pipe = *hcd.pipes.get(pipe, owner).map_err(pipe_error)?;
            // Split transactions are not implemented: a routed pipe waits for
            // the hub driver.
            if pipe.route.is_some() {
                return Err(Exception::NotPermitted);
            }
            hcd.controller
                .control(&pipe, &area, stage.base - area.base, stage_len)
                .map(|moved| u32::try_from(moved).unwrap_or(u32::MAX))
        }
    }

    usb_hcd::usb_hcd_v1_export!(Dwc2);

    /// Set up the controller and the turnstile, then service the controller's
    /// interrupt: calls run on their
    /// callers' Threads.
    fn main(init_va: u64) -> ! {
        // SAFETY: Kickstart passes the init page's address in x0.
        let init = unsafe { driver_init(init_va) };
        let next_slot = map_device_pages(init).unwrap_or_else(|error| {
            panic!("usb-hcd-dwc2: mapping registers failed: {:?}", error.code())
        });
        // The controller's register block is the node's largest range: the
        // node also names a 4 KiB range that is not the DWC2 window.
        let registers_paddr = init
            .regions()
            .iter()
            .max_by_key(|region| region.size)
            .unwrap_or_else(|| panic!("usb-hcd-dwc2: no register range"))
            .start;
        let registers = device_address(init, registers_paddr).expect("registers mapped");
        let base = usize::try_from(registers).expect("a register address fits in usize");
        let (dma, dma_bus) = dma_page(init, next_slot);
        let completions_key = UntypedKey::from_key(RawKey::from_wire(init.memory))
            .retype(
                ObjectType::EVENT_COUNT,
                0,
                0,
                1,
                &KeyTableKey::from_key(RawKey::from_wire(init.table)),
                u32::try_from(next_slot + 2).expect("a slot fits in u32"),
                Rights::all(),
            )
            .unwrap_or_else(|error| {
                panic!(
                    "usb-hcd-dwc2: completions Retype failed: {:?}",
                    error.code()
                )
            });
        let mut controller = Controller {
            // SAFETY: `base` is the register page `map_device_pages` mapped
            // read/write as device memory for the driver's lifetime.
            registers: unsafe {
                RegisterBlock::new_mmio(core::ptr::with_exposed_provenance_mut::<RegisterBlock>(
                    base,
                ))
            },
            base,
            dma,
            dma_bus,
            completions: EventCountKey::from_key(completions_key),
        };
        controller.init_core();
        controller.power_port();

        let turns = UntypedKey::from_key(RawKey::from_wire(init.memory))
            .retype(
                ObjectType::EVENT_COUNT,
                0,
                0,
                1,
                &KeyTableKey::from_key(RawKey::from_wire(init.table)),
                u32::try_from(next_slot + 1).expect("a slot fits in u32"),
                Rights::all(),
            )
            .unwrap_or_else(|error| {
                panic!("usb-hcd-dwc2: turns Retype failed: {:?}", error.code())
            });
        TURNSTILE.open(turns);
        // SAFETY: nothing else touches the cell until the component says it is
        // ready, just below.
        unsafe {
            *HCD.0.get() = Some(Hcd {
                controller,
                pipes: PipeTable::new(),
            });
        }
        EventCountKey::from_key(RawKey::from_wire(init.event_count))
            .advance(1)
            .unwrap_or_else(|error| {
                panic!("usb-hcd-dwc2: ready advance failed: {:?}", error.code())
            });
        semi::println!(
            "usb-hcd-dwc2: serving usb_hcd_v1: {} data areas",
            init.areas().len()
        );

        // The interrupt Thread: this one. The controller's line stays masked until
        // the first `Ack`; each interrupt is then serviced here and the line
        // acknowledged again. The Thread reads the host-channel summary and
        // clears the events of the channel it saw; the calling Threads program the
        // channel and sleep on `completions`.
        // SAFETY: the registers stay mapped as device memory for the driver's
        // lifetime; this Thread uses only the host-channel interrupt summary and
        // the interrupt register of the transfer channel.
        let service_registers = unsafe {
            RegisterBlock::new_mmio(core::ptr::with_exposed_provenance_mut::<RegisterBlock>(
                base,
            ))
        };
        let handlers = init.interrupts();
        let acknowledge = || {
            for interrupt in handlers {
                IrqHandlerKey::from_key(RawKey::from_wire(interrupt.handler))
                    .ack()
                    .unwrap_or_else(|error| {
                        panic!("usb-hcd-dwc2: interrupt Ack failed: {:?}", error.code())
                    });
            }
        };
        acknowledge();
        let interrupts = NotificationKey::from_key(RawKey::from_wire(init.notification));
        loop {
            interrupts
                .await_bits(NotificationKey::WAIT_INFINITE)
                .unwrap_or_else(|error| {
                    panic!("usb-hcd-dwc2: interrupt wait failed: {:?}", error.code())
                });
            if service_registers.read_host_channel_interrupt() & host_channel_bit(CHANNEL) != 0 {
                let mut transfer = channel(base, CHANNEL);
                let events = transfer.read_interrupt();
                // Clear exactly the events seen: a bit that arrives meanwhile
                // raises the line again.
                transfer.write_interrupt(events);
                if ARMED.load(Ordering::Acquire) {
                    STATUS.fetch_or(events.raw_value(), Ordering::AcqRel);
                }
            }
            // Counted even when nothing was recorded, so a sleeping transfer
            // re-reads `STATUS`.
            INTERRUPTS.fetch_add(1, Ordering::AcqRel);
            EventCountKey::from_key(completions_key)
                .advance(1)
                .unwrap_or_else(|error| {
                    panic!(
                        "usb-hcd-dwc2: completion advance failed: {:?}",
                        error.code()
                    )
                });
            acknowledge();
        }
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}

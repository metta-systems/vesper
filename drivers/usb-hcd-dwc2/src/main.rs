//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

// Host tests build this binary natively too; it is only an empty `main` there.
#![cfg_attr(target_os = "vesper", no_std, no_main)]

//! EL0 driver component for the `DesignWare` DWC2 USB host controller.
//!
//! Kickstart loads one instance per enabled `brcm,bcm2708-usb` node, which is
//! the Raspberry Pi 3's controller. The instance maps its register page, puts
//! the core into host mode, powers the root port, and serves `usb_hcd_v1` and
//! `pipe_v1` (see `interfaces/`) to its clients. It does not enumerate: a client
//! (`usb-core`) resets the port, opens a control pipe to address 0 and runs the
//! enumeration through `vesper_usb_protocol`.
//!
//! A call runs on the caller's Thread, one at a time (a `Turnstile`). Each
//! control transfer runs on host channel 0 by DMA, one endpoint 0 stage at a
//! time. The client stages the setup packet and the data stage in the data area
//! it shares with the component; the component finds the caller's area from the
//! badge of the key the call came through (`DriverInit::area_for_badge`), and copies
//! between the area and its DMA page.
//!
//! Packet data moves by DMA through one page the driver owns: a frame retyped
//! from its own memory. The page is cacheable, so the driver cleans it before
//! the controller reads an OUT packet and invalidates it before the CPU reads
//! an IN reply. QEMU's model supports DMA only.
//!
//! Control and interrupt-IN pipes are served; bulk pipes are not. A pipe routed
//! through a high-speed hub (`set_route`) runs one split transaction per packet
//! (see `split`).
//! Completion is interrupt driven: the component's own Thread services the
//! controller's interrupt, and a transfer sleeps on an `EventCount` it advances.
//!
//! A second Thread polls the interrupt pipes (see `periodic`): when a pipe is due
//! and its client has posted an empty buffer on the report channel, it runs one
//! IN transaction on the same host channel, under the same turnstile as the
//! calls, and returns the report in the buffer. A NAK is not an error, and the
//! pipe is due again an interval later. Each interrupt transaction is placed in
//! the next (micro)frame (`HCCHAR.OddFrm` from `HFNUM`).
//!
//! A pipe is the set of `pipe_v1` Invocation keys `open_pipe` installs in the
//! table the caller lends, badged with the pipe's handle: holding a key is the
//! right to use the pipe. The component keeps one master key per client and
//! function (see `libdevice_driver::Masters`) and derives the badged copies.

/// `note!("text {value}")`: one trace line, on the semihosting console and on
/// the log channel when the image has a log server, else in the backlog
/// `usb_hcd_v1.trace` drains.
///
/// What is traced: the bring-up (power domain, core reset, root port power),
/// each root port reset, each interrupt pipe scheduled, and failures. Nothing
/// is traced per transfer or per poll; repeating failures and slow transfers
/// are traced the 1st, 2nd, 4th, 8th... time with their count (see
/// `counted`), so a dead device cannot flood the log.
#[cfg(target_os = "vesper")]
macro_rules! note {
    ($($argument:tt)*) => {
        libserial_trace::note(format_args!($($argument)*))
    };
}

#[cfg(target_os = "vesper")]
mod component {
    use {
        aarch64_cpu::registers::{CNTFRQ_EL0, CNTVCT_EL0, Readable},
        core::{
            cell::UnsafeCell,
            sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        },
        libdevice_driver::{
            DEVICE_WINDOW_VA, DriverInit, ExportedArea, INIT_VA, MAX_DEVICE_PAGES, Masters,
            NO_BUS_ADDRESS, Priority, Turn, Turnstile, device_address, driver_compatible,
            driver_init, driver_threads, entry_priority, map_device_pages,
        },
        libio_channel::{Cursor, NoEvent, Packet},
        libobject::{
            AddressSpaceKey, CapError, EventCountKey, FrameAttribute, FrameKey, IrqHandlerKey,
            KeyTableKey, NotificationKey, ObjectType, RawKey, Rights, StackPoolHandle, UntypedKey,
            export::init_return_key, thread::ThreadReturnKey,
        },
        mailbox_client::mailbox_v1::{Bindings, Client, Device},
        usb_hcd::{
            pipe_v1::{self, Exception as PipeException, PipeV1, TransferKind},
            usb_hcd_v1::{
                __runtime::{DataArea, IoRec},
                Exception, Speed, UsbHcdV1,
            },
        },
        usb_hcd_dwc2::{
            control::{self, StageError},
            periodic::{Polled, Poller, ScheduleError, is_pollable, transaction_us},
            pipes::{Config, Pipe, PipeError, PipeTable, Route},
            regs::{
                Channel, ChannelInterrupts, ChannelOutcome, GLOBAL_HOST_CHANNEL, MmioChannel,
                MmioRegisterBlock, Pid, PollOutcome, RegisterBlock, RxFifoSize, Split, SplitRoute,
                Target, TxFifoSize, channel_event_mask, channel_offset, channel_outcome,
                characteristics, host_channel_bit, poll_outcome, port_speed, port_status_bits,
                port_with_reset_and_power, split_control, split_event_mask, transfer_size,
            },
            split::{
                Event, Phase, Sequence, Step, event_of, failure_status, next_pid, packet_bytes,
            },
        },
        vesper_usb_protocol::{
            descriptor::TransferKind as UsbKind,
            enumeration::Speed as UsbSpeed,
            report_channel::{ReportChannel, data_area as report_data_area},
            setup::SetupPacket,
        },
    };

    libuser::entry!(main);

    driver_compatible!("brcm,bcm2708-usb");

    // The poller Thread polls the interrupt pipes while the first Thread
    // services the controller's interrupt. Both are on the path of every key
    // and run ahead of the normal Threads: a split poll's complete split must
    // follow its start split within a few microframes (USB 2.0 §11.18.4), and
    // each phase wakes the interrupt Thread, then the poller. Both block
    // between interrupts and polls and do little work each time.
    entry_priority!(Priority::INPUT);
    driver_threads!(poller_main => Priority::INPUT);

    /// Host channel that runs every control transfer.
    const CHANNEL: usize = 0;
    /// Time one transfer may take before it is abandoned, in milliseconds.
    const TRANSFER_MS: u64 = 500;
    /// How long a transfer is issued again after a NAK or a transaction error,
    /// in milliseconds. A device may NAK a stage of a control transfer while it
    /// is busy (USB 2.0 §9.2.6.4 allows it 5 s for a data stage and 50 ms for the
    /// status stage); a hub-attached device that is still starting needs this.
    const RETRY_MS: u64 = 1000;
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

    /// The frequency of the virtual counter in Hz: the register, or the Raspberry
    /// Pi's 19.2 MHz where the firmware left it zero. With a zero every timeout
    /// here would be none at all, and the first bring-up wait would give up.
    fn counter_frequency() -> u64 {
        match CNTFRQ_EL0.get() {
            0 => 19_200_000,
            frequency => frequency,
        }
    }

    /// What came before the failing run of a split packet, in the bits above the
    /// channel's events: the events of the last start split (bits 12 to 21), the
    /// complete splits answered NYET (bits 22 to 24, at most 7) and the times the
    /// transaction was started over (bits 28 and 29, at most 3).
    fn split_history(start_events: u32, not_yet: u32, restarts: u32) -> u32 {
        ((start_events & 0x3ff) << 12) | (not_yet.min(7) << 22) | (restarts.min(3) << 28)
    }

    fn ticks_per_ms() -> u64 {
        counter_frequency() / 1000
    }

    /// Nanoseconds in `ticks` ticks of the virtual counter.
    fn ticks_to_ns(ticks: u64) -> u64 {
        ticks.saturating_mul(1_000_000_000) / counter_frequency()
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

    /// Count one more event on `counter`; returns the count and whether this
    /// one is traced: the 1st, 2nd, 4th, 8th..., so an event that keeps
    /// repeating adds a line per doubling, not one per retry.
    fn counted(counter: &AtomicU32) -> (u32, bool) {
        let count = counter.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        (count, count.is_power_of_two())
    }

    /// A step of the controller's bring-up. Its number is what
    /// `usb_hcd_v1`'s `controller_failed` reports when the step times out.
    #[derive(Clone, Copy, Debug)]
    enum BringUp {
        AhbMasterIdle = 1,
        CoreSoftReset = 2,
        HostMode = 3,
        TransmitFifoFlush = 4,
        ReceiveFifoFlush = 5,
    }

    /// Wait until `done` holds. For bring-up, where a timeout is not a panic
    /// nobody sees but a failure the controller reports from every call.
    ///
    /// # Errors
    ///
    /// The `step` that did not finish within `milliseconds`.
    fn wait_for(
        milliseconds: u64,
        step: BringUp,
        done: impl FnMut() -> bool,
    ) -> Result<(), BringUp> {
        if wait_until(milliseconds, done) {
            Ok(())
        } else {
            Err(step)
        }
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
        ///
        /// # Errors
        ///
        /// The step that timed out; the controller is then not usable.
        fn init_core(&mut self) -> Result<(), BringUp> {
            wait_for(100, BringUp::AhbMasterIdle, || {
                self.registers.read_reset().ahb_master_idle()
            })?;
            self.registers
                .modify_reset(|reset| reset.with_core_soft_reset(true));
            wait_for(100, BringUp::CoreSoftReset, || {
                !self.registers.read_reset().core_soft_reset()
            })?;

            self.registers.modify_usb_config(|config| {
                config
                    .with_force_device_mode(false)
                    .with_force_host_mode(true)
            });
            // `CurMod` is set in host mode.
            wait_for(500, BringUp::HostMode, || {
                self.registers.read_interrupt_status().host_mode()
            })?;

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
            wait_for(100, BringUp::TransmitFifoFlush, || {
                !self.registers.read_reset().tx_fifo_flush()
            })?;
            self.registers
                .modify_reset(|reset| reset.with_rx_fifo_flush(true));
            wait_for(100, BringUp::ReceiveFifoFlush, || {
                !self.registers.read_reset().rx_fifo_flush()
            })?;

            // Host channel events reach the CPU through the controller's
            // interrupt line, which the interrupt Thread services. Port events
            // stay masked.
            self.registers
                .write_host_channel_interrupt_mask(host_channel_bit(CHANNEL));
            self.registers.write_interrupt_mask(GLOBAL_HOST_CHANNEL);
            self.registers
                .modify_ahb_config(|ahb| ahb.with_global_interrupt_mask(true));
            Ok(())
        }

        /// Milliseconds the root port waits for a device after power-up.
        const CONNECT_MS: u64 = 3000;

        /// Power the root port and give a device time to connect. A hub behind
        /// the port (the Raspberry Pi's) takes seconds to come up; a device that
        /// connects later shows in `port_status`. Traces whether one connected.
        fn power_port(&mut self) {
            let port = self.registers.read_port();
            self.registers
                .write_port(port_with_reset_and_power(port, false, true));
            let started = now_ms();
            let connected = wait_until(Self::CONNECT_MS, || self.registers.read_port().connected());
            let port = self.registers.read_port().raw_value();
            if connected {
                note!(
                    "usb-hcd-dwc2: root port powered, device connected after {} ms (port {port:#010x})",
                    now_ms() - started
                );
            } else {
                note!(
                    "usb-hcd-dwc2: root port powered, nothing connected within {} ms (port {port:#010x})",
                    Self::CONNECT_MS
                );
            }
        }

        /// Reset the root port `port` and report the speed of the device on it.
        /// The outcome is traced with the port register.
        fn reset_port(&mut self, port: u32) -> Result<Speed, Exception> {
            if port != ROOT_PORT {
                return Err(Exception::NoDevice);
            }
            if !self.registers.read_port().connected() {
                note!(
                    "usb-hcd-dwc2: port reset: no device connected (port {:#010x})",
                    self.registers.read_port().raw_value()
                );
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
                note!(
                    "usb-hcd-dwc2: port reset: not enabled within 100 ms (port {:#010x})",
                    self.registers.read_port().raw_value()
                );
                return Err(Exception::NoDevice);
            }

            // Acknowledge the enable change the reset caused.
            let control = self.registers.read_port();
            self.registers
                .write_port(control.without_events().with_enable_changed(true));
            let status = self.registers.read_port();
            let found = port_speed(status);
            match found {
                Some(speed) => note!(
                    "usb-hcd-dwc2: port reset: enabled, {speed:?} speed (port {:#010x})",
                    status.raw_value()
                ),
                None => note!(
                    "usb-hcd-dwc2: port reset: enabled, reserved speed bits (port {:#010x})",
                    status.raw_value()
                ),
            }
            match found {
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
            // shortcut: a channel that does not halt is left as it is, since a
            // panic here would hold the turnstile for good; reset the channel
            // instead once the case is seen.
            if !wait_until(10, || channel.read_interrupt().halted()) {
                let (count, report) = counted(&HALT_FAILURES);
                if report {
                    note!(
                        "usb-hcd-dwc2: channel {index} did not halt ({count} so far): characteristics {:#010x} split {:#010x} events {:#010x}",
                        channel.read_characteristics().raw_value(),
                        channel.read_split().raw_value(),
                        channel.read_interrupt().raw_value()
                    );
                }
            }
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

        /// Program host channel `index` for `attempt` and start it. Returns the
        /// number of interrupts counted so far: the next one may be the
        /// attempt's, see [`Self::wait_event`].
        fn start_attempt(&mut self, index: usize, attempt: &Attempt) -> u64 {
            let mut channel = channel(self.base, index);
            let dma_address = self
                .dma_bus
                .wrapping_add(u32::try_from(attempt.offset).unwrap_or(u32::MAX));
            // Quiesce the channel, then arm the interrupt Thread to record its
            // events for this attempt only.
            channel.write_interrupt_mask(ChannelInterrupts::ZERO);
            channel.write_interrupt(ChannelInterrupts::new_with_raw_value(u32::MAX));
            STATUS.store(0, Ordering::Release);
            channel.write_split(attempt.split);
            channel.write_dma_address(dma_address);
            channel.write_transfer_size(transfer_size(
                u32::try_from(attempt.bytes).unwrap_or(u32::MAX),
                attempt.packets,
                attempt.pid,
            ));
            // Interrupts the interrupt Thread has counted so far; the next one
            // may belong to this attempt.
            let seen = INTERRUPTS.load(Ordering::Acquire);
            ARMED.store(true, Ordering::Release);
            channel.write_interrupt_mask(if attempt.split.enable() {
                split_event_mask()
            } else {
                channel_event_mask()
            });
            // A periodic transfer runs in the (micro)frame parity `HCCHAR.OddFrm`
            // names (periodic transfers only); the next one is asked for.
            let next_is_odd = self.registers.read_frame_number() & 1 == 0;
            channel.write_characteristics(
                characteristics(attempt.target, true)
                    .with_odd_frame(attempt.target.kind == UsbKind::Interrupt && next_is_odd),
            );
            seen
        }

        /// Sleep until the interrupt Thread has recorded events `interpret`
        /// reads as an outcome, or the transfer's deadline passes (the channel is
        /// halted then). Returns the outcome with the events recorded.
        fn wait_event<Outcome>(
            &mut self,
            index: usize,
            target: Target,
            mut seen: u64,
            interpret: impl Fn(ChannelInterrupts) -> Option<Outcome>,
        ) -> Result<(Outcome, u32), PipeException> {
            let deadline = ticks() + ticks_per_ms() * TRANSFER_MS;
            loop {
                let interrupts =
                    ChannelInterrupts::new_with_raw_value(STATUS.load(Ordering::Acquire));
                let status = interrupts.raw_value();
                if let Some(outcome) = interpret(interrupts) {
                    return Ok((outcome, status));
                }
                // Sleep until the interrupt Thread counts another interrupt or
                // the deadline passes.
                let now = ticks();
                if now >= deadline {
                    self.halt(index, target);
                    return Err(PipeException::TransferFailed { status });
                }
                match self
                    .completions
                    .await_ge(seen + 1, ticks_to_ns(deadline - now).max(1))
                {
                    Ok(count) => seen = count,
                    Err(CapError::TimedOut { .. }) => {
                        self.halt(index, target);
                        return Err(PipeException::TransferFailed { status });
                    }
                    Err(error) => {
                        panic!("usb-hcd-dwc2: completion wait failed: {:?}", error.code())
                    }
                }
            }
        }

        /// Disarm channel `index` after a transfer that finished, and return
        /// the bytes it left over.
        fn finish_channel(index: usize, base: usize) -> usize {
            ARMED.store(false, Ordering::Release);
            let mut channel = channel(base, index);
            channel.write_interrupt_mask(ChannelInterrupts::ZERO);
            channel.write_interrupt(ChannelInterrupts::new_with_raw_value(u32::MAX));
            usize::try_from(channel.read_transfer_size().bytes().value()).unwrap_or(usize::MAX)
        }

        /// Run one transfer of `bytes` on host channel `index` to `target`, in
        /// packets of `target.max_packet`, by DMA. The bytes sit in the DMA page
        /// at `offset`: an OUT transfer sends what the caller put there, an IN
        /// transfer leaves the reply there. Returns the bytes moved. A target
        /// behind a high-speed hub runs one split transaction per packet.
        fn run_channel(
            &mut self,
            index: usize,
            target: Target,
            pid: Pid,
            offset: u64,
            bytes: usize,
        ) -> Result<usize, PipeException> {
            let length = u64::try_from(bytes).unwrap_or(u64::MAX);
            if target.input {
                // The controller writes the page behind the CPU's back.
                self.maintain(false, offset, offset + length);
            } else {
                // The controller reads the page from memory, not the cache.
                self.maintain(true, offset, offset + length);
            }
            let moved = if target.split.is_some() {
                self.run_split(index, target, pid, offset, bytes)?
            } else {
                self.run_whole(index, target, pid, offset, bytes)?
            };
            if target.input {
                // The reply is in memory now; drop stale lines.
                self.maintain(
                    false,
                    offset,
                    offset + u64::try_from(moved).unwrap_or(u64::MAX),
                );
            }
            Ok(moved)
        }

        /// The packets of one transfer to a device on the root port, which
        /// the controller sends or receives in one run of the channel.
        fn run_whole(
            &mut self,
            index: usize,
            target: Target,
            pid: Pid,
            offset: u64,
            bytes: usize,
        ) -> Result<usize, PipeException> {
            let packets = u16::try_from(control::packet_count(
                u32::try_from(bytes).unwrap_or(u32::MAX),
                u32::from(target.max_packet),
            ))
            .unwrap_or(u16::MAX);
            let attempt = Attempt {
                target,
                split: Split::ZERO,
                pid,
                offset,
                bytes,
                packets,
            };
            let deadline = now_ms() + RETRY_MS;
            loop {
                let seen = self.start_attempt(index, &attempt);
                let (outcome, status) = self.wait_event(index, target, seen, channel_outcome)?;
                match outcome {
                    ChannelOutcome::Complete => {
                        let remaining = Self::finish_channel(index, self.base);
                        return Ok(bytes.saturating_sub(remaining));
                    }
                    ChannelOutcome::Stalled => {
                        self.halt(index, target);
                        return Err(PipeException::Stalled);
                    }
                    ChannelOutcome::Failed | ChannelOutcome::Halted => {
                        self.halt(index, target);
                        return Err(PipeException::TransferFailed { status });
                    }
                    ChannelOutcome::Retry => {
                        self.halt(index, target);
                        if now_ms() >= deadline {
                            return Err(PipeException::TransferFailed { status });
                        }
                        delay_ms(1);
                    }
                }
            }
        }

        /// The packets of one transfer to a device behind a high-speed hub: one
        /// split transaction each, with the PID alternating from `pid`. An IN
        /// transfer ends at a short packet.
        fn run_split(
            &mut self,
            index: usize,
            target: Target,
            pid: Pid,
            offset: u64,
            bytes: usize,
        ) -> Result<usize, PipeException> {
            let mut moved = 0;
            let mut pid = pid;
            loop {
                let chunk = packet_bytes(bytes - moved, target.max_packet);
                let at = offset + u64::try_from(moved).unwrap_or(u64::MAX);
                let done = self
                    .run_split_packet(index, target, pid, at, chunk, Sequence::new())?
                    // Only a poll ends with no data.
                    .ok_or(PipeException::TransferFailed { status: 0 })?;
                moved += done;
                pid = next_pid(pid);
                if moved >= bytes || (target.input && done < usize::from(target.max_packet)) {
                    return Ok(moved);
                }
            }
        }

        /// One packet through the hub's transaction translator: a start split,
        /// then complete splits until the hub has the result (see
        /// `split::Sequence`). Returns the bytes of the packet that moved, or
        /// `None` for a poll (`sequence` made by `Sequence::poll`) the device
        /// answered with a NAK.
        fn run_split_packet(
            &mut self,
            index: usize,
            target: Target,
            pid: Pid,
            offset: u64,
            bytes: usize,
            mut sequence: Sequence,
        ) -> Result<Option<usize>, PipeException> {
            // What the last start split ended with and how often a complete split
            // was answered NYET, for the failure status.
            let mut start_events = 0;
            let mut not_yet = 0;
            let deadline = now_ms() + RETRY_MS;
            loop {
                let phase = sequence.phase();
                let attempt = Attempt {
                    target,
                    split: split_control(target, phase == Phase::Complete),
                    pid,
                    offset,
                    // The complete split of an OUT or SETUP carries no data (USB 2.0
                    // §11.17.2, Figure 11-53).
                    bytes: if phase == Phase::Complete && !target.input {
                        0
                    } else {
                        bytes
                    },
                    packets: 1,
                };
                let seen = self.start_attempt(index, &attempt);
                let (event, status) = self
                    .wait_event(index, target, seen, |events| event_of(phase, events))
                    .map_err(|error| match error {
                        PipeException::TransferFailed { status } => PipeException::TransferFailed {
                            status: failure_status(status, phase)
                                | split_history(start_events, not_yet, sequence.restarts()),
                        },
                        other => other,
                    })?;
                match phase {
                    Phase::Start => start_events = status,
                    Phase::Complete if event == Event::NotYet => not_yet += 1,
                    Phase::Complete => {}
                }
                let step = sequence.next(event);
                if phase == Phase::Complete
                    && step == Step::Issue(Phase::Start)
                    && matches!(target.kind, UsbKind::Interrupt)
                {
                    // A periodic complete split that did not get the result
                    // starts over. The hub keeps a periodic result only for
                    // the microframes right after its start split (USB 2.0
                    // §11.18.4, §11.18.6), and it has already given the
                    // device its handshake: a report the device sent is lost,
                    // and the next one may come with the other DATA PID.
                    let (count, report) = counted(&LOST_SPLITS);
                    if report {
                        note!(
                            "usb-hcd-dwc2: complete split of device {} endpoint {} restarted ({count} so far), a report may be lost: events {status:#010x} after {not_yet} NYETs",
                            target.device_address,
                            target.endpoint
                        );
                    }
                }
                match step {
                    Step::Issue(_) => {
                        // A run that ended on its own left the channel halted.
                        if !ChannelInterrupts::new_with_raw_value(status).halted() {
                            self.halt(index, target);
                        }
                        // NAKs do not count against the restarts, so time bounds them.
                        if event == Event::Nak {
                            if now_ms() >= deadline {
                                return Err(PipeException::TransferFailed {
                                    status: failure_status(status, phase)
                                        | split_history(start_events, not_yet, sequence.restarts()),
                                });
                            }
                            delay_ms(1);
                        }
                    }
                    Step::Done => {
                        let remaining = Self::finish_channel(index, self.base);
                        return Ok(Some(bytes.saturating_sub(remaining)));
                    }
                    Step::NoData => {
                        self.halt(index, target);
                        return Ok(None);
                    }
                    Step::Stalled => {
                        self.halt(index, target);
                        // The status says which phase met the STALL and what else
                        // the channel saw, which a bare `Stalled` would hide.
                        return Err(PipeException::TransferFailed {
                            status: failure_status(status, phase)
                                | split_history(start_events, not_yet, sequence.restarts()),
                        });
                    }
                    Step::Failed => {
                        self.halt(index, target);
                        return Err(PipeException::TransferFailed {
                            status: failure_status(status, phase)
                                | split_history(start_events, not_yet, sequence.restarts()),
                        });
                    }
                }
            }
        }

        /// Poll the interrupt-IN `pipe` once: one IN transaction of up to
        /// `config.max_packet` bytes expecting DATA1 if `data1`. Returns the
        /// bytes of the report, left in the DMA page at `DATA_OFFSET`, or
        /// `None` if the device had nothing to send.
        fn poll(
            &mut self,
            pipe: &Pipe,
            config: Config,
            data1: bool,
        ) -> Result<Option<usize>, PipeException> {
            if !self.registers.read_port().connected() {
                return Err(PipeException::NoDevice);
            }
            let target = Target {
                device_address: pipe.device_address,
                endpoint: pipe.endpoint,
                input: true,
                low_speed: pipe.speed == UsbSpeed::Low,
                kind: UsbKind::Interrupt,
                max_packet: config.max_packet,
                split: pipe.route.map(|route| SplitRoute {
                    hub_address: route.hub_address,
                    hub_port: route.hub_port,
                }),
            };
            let pid = if data1 { Pid::Data1 } else { Pid::Data0 };
            let bytes = usize::from(config.max_packet).min(DATA_CAPACITY);
            let length = u64::try_from(bytes).unwrap_or(u64::MAX);
            // The controller writes the page behind the CPU's back.
            self.maintain(false, DATA_OFFSET, DATA_OFFSET + length);
            let moved = if target.split.is_some() {
                self.run_split_packet(CHANNEL, target, pid, DATA_OFFSET, bytes, Sequence::poll())?
            } else {
                self.poll_whole(target, pid, bytes)?
            };
            if let Some(moved) = moved {
                self.maintain(
                    false,
                    DATA_OFFSET,
                    DATA_OFFSET + u64::try_from(moved).unwrap_or(u64::MAX),
                );
            }
            Ok(moved)
        }

        /// One poll of a device on the root port.
        fn poll_whole(
            &mut self,
            target: Target,
            pid: Pid,
            bytes: usize,
        ) -> Result<Option<usize>, PipeException> {
            let attempt = Attempt {
                target,
                split: Split::ZERO,
                pid,
                offset: DATA_OFFSET,
                bytes,
                packets: 1,
            };
            let seen = self.start_attempt(CHANNEL, &attempt);
            let (outcome, status) = self.wait_event(CHANNEL, target, seen, poll_outcome)?;
            match outcome {
                PollOutcome::Data => {
                    let remaining = Self::finish_channel(CHANNEL, self.base);
                    Ok(Some(bytes.saturating_sub(remaining)))
                }
                PollOutcome::Nothing => {
                    self.halt(CHANNEL, target);
                    Ok(None)
                }
                PollOutcome::Stalled => {
                    self.halt(CHANNEL, target);
                    Err(PipeException::Stalled)
                }
                PollOutcome::Failed => {
                    self.halt(CHANNEL, target);
                    Err(PipeException::TransferFailed { status })
                }
            }
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
        ///
        /// A `transfer_failed` status carries the stage that failed in bits 26
        /// and 27 (0 SETUP, 1 data IN, 2 data OUT, 3 status) and bit 11.
        fn control(
            &mut self,
            pipe: &Pipe,
            max_packet: u16,
            area: &DataArea,
            offset: u64,
            stage: usize,
        ) -> Result<usize, PipeException> {
            let mut failed_stage = 0;
            self.control_stages(pipe, max_packet, area, offset, stage, &mut failed_stage)
                .map_err(|error| match error {
                    PipeException::TransferFailed { status } => PipeException::TransferFailed {
                        status: (status & !(3 << 26)) | (failed_stage << 26) | (1 << 11),
                    },
                    other => other,
                })
        }

        /// The stages of [`control`](Self::control); `failed_stage` is the stage
        /// running when it returns.
        fn control_stages(
            &mut self,
            pipe: &Pipe,
            max_packet: u16,
            area: &DataArea,
            offset: u64,
            stage: usize,
            failed_stage: &mut u32,
        ) -> Result<usize, PipeException> {
            if stage < control::SETUP_BYTES {
                return Err(PipeException::NotPermitted);
            }
            let mut packet = [0_u8; control::SETUP_BYTES];
            if !area.read_bytes(offset, &mut packet) {
                return Err(PipeException::NotPermitted);
            }
            let setup = SetupPacket::from_bytes(packet);
            let length = match control::data_length(stage, setup, DATA_CAPACITY) {
                Ok(length) => length,
                Err(StageError::TooShort | StageError::TooLong) => {
                    return Err(PipeException::NotPermitted);
                }
            };
            if !self.registers.read_port().connected() {
                return Err(PipeException::NoDevice);
            }

            let stages = control::plan(setup);
            let target = |input: bool| Target {
                device_address: pipe.device_address,
                endpoint: pipe.endpoint,
                input,
                low_speed: pipe.speed == UsbSpeed::Low,
                kind: UsbKind::Control,
                max_packet,
                split: pipe.route.map(|route| SplitRoute {
                    hub_address: route.hub_address,
                    hub_port: route.hub_port,
                }),
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
                    *failed_stage = 1;
                    moved =
                        self.run_channel(CHANNEL, target(true), Pid::Data1, DATA_OFFSET, length)?;
                    if !Self::copy_out(area, DATA_OFFSET, data_offset, moved) {
                        return Err(PipeException::NotPermitted);
                    }
                } else {
                    if !Self::copy_in(area, data_offset, DATA_OFFSET, length) {
                        return Err(PipeException::NotPermitted);
                    }
                    *failed_stage = 2;
                    self.run_channel(CHANNEL, target(false), Pid::Data1, DATA_OFFSET, length)?;
                    moved = length;
                }
            }
            *failed_stage = 3;
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

    /// One run of a host channel: what to send or receive, and how.
    struct Attempt {
        target: Target,
        /// The split control; disabled for a device on the root port.
        split: Split,
        pid: Pid,
        /// Offset in the DMA page of the packets' bytes.
        offset: u64,
        bytes: usize,
        packets: u16,
    }

    /// What the calls share, used only while holding a turn.
    struct Hcd {
        controller: Controller<'static>,
        /// The bring-up step that timed out, if one did; every call then raises
        /// `controller_failed`.
        failed: Option<BringUp>,
        pipes: PipeTable,
        /// The interrupt pipes the poller Thread polls.
        poller: Poller,
        /// The keys of `pipe_v1` each client gets for a pipe.
        masters: Masters<{ pipe_v1::FUNCTION_COUNT }>,
    }

    /// The `EventCount` the poller Thread sleeps on between polls, as a wire
    /// key; calls that change the schedule advance it. Set before the
    /// component is ready.
    static WAKE: AtomicU64 = AtomicU64::new(0);

    /// Wake the poller Thread to look at the schedule.
    fn wake_poller() {
        EventCountKey::from_key(RawKey::from_wire(WAKE.load(Ordering::Acquire)))
            .advance(1)
            .unwrap_or_else(|error| {
                panic!("usb-hcd-dwc2: wake advance failed: {:?}", error.code())
            });
    }

    /// Milliseconds on the virtual counter.
    fn now_ms() -> u64 {
        ticks() / ticks_per_ms().max(1)
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

    /// The client whose key carries `badge`: its position among the clients, its data area as the
    /// component's record names it and as a typed handle.
    fn caller(badge: u64) -> Option<(usize, ExportedArea, DataArea)> {
        let (client, area) = init().area_for_badge(badge)?;
        Some((
            client,
            area,
            DataArea {
                base: area.base,
                len: area.len,
            },
        ))
    }

    /// The pipe handle a key's badge names.
    fn handle(badge: u64) -> Result<u32, PipeException> {
        u32::try_from(badge).map_err(|_overflow| PipeException::InvalidPipe)
    }

    fn pipe_error(error: PipeError) -> PipeException {
        match error {
            PipeError::Invalid => PipeException::InvalidPipe,
            PipeError::Full => PipeException::Busy,
        }
    }

    /// The host controller.
    struct Dwc2;

    fn record_return_key() {
        init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
            init().return_key,
        )));
    }

    impl UsbHcdV1 for Dwc2 {
        fn enter(_badge: u64) {
            record_return_key();
        }

        fn reset_port(_badge: u64, port: u32) -> Result<Speed, Exception> {
            let mut turn = TURNSTILE.enter(init());
            let hcd = hcd(&mut turn);
            if let Some(step) = hcd.failed {
                return Err(Exception::ControllerFailed { step: step as u32 });
            }
            hcd.controller.reset_port(port)
        }

        fn port_status(_badge: u64, port: u32) -> Result<u32, Exception> {
            let mut turn = TURNSTILE.enter(init());
            let hcd = hcd(&mut turn);
            if let Some(step) = hcd.failed {
                return Err(Exception::ControllerFailed { step: step as u32 });
            }
            Ok(hcd.controller.port_status(port))
        }

        fn trace(badge: u64, text: IoRec) -> Result<u32, Exception> {
            let (_client, _area, data) = caller(badge).ok_or(Exception::NotPermitted)?;
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
            speed: Speed,
        ) -> Result<(), Exception> {
            let (client, _area, data) = caller(badge).ok_or(Exception::NotPermitted)?;
            // `endpoint` is an endpoint address: bit 7 is the direction.
            let address = u8::try_from(device_address).ok().filter(|&a| a <= 127);
            let address_byte = u8::try_from(endpoint).ok().filter(|&e| e & 0x70 == 0);
            let (Some(device_address), Some(address_byte)) = (address, address_byte) else {
                return Err(Exception::NotPermitted);
            };
            let input = address_byte & 0x80 != 0;
            let endpoint = address_byte & 0x0f;
            let speed = match speed {
                Speed::Low => UsbSpeed::Low,
                Speed::Full => UsbSpeed::Full,
                Speed::High => UsbSpeed::High,
            };
            let mut turn = TURNSTILE.enter(init());
            let hcd = hcd(&mut turn);
            if let Some(step) = hcd.failed {
                return Err(Exception::ControllerFailed { step: step as u32 });
            }
            let handle = hcd
                .pipes
                .open(Pipe {
                    device_address,
                    endpoint,
                    input,
                    speed,
                    config: None,
                    route: None,
                    owner: client,
                })
                .map_err(|_full| Exception::Busy)?;
            // The pipe's keys carry its handle as their badge. A pipe whose keys
            // could not be installed or reported is closed again; keys already
            // installed fail on the closed handle.
            let installed = hcd
                .masters
                .install(into, first_slot, u64::from(handle))
                .ok()
                // The keys' values, then the pipe's handle.
                .filter(|keys| {
                    data.write_u64s(0, keys)
                        && data.write_u64s(8 * keys.len() as u64, &[u64::from(handle)])
                });
            if installed.is_none() {
                hcd.pipes.close(handle).ok();
                return Err(Exception::NotPermitted);
            }
            Ok(())
        }
    }

    impl PipeV1 for Dwc2 {
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
            let kind = match kind {
                TransferKind::Control => UsbKind::Control,
                TransferKind::Interrupt => UsbKind::Interrupt,
                TransferKind::Bulk => UsbKind::Bulk,
            };
            let config = Config::new(kind, max_packet, interval_ms)
                .map_err(|_unusable| PipeException::NotPermitted)?;
            let mut turn = TURNSTILE.enter(init());
            let hcd = hcd(&mut turn);
            let pipe = *hcd.pipes.get(handle).map_err(pipe_error)?;
            if kind == UsbKind::Interrupt {
                // Only interrupt-IN endpoints are polled.
                if !is_pollable(kind, pipe.input) {
                    return Err(PipeException::NotPermitted);
                }
                hcd.poller
                    .schedule(
                        handle,
                        config.interval_ms,
                        transaction_us(pipe.speed, config.max_packet),
                        now_ms(),
                    )
                    .map_err(|error| match error {
                        ScheduleError::NoBandwidth => PipeException::NoBandwidth,
                        ScheduleError::Invalid => PipeException::NotPermitted,
                        ScheduleError::Full => PipeException::Busy,
                    })?;
            } else {
                hcd.poller.remove(handle);
            }
            hcd.pipes.get_mut(handle).map_err(pipe_error)?.config = Some(config);
            drop(turn);
            wake_poller();
            if kind == UsbKind::Interrupt {
                note!(
                    "usb-hcd-dwc2: pipe {handle}: polling endpoint {} of device {} ({:?} speed{}) every {} ms, {} bytes",
                    pipe.endpoint,
                    pipe.device_address,
                    pipe.speed,
                    if pipe.route.is_some() { ", split" } else { "" },
                    config.interval_ms,
                    config.max_packet
                );
            }
            Ok(())
        }

        fn set_route(badge: u64, hub_address: u32, hub_port: u32) -> Result<(), PipeException> {
            let handle = handle(badge)?;
            let (Some(hub_address), Some(hub_port)) = (
                u8::try_from(hub_address)
                    .ok()
                    .filter(|a| (1..=127).contains(a)),
                u8::try_from(hub_port).ok().filter(|&p| p >= 1),
            ) else {
                return Err(PipeException::NotPermitted);
            };
            let mut turn = TURNSTILE.enter(init());
            let pipe = hcd(&mut turn).pipes.get_mut(handle).map_err(pipe_error)?;
            // Split transactions serve devices slower than the hub.
            if pipe.speed == UsbSpeed::High {
                return Err(PipeException::NotPermitted);
            }
            pipe.route = Some(Route {
                hub_address,
                hub_port,
            });
            Ok(())
        }

        fn close_pipe(badge: u64) -> Result<(), PipeException> {
            let handle = handle(badge)?;
            let mut turn = TURNSTILE.enter(init());
            let hcd = hcd(&mut turn);
            hcd.poller.remove(handle);
            hcd.pipes.close(handle).map_err(pipe_error)
        }

        fn clear_halt(badge: u64) -> Result<(), PipeException> {
            let handle = handle(badge)?;
            let mut turn = TURNSTILE.enter(init());
            // A control pipe starts every transfer with DATA0 SETUP, so a stall
            // leaves no toggle to reset; the halt itself is cleared by the
            // device's CLEAR_FEATURE request through `control`. An interrupt
            // pipe starts again at DATA0, and is polled again if a stall
            // stopped its polls.
            let hcd = hcd(&mut turn);
            let pipe = *hcd.pipes.get(handle).map_err(pipe_error)?;
            if let Some(
                config @ Config {
                    kind: UsbKind::Interrupt,
                    ..
                },
            ) = pipe.config
            {
                if hcd.poller.contains(handle) {
                    hcd.poller.reset_toggle(handle);
                } else {
                    // The schedule had room when the pipe was configured, and
                    // the stalled pipe's share was given back since.
                    hcd.poller
                        .schedule(
                            handle,
                            config.interval_ms,
                            transaction_us(pipe.speed, config.max_packet),
                            now_ms(),
                        )
                        .map_err(|_unscheduled| PipeException::NoBandwidth)?;
                }
                drop(turn);
                wake_poller();
            }
            Ok(())
        }

        fn control(badge: u64, stage: IoRec) -> Result<u32, PipeException> {
            let handle = handle(badge)?;
            let mut turn = TURNSTILE.enter(init());
            let hcd = hcd(&mut turn);
            let pipe = *hcd.pipes.get(handle).map_err(pipe_error)?;
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
            // Only a control pipe transfers control stages.
            let Some(Config {
                kind: UsbKind::Control,
                max_packet,
                ..
            }) = pipe.config
            else {
                return Err(PipeException::NotPermitted);
            };
            let started = now_ms();
            let result =
                hcd.controller
                    .control(&pipe, max_packet, &area, stage.base - area.base, stage_len);
            // The channel is shared with the polls: a long transfer delays them.
            let took = now_ms() - started;
            if took > SLOW_MS {
                let (count, report) = counted(&SLOW_CONTROLS);
                if report {
                    note!(
                        "usb-hcd-dwc2: control to device {} held the channel {took} ms ({count} slow so far): {result:?}",
                        pipe.device_address
                    );
                }
            }
            result.map(|moved| u32::try_from(moved).unwrap_or(u32::MAX))
        }
    }

    usb_hcd::usb_hcd_v1_export!(Dwc2);
    usb_hcd::pipe_v1_export!(Dwc2);

    /// Bytes of a report the poller reads from the DMA page at a time.
    const REPORT_BYTES: usize = 1024;

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

    /// Give the client at position `owner` the `bytes` of report in the DMA
    /// page, in the first empty buffer it posted, tagged with the pipe
    /// `handle`. A report longer than the buffer is cut.
    fn deliver(owner: usize, handle: u32, bytes: usize) {
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
            let mut report = [0_u8; REPORT_BYTES];
            let count = bytes.min(REPORT_BYTES);
            Controller::dma_read(DATA_OFFSET, &mut report[..count]);
            let mut cursor = Cursor::START;
            for &byte in &report[..count] {
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

    /// Milliseconds on the shared host channel past which a call or poll is
    /// traced.
    const SLOW_MS: u64 = 20;

    /// Polls that failed, for the trace's back-off.
    static POLL_FAILURES: AtomicU32 = AtomicU32::new(0);
    /// Polls and control transfers that held the channel past `SLOW_MS`, and
    /// channels that did not halt, for the trace's back-off.
    static SLOW_POLLS: AtomicU32 = AtomicU32::new(0);
    static SLOW_CONTROLS: AtomicU32 = AtomicU32::new(0);
    static HALT_FAILURES: AtomicU32 = AtomicU32::new(0);
    /// Periodic complete splits started over, each a report the device may
    /// have sent and the hub dropped, for the trace's back-off.
    static LOST_SPLITS: AtomicU32 = AtomicU32::new(0);

    /// Poll the first pipe that is due and has a buffer to fill, if any; the
    /// milliseconds until the next pipe is due, if any pipe is polled.
    fn poll_due(hcd: &mut Hcd, now: u64) -> Option<u64> {
        while let Some((handle, data1)) = hcd.poller.due(now) {
            let Ok(&pipe) = hcd.pipes.get(handle) else {
                hcd.poller.remove(handle);
                continue;
            };
            let Some(config) = pipe.config else {
                hcd.poller.remove(handle);
                continue;
            };
            // Without a buffer there is nowhere to put a report: do not poll.
            if !buffer_waiting(pipe.owner) {
                hcd.poller.polled(handle, now, Polled::Nothing);
                continue;
            }
            let started = now_ms();
            let polled = hcd.controller.poll(&pipe, config, data1);
            let took = now_ms() - started;
            if took > SLOW_MS {
                let (count, report) = counted(&SLOW_POLLS);
                if report {
                    note!(
                        "usb-hcd-dwc2: poll of pipe {handle} held the channel {took} ms ({count} slow so far): {polled:?}"
                    );
                }
            }
            match polled {
                Ok(Some(bytes)) => {
                    deliver(pipe.owner, handle, bytes);
                    hcd.poller.polled(handle, now, Polled::Data);
                }
                // A stall stops the polls until `clear_halt`.
                Err(PipeException::Stalled) => {
                    note!("usb-hcd-dwc2: pipe {handle} stalled, polls stop");
                    hcd.poller.remove(handle);
                }
                Ok(None) => hcd.poller.polled(handle, now, Polled::Nothing),
                Err(error) => {
                    // The 1st, 2nd, 4th, 8th... failure, so a dead endpoint cannot
                    // flood the trace.
                    let (failures, report) = counted(&POLL_FAILURES);
                    if report {
                        let status = match error {
                            PipeException::TransferFailed { status } => status,
                            _ => 0,
                        };
                        note!(
                            "usb-hcd-dwc2: poll of pipe {handle} failed ({failures} so far): {error:?} status {status:#010x}"
                        );
                    }
                    hcd.poller.polled(handle, now, Polled::Nothing);
                }
            }
            break;
        }
        hcd.poller.wait_ms(now_ms())
    }

    /// The poller Thread: poll the interrupt pipes when they are due, and
    /// sleep until the next is due or the schedule changes.
    fn poller_main(init_va: u64) -> ! {
        // SAFETY: Kickstart passes the init page's address in x0.
        let init = unsafe { driver_init(init_va) };
        // Wait for the component to be ready, which also sets `WAKE`.
        drop(TURNSTILE.enter(init));
        let wake = EventCountKey::from_key(RawKey::from_wire(WAKE.load(Ordering::Acquire)));
        let mut seen = 0;
        loop {
            let wait = {
                let mut turn = TURNSTILE.enter(init);
                poll_due(hcd(&mut turn), now_ms())
            };
            let timeout = wait.map_or(EventCountKey::WAIT_INFINITE, |milliseconds| {
                milliseconds.saturating_mul(1_000_000).max(1)
            });
            match wake.await_ge(seen + 1, timeout) {
                Ok(count) => seen = count,
                Err(CapError::TimedOut { .. }) => {}
                Err(error) => panic!("usb-hcd-dwc2: poller wait failed: {:?}", error.code()),
            }
        }
    }

    /// Ask the firmware to power the controller's domain. The controller, and
    /// the hub behind it on the Raspberry Pi, are dead until it does.
    ///
    /// The answer is only traced: QEMU does not implement the request and echoes
    /// it back, which the mailbox reads as a missing device. A domain that stays
    /// off shows as a core id other than `0x4f54…` and a bring-up step that
    /// times out in `init_core`.
    fn power_domain(init: &DriverInit) {
        let mailbox = Client::new(Bindings::from_imports(init.imports()));
        match mailbox.set_power_state(Device::UsbHcd, true) {
            Ok(on) => note!("usb-hcd-dwc2: power domain requested on, firmware says on: {on}"),
            Err(error) => note!("usb-hcd-dwc2: power domain request failed: {error:?}"),
        }
    }

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
        power_domain(init);
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
        let core_id = controller.registers.read_core_id();
        let failed = controller.init_core().err();
        match failed {
            None => {
                note!("usb-hcd-dwc2: core {core_id:#010x} reset, host mode, DMA on");
                controller.power_port();
            }
            Some(step) => note!(
                "usb-hcd-dwc2: core {core_id:#010x} bring-up timed out at {step:?} (step {}): every call raises controller_failed",
                step as u32
            ),
        }

        // The masters of the pipe keys sit in the slots after the DMA frame,
        // the turnstile and the completions.
        let masters = Masters::new(
            AddressSpaceKey::from_key(RawKey::from_wire(init.address_space)),
            KeyTableKey::from_key(RawKey::from_wire(init.table)),
            StackPoolHandle::from_wire(init.export_pool).expect("a valid stack pool handle"),
            u32::try_from(next_slot + 3).expect("a slot fits in u32"),
            pipe_v1_entries(),
        );

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
        // The poller Thread sleeps on this `EventCount`, in the slot after the
        // masters'.
        let wake = UntypedKey::from_key(RawKey::from_wire(init.memory))
            .retype(
                ObjectType::EVENT_COUNT,
                0,
                0,
                1,
                &KeyTableKey::from_key(RawKey::from_wire(init.table)),
                u32::try_from(next_slot + 3 + pipe_v1::FUNCTION_COUNT as u64)
                    .expect("a slot fits in u32"),
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("usb-hcd-dwc2: wake Retype failed: {:?}", error.code()));
        WAKE.store(wake.to_wire(), Ordering::Release);
        // SAFETY: nothing else touches the cell until the component says it is
        // ready, just below.
        unsafe {
            *HCD.0.get() = Some(Hcd {
                controller,
                failed,
                pipes: PipeTable::new(),
                poller: Poller::new(),
                masters,
            });
        }
        EventCountKey::from_key(RawKey::from_wire(init.event_count))
            .advance(1)
            .unwrap_or_else(|error| {
                panic!("usb-hcd-dwc2: ready advance failed: {:?}", error.code())
            });
        note!(
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

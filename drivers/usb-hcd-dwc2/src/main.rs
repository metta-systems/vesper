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
//! the next (micro)frame (`HCCHAR.OddFrm` from `HFNUM`). A poll through a
//! hub's transaction translator starts its start split early in a frame, and
//! the interrupt Thread issues its complete splits itself as each run halts,
//! in the microframes USB 2.0 §11.18.4 allots them (`SplitEngine`). Every
//! non-periodic run and every split phase interrupts only at the channel's
//! halt, as the databook's DMA mode has it (`regs::halt_event_mask`). Both Threads run at
//! input priority (`libdevice_driver::Priority::INPUT`), ahead of the CPU-bound
//! programs; they block between interrupts and polls. Nothing here waits
//! milliseconds by spinning: a pause parks the Thread on a timed wait
//! (`delay_ms`), and a register wait spins for at most 250 µs before it
//! parks between checks (`wait_until`). The poller makes one pass over the
//! due pipes per wake and then sleeps at least a millisecond.
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
            microframe::{
                CompleteSplitSlot, FRAME_NUMBER_MASK, between, complete_split_slot, is_odd,
                next as next_microframe, start_split_ran, start_split_target,
            },
            periodic::{MAX_POLLED, Polled, Poller, ScheduleError, is_pollable, transaction_us},
            pipes::{Config, MAX_PIPES, Pipe, PipeError, PipeTable, Route},
            regs::{
                Channel, ChannelInterrupts, ChannelOutcome, ChannelProgram, Characteristics,
                EventCounts, GLOBAL_HOST_CHANNEL, MmioChannel, MmioRegisterBlock, Pid, PollOutcome,
                RegisterBlock, RxFifoSize, Split, SplitRoute, Target, TransferSize, TxFifoSize,
                channel_event_mask, channel_offset, channel_outcome, channel_program,
                characteristics, halt_event_mask, host_channel_bit, poll_outcome, port_speed,
                port_status_bits, port_with_reset_and_power,
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
    // the interrupt Thread issues it at the halt of the run before. Both block
    // between interrupts and polls and do little work each time; the root
    // dispatcher's starvation guard runs either as a normal Thread if it ever
    // uses a whole slice without blocking.
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

    /// Park the calling Thread for `milliseconds`: a timed wait on the
    /// component's own `EventCount` (`DriverInit::event_count`) for a count it
    /// never reaches; the component advances it once, to say it is ready. Any
    /// Thread that reaches this can sleep on it: the interrupt Thread, the
    /// poller, and a client's Thread inside an exported call, which runs in
    /// this component's `AddressSpace` and table. The nucleus arms its one-shot
    /// timer for the wait's deadline, so a 1 ms sleep ends on time.
    fn delay_ms(milliseconds: u32) {
        EventCountKey::from_key(RawKey::from_wire(init().event_count))
            .sleep(u64::from(milliseconds.max(1)) * 1_000_000)
            .unwrap_or_else(|error| panic!("usb-hcd-dwc2: sleep failed: {:?}", error.code()));
    }

    /// How long [`wait_until`] spins before it parks: two high-speed
    /// microframes of 125 µs (USB 2.0 §8.4.3.1). The controller settles a
    /// register bit in that time when nothing is wrong: a channel disabled
    /// with `HCCHARn.ChDis` and `ChEna` raises `HCINTn.ChHltd` once its
    /// current transaction ends (the `DWC_otg` `HCCHARn` and `HCINTn` register
    /// descriptions), and the root port's bus is high speed, where a
    /// transaction fits in a microframe. The bound is our own choice: a wait
    /// still not done after it parks instead of spinning on.
    const SPIN_US: u64 = 250;

    /// Whether `done` held within `milliseconds`: checked in a tight spin for
    /// [`SPIN_US`], then once a millisecond with the Thread parked in between,
    /// so a wait that takes milliseconds (a device connecting, a port enabling
    /// after reset, a channel that does not halt) leaves the CPU to other
    /// Threads.
    fn wait_until(milliseconds: u64, mut done: impl FnMut() -> bool) -> bool {
        let start = ticks();
        let end = start + ticks_per_ms() * milliseconds;
        let spin_end = start + (counter_frequency() * SPIN_US).div_ceil(1_000_000);
        loop {
            if done() {
                return true;
            }
            let now = ticks();
            if now >= end {
                return false;
            }
            if now < spin_end {
                core::hint::spin_loop();
            } else {
                delay_ms(1);
            }
        }
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
        /// What the channel runs of the current control transfer or poll
        /// ended with, for the trace of a failure.
        events: EventCounts,
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

        /// Halt host channel `index` and wait until it has. A channel that is
        /// not enabled any more (it halted by itself, which is how every run
        /// ends in DMA mode) is only quiesced: setting `ChEna` with `ChDis` on
        /// it would start it again (the `DWC_otg` databook's `HCCHARn`: the
        /// pair halts a channel that is running).
        fn halt(&mut self, index: usize, target: Target) {
            // The interrupt Thread stops placing complete splits first, so it
            // cannot start the channel again behind the halt.
            ENGINE.cancel();
            let mut channel = channel(self.base, index);
            // Nothing the halt raises is of interest: the interrupt Thread stops
            // recording, and the channel stops raising.
            ARMED.store(false, Ordering::Release);
            channel.write_interrupt_mask(ChannelInterrupts::ZERO);
            if !channel.read_characteristics().enable() {
                channel.write_interrupt(ChannelInterrupts::new_with_raw_value(u32::MAX));
                return;
            }
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

        /// The (micro)frame number `HFNUM` reads now.
        fn frame_number(&self) -> u32 {
            self.registers.read_frame_number() & FRAME_NUMBER_MASK
        }

        /// The bus address of the DMA page's byte at `offset`.
        fn dma_address(&self, offset: u64) -> u32 {
            self.dma_bus
                .wrapping_add(u32::try_from(offset).unwrap_or(u32::MAX))
        }

        /// Program host channel `index` with `program`, its data at
        /// `dma_address`, raising the events in `mask`, and start it. Every
        /// register of the run is written, so nothing of the channel's last
        /// run carries over (see `ChannelProgram`). Returns the number of
        /// interrupts counted so far: the next one may be the run's, see
        /// [`Self::wait_event`].
        fn start_program(
            &mut self,
            index: usize,
            program: ChannelProgram,
            dma_address: u32,
            mask: ChannelInterrupts,
        ) -> u64 {
            let mut channel = channel(self.base, index);
            // Quiesce the channel, then arm the interrupt Thread to record its
            // events for this run only.
            channel.write_interrupt_mask(ChannelInterrupts::ZERO);
            channel.write_interrupt(ChannelInterrupts::new_with_raw_value(u32::MAX));
            STATUS.store(0, Ordering::Release);
            channel.write_split(program.split);
            channel.write_dma_address(dma_address);
            channel.write_transfer_size(program.transfer_size);
            // Interrupts the interrupt Thread has counted so far; the next one
            // may belong to this run.
            let seen = INTERRUPTS.load(Ordering::Acquire);
            ARMED.store(true, Ordering::Release);
            channel.write_interrupt_mask(mask);
            channel.write_characteristics(program.characteristics);
            seen
        }

        /// The microframe a periodic start split goes in: the next one, once
        /// it is early enough in its frame (`microframe::start_split_target`).
        /// Spins at most four microframes, the most it can take; a frame
        /// counter that does not move gets the next microframe anyway.
        fn start_split_microframe(&self) -> u32 {
            let limit = ticks() + (counter_frequency() * 4 * MICROFRAME_US).div_ceil(1_000_000);
            loop {
                let now = self.frame_number();
                if let Some(target) = start_split_target(now) {
                    return target;
                }
                if ticks() >= limit {
                    return next_microframe(now);
                }
                core::hint::spin_loop();
            }
        }

        /// Sleep until the interrupt Thread has recorded events `interpret`
        /// reads as an outcome, or the transfer's deadline passes (the channel is
        /// halted then). Returns the outcome with the events recorded, which
        /// are counted in `self.events` either way.
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
                    self.events.add(interrupts);
                    return Ok((outcome, status));
                }
                // Sleep until the interrupt Thread counts another interrupt or
                // the deadline passes.
                let now = ticks();
                if now >= deadline {
                    self.halt(index, target);
                    self.events.add(interrupts);
                    return Err(PipeException::TransferFailed { status });
                }
                match self
                    .completions
                    .await_ge(seen + 1, ticks_to_ns(deadline - now).max(1))
                {
                    Ok(count) => seen = count,
                    Err(CapError::TimedOut { .. }) => {
                        self.halt(index, target);
                        self.events.add(interrupts);
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
            let program = channel_program(
                target,
                false,
                pid,
                u32::try_from(bytes).unwrap_or(u32::MAX),
                packets,
                false,
            );
            let dma_address = self.dma_address(offset);
            let deadline = now_ms() + RETRY_MS;
            loop {
                // In DMA mode the controller retries NAKs itself and halts the
                // channel when the transfer ends: only the halt is waited for.
                let seen = self.start_program(index, program, dma_address, halt_event_mask());
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
                        // Parked, so a device that NAKs for the whole retry
                        // budget does not keep the CPU from other Threads.
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
                let done = self.run_split_packet(index, target, pid, at, chunk)?;
                moved += done;
                pid = next_pid(pid);
                if moved >= bytes || (target.input && done < usize::from(target.max_packet)) {
                    return Ok(moved);
                }
            }
        }

        /// One packet of a non-periodic (control) transfer through the hub's
        /// transaction translator: a start split, then complete splits until
        /// the hub has the result (see `split::Sequence`). Returns the bytes of
        /// the packet that moved.
        ///
        /// Non-periodic splits have no microframe window: the translator keeps
        /// the result until a complete split collects it (USB 2.0 §11.17.1), so
        /// each phase is issued from this Thread when the last one halted, a
        /// NYET is followed by another complete split, and only time bounds
        /// the NAKs and NYETs.
        fn run_split_packet(
            &mut self,
            index: usize,
            target: Target,
            pid: Pid,
            offset: u64,
            bytes: usize,
        ) -> Result<usize, PipeException> {
            let mut sequence = Sequence::new();
            // What the last start split ended with and how often a complete split
            // was answered NYET, for the failure status.
            let mut start_events = 0;
            let mut not_yet = 0;
            let deadline = now_ms() + RETRY_MS;
            let dma_address = self.dma_address(offset);
            loop {
                let phase = sequence.phase();
                // The complete split of an OUT or SETUP carries no data (USB 2.0
                // §11.17.2, Figure 11-53).
                let run_bytes = if phase == Phase::Complete && !target.input {
                    0
                } else {
                    bytes
                };
                let program = channel_program(
                    target,
                    phase == Phase::Complete,
                    pid,
                    u32::try_from(run_bytes).unwrap_or(u32::MAX),
                    1,
                    false,
                );
                let seen = self.start_program(index, program, dma_address, halt_event_mask());
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
                let failed = PipeException::TransferFailed {
                    status: failure_status(status, phase)
                        | split_history(start_events, not_yet, sequence.restarts()),
                };
                match step {
                    Step::Issue(_) => {
                        // NAKs and NYETs do not count against the restarts, so
                        // time bounds them. After a NAK the pause parks the
                        // Thread before the next start split.
                        if matches!(event, Event::Nak | Event::NotYet) && now_ms() >= deadline {
                            return Err(failed);
                        }
                        if event == Event::Nak {
                            delay_ms(1);
                        }
                    }
                    Step::Done => {
                        let remaining = Self::finish_channel(index, self.base);
                        return Ok(bytes.saturating_sub(remaining));
                    }
                    // A NAK of a transfer that is not a poll starts it over
                    // (above); a poll does not come here.
                    Step::NoData => return Err(failed),
                    // The status says which phase met the STALL and what else
                    // the channel saw, which a bare `Stalled` would hide.
                    Step::Stalled | Step::Failed => {
                        self.halt(index, target);
                        return Err(failed);
                    }
                }
            }
        }

        /// One poll of an interrupt-IN endpoint behind a high-speed hub: a
        /// start split in an early microframe of a frame, then complete splits
        /// that the interrupt Thread issues itself as each run halts
        /// ([`SplitEngine`]), aimed by `HFNUM` at the microframes USB 2.0
        /// §11.18.4 allots them (2 to 4 after the start split, see
        /// `microframe`). This Thread wakes only when the transaction has an
        /// answer: data, a NAK (no report), a STALL, an error, or the window
        /// passed. Returns the bytes of the report, or `None` without one.
        fn run_periodic_split(
            &mut self,
            target: Target,
            pid: Pid,
            bytes: usize,
        ) -> Result<Option<usize>, PipeException> {
            let mut sequence = Sequence::poll();
            let dma_address = self.dma_address(DATA_OFFSET);
            let length = u32::try_from(bytes).unwrap_or(u32::MAX);
            let complete = channel_program(target, true, pid, length, 1, false);
            loop {
                let aimed = self.start_split_microframe();
                let start = channel_program(target, false, pid, length, 1, is_odd(aimed));
                ENGINE.arm(complete, dma_address, aimed);
                let seen = self.start_program(CHANNEL, start, dma_address, halt_event_mask());
                let waited =
                    self.wait_event(CHANNEL, target, seen, |_| ENGINE.done().then_some(()));
                let report = ENGINE.report();
                self.events.add_nyets(report.not_yet);
                let phase = report.phase;
                let status = match waited {
                    Ok(((), status)) => status,
                    Err(PipeException::TransferFailed { status }) => {
                        return Err(PipeException::TransferFailed {
                            status: failure_status(status, phase)
                                | split_history(0, report.not_yet, sequence.restarts()),
                        });
                    }
                    Err(other) => return Err(other),
                };
                let event = if report.missed {
                    Event::Missed
                } else {
                    event_of(phase, ChannelInterrupts::new_with_raw_value(status))
                        .unwrap_or(Event::Error)
                };
                // The interrupt Thread ran the start split and its acceptance
                // itself: bring the sequence to the complete-split phase it
                // reached, or a result would read as a start split's error.
                if phase == Phase::Complete && sequence.phase() == Phase::Start {
                    let _accepted = sequence.next(Event::Accepted);
                }
                let step = sequence.next(event);
                if phase == Phase::Complete && step == Step::Issue(Phase::Start) {
                    // A periodic complete split that did not get the result
                    // starts over. The hub keeps a periodic result only for
                    // the microframes right after its start split (USB 2.0
                    // §11.18.4, §11.18.6), and it has already given the
                    // device its handshake: a report the device sent is lost,
                    // and the next one may come with the other DATA PID.
                    let (count, traced) = counted(&LOST_SPLITS);
                    if traced {
                        note!(
                            "usb-hcd-dwc2: complete split of device {} endpoint {} restarted ({count} so far), a report may be lost: {event:?}, events {status:#010x} after {} NYETs; start split in microframe {} (of 8), last complete split {} microframes after it{}; this poll {}",
                            target.device_address,
                            target.endpoint,
                            report.not_yet,
                            report.start % 8,
                            between(report.start, report.last_complete),
                            if report.early_waits > 0 {
                                " (waited for a too-early microframe)"
                            } else {
                                ""
                            },
                            self.events
                        );
                    }
                }
                match step {
                    // The channel halted at the end of the last run.
                    Step::Issue(_) => {}
                    Step::Done => {
                        let remaining = Self::finish_channel(CHANNEL, self.base);
                        return Ok(Some(bytes.saturating_sub(remaining)));
                    }
                    Step::NoData => {
                        Self::finish_channel(CHANNEL, self.base);
                        return Ok(None);
                    }
                    Step::Stalled | Step::Failed => {
                        self.halt(CHANNEL, target);
                        return Err(PipeException::TransferFailed {
                            status: failure_status(status, phase)
                                | split_history(0, report.not_yet, sequence.restarts()),
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
                self.run_periodic_split(target, pid, bytes)?
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
            // The transaction goes in the next (micro)frame: `HCCHARn.OddFrm`
            // names its parity.
            let program = channel_program(
                target,
                false,
                pid,
                u32::try_from(bytes).unwrap_or(u32::MAX),
                1,
                is_odd(next_microframe(self.frame_number())),
            );
            let dma_address = self.dma_address(DATA_OFFSET);
            let seen = self.start_program(CHANNEL, program, dma_address, channel_event_mask());
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
            self.events = EventCounts::ZERO;
            let started = now_ms();
            let result = self
                .control_stages(pipe, max_packet, area, offset, stage, &mut failed_stage)
                .map_err(|error| match error {
                    PipeException::TransferFailed { status } => PipeException::TransferFailed {
                        status: (status & !(3 << 26)) | (failed_stage << 26) | (1 << 11),
                    },
                    other => other,
                });
            if let Err(error) = result
                && !matches!(error, PipeException::NotPermitted | PipeException::NoDevice)
            {
                // The 1st, 2nd, 4th, 8th... failed control transfer: which
                // stage, how long, and what the channel's runs ended with.
                let (count, traced) = counted(&CONTROL_FAILURES);
                if traced {
                    let mut setup = [0_u8; control::SETUP_BYTES];
                    let _read = area.read_bytes(offset, &mut setup);
                    note!(
                        "usb-hcd-dwc2: control {setup:02x?} to device {} ({:?} speed{}) failed ({count} so far) in the {} stage after {} ms: {error:?}; {}",
                        pipe.device_address,
                        pipe.speed,
                        Hops(pipe.route),
                        STAGE_NAMES[usize::try_from(failed_stage).unwrap_or(0) & 3],
                        now_ms() - started,
                        self.events
                    );
                }
            }
            result
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

    /// The stages of a control transfer, by the number a failure status
    /// carries in bits 26 and 27.
    const STAGE_NAMES: [&str; 4] = ["setup", "data in", "data out", "status"];

    /// A pipe's route, for the trace: `, split via hub 1 port 2` or nothing.
    struct Hops(Option<Route>);

    impl core::fmt::Display for Hops {
        fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            match self.0 {
                Some(route) => write!(
                    formatter,
                    ", split via hub {} port {}",
                    route.hub_address, route.hub_port
                ),
                None => Ok(()),
            }
        }
    }

    /// Phases of the periodic split transaction the interrupt Thread carries
    /// ([`SplitEngine::state`]).
    const ENGINE_IDLE: u32 = 0;
    const ENGINE_START: u32 = 1;
    const ENGINE_COMPLETE: u32 = 2;
    const ENGINE_DONE: u32 = 3;

    /// Microseconds in a high-speed microframe (USB 2.0 §8.4.3.1).
    const MICROFRAME_US: u64 = 125;

    /// What the interrupt Thread reports about a periodic split transaction
    /// once it has an answer.
    struct EngineReport {
        /// The phase whose run gave the answer.
        phase: Phase,
        /// Whether the complete-split window passed without the result.
        missed: bool,
        /// Complete splits answered NYET.
        not_yet: u32,
        /// The microframe the start split ran in, and the one the last complete
        /// split was aimed at.
        start: u32,
        last_complete: u32,
        /// Whether a complete split waited for a microframe that was too early.
        early_waits: u32,
    }

    /// A periodic split transaction carried by the interrupt Thread from the
    /// halt of its start split through its complete splits, so a complete
    /// split goes out from the first context that sees the previous run halt,
    /// with no Thread hop between (USB 2.0 §11.18.4 gives it microframes, not
    /// milliseconds). The poller arms it with the complete split's registers
    /// and starts the start split; the interrupt Thread, at each halt, either
    /// programs the next complete split (after the start split's ACK, after a
    /// NYET while the window lasts) aimed by `HFNUM` and `HCCHARn.OddFrm`, or
    /// marks the transaction done and wakes the poller. A halt of the channel
    /// cancels it first ([`SplitEngine::cancel`]).
    struct SplitEngine {
        /// `ENGINE_IDLE`, `ENGINE_START`, `ENGINE_COMPLETE` or `ENGINE_DONE`.
        state: AtomicU32,
        /// Set while the interrupt Thread programs the channel.
        busy: AtomicBool,
        /// The complete split's `HCCHARn` (with `ChEna`, without `OddFrm`),
        /// `HCSPLTn`, `HCTSIZn` and `HCDMAn`.
        characteristics: AtomicU32,
        split: AtomicU32,
        transfer_size: AtomicU32,
        dma_address: AtomicU32,
        /// The microframe the start split was aimed at, then the one it ran in.
        start: AtomicU32,
        /// The microframe the last complete split was aimed at.
        last_complete: AtomicU32,
        not_yet: AtomicU32,
        /// The state whose run gave the answer.
        ended_in: AtomicU32,
        missed: AtomicBool,
        early_waits: AtomicU32,
    }

    impl SplitEngine {
        const fn new() -> Self {
            Self {
                state: AtomicU32::new(ENGINE_IDLE),
                busy: AtomicBool::new(false),
                characteristics: AtomicU32::new(0),
                split: AtomicU32::new(0),
                transfer_size: AtomicU32::new(0),
                dma_address: AtomicU32::new(0),
                start: AtomicU32::new(0),
                last_complete: AtomicU32::new(0),
                not_yet: AtomicU32::new(0),
                ended_in: AtomicU32::new(ENGINE_START),
                missed: AtomicBool::new(false),
                early_waits: AtomicU32::new(0),
            }
        }

        /// Arm for a start split aimed at microframe `aimed`, whose complete
        /// splits run `complete` with their data at `dma_address`. Called
        /// before the start split is started.
        fn arm(&self, complete: ChannelProgram, dma_address: u32, aimed: u32) {
            self.characteristics
                .store(complete.characteristics.raw_value(), Ordering::Relaxed);
            self.split
                .store(complete.split.raw_value(), Ordering::Relaxed);
            self.transfer_size
                .store(complete.transfer_size.raw_value(), Ordering::Relaxed);
            self.dma_address.store(dma_address, Ordering::Relaxed);
            self.start.store(aimed, Ordering::Relaxed);
            self.last_complete.store(aimed, Ordering::Relaxed);
            self.not_yet.store(0, Ordering::Relaxed);
            self.ended_in.store(ENGINE_START, Ordering::Relaxed);
            self.missed.store(false, Ordering::Relaxed);
            self.early_waits.store(0, Ordering::Relaxed);
            self.state.store(ENGINE_START, Ordering::Release);
        }

        /// Whether a transaction is in flight.
        fn active(&self) -> bool {
            matches!(
                self.state.load(Ordering::Acquire),
                ENGINE_START | ENGINE_COMPLETE
            )
        }

        /// Whether the transaction has its answer.
        fn done(&self) -> bool {
            self.state.load(Ordering::Acquire) == ENGINE_DONE
        }

        /// Stop carrying the transaction, and wait for a channel program the
        /// interrupt Thread is writing to be written, so the caller's halt
        /// comes after it.
        fn cancel(&self) {
            self.state.store(ENGINE_IDLE, Ordering::SeqCst);
            if self.busy.load(Ordering::SeqCst) {
                let _written = wait_until(1, || !self.busy.load(Ordering::SeqCst));
            }
        }

        /// The answer, for the poller once [`done`](Self::done).
        fn report(&self) -> EngineReport {
            EngineReport {
                phase: if self.ended_in.load(Ordering::Acquire) == ENGINE_COMPLETE {
                    Phase::Complete
                } else {
                    Phase::Start
                },
                missed: self.missed.load(Ordering::Acquire),
                not_yet: self.not_yet.load(Ordering::Acquire),
                start: self.start.load(Ordering::Acquire),
                last_complete: self.last_complete.load(Ordering::Acquire),
                early_waits: self.early_waits.load(Ordering::Acquire),
            }
        }

        /// The interrupt Thread saw the channel halt with `events`: issue the
        /// next complete split if the transaction wants one now. Returns
        /// whether it did; otherwise the caller records the events and calls
        /// [`finish`](Self::finish).
        fn continue_split(
            &self,
            base: usize,
            frame_number: impl Fn() -> u32,
            events: ChannelInterrupts,
        ) -> bool {
            let state = self.state.load(Ordering::Acquire);
            let phase = match state {
                ENGINE_START => Phase::Start,
                ENGINE_COMPLETE => Phase::Complete,
                _ => return false,
            };
            let start = match (phase, event_of(phase, events)) {
                (Phase::Start, Some(Event::Accepted)) => {
                    let ran = start_split_ran(self.start.load(Ordering::Relaxed), frame_number());
                    self.start.store(ran, Ordering::Relaxed);
                    ran
                }
                (Phase::Complete, Some(Event::NotYet)) => {
                    self.not_yet.fetch_add(1, Ordering::Relaxed);
                    self.start.load(Ordering::Relaxed)
                }
                _ => return false,
            };
            // Wait out a microframe that is too early; the start split ran
            // in the current one, so this spins less than a microframe. A
            // counter that does not move in two counts as a missed window.
            let spin_end = ticks() + (counter_frequency() * 2 * MICROFRAME_US).div_ceil(1_000_000);
            let mut waited = false;
            let target = loop {
                match complete_split_slot(start, frame_number()) {
                    CompleteSplitSlot::Issue { target } => break Some(target),
                    CompleteSplitSlot::TooEarly if ticks() < spin_end => {
                        if !waited {
                            waited = true;
                            self.early_waits.fetch_add(1, Ordering::Relaxed);
                        }
                        core::hint::spin_loop();
                    }
                    CompleteSplitSlot::Missed | CompleteSplitSlot::TooEarly => break None,
                }
            };
            let Some(target) = target else {
                self.missed.store(true, Ordering::Release);
                return false;
            };
            self.busy.store(true, Ordering::SeqCst);
            // A halt cancelled the transaction meanwhile: leave the channel be.
            if self
                .state
                .compare_exchange(state, ENGINE_COMPLETE, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                self.busy.store(false, Ordering::SeqCst);
                return false;
            }
            STATUS.store(0, Ordering::Release);
            let mut transfer = channel(base, CHANNEL);
            transfer.write_split(Split::new_with_raw_value(
                self.split.load(Ordering::Relaxed),
            ));
            transfer.write_dma_address(self.dma_address.load(Ordering::Relaxed));
            transfer.write_transfer_size(TransferSize::new_with_raw_value(
                self.transfer_size.load(Ordering::Relaxed),
            ));
            transfer.write_characteristics(
                Characteristics::new_with_raw_value(self.characteristics.load(Ordering::Relaxed))
                    .with_odd_frame(is_odd(target)),
            );
            self.last_complete.store(target, Ordering::Relaxed);
            self.busy.store(false, Ordering::SeqCst);
            true
        }

        /// The transaction has its answer, which the caller has recorded in
        /// `STATUS`: mark it done for the poller.
        fn finish(&self) {
            let state = self.state.load(Ordering::Acquire);
            if matches!(state, ENGINE_START | ENGINE_COMPLETE) {
                // Before the state, so a poller that sees it done reads the
                // phase too. A cancel in between only leaves it unread.
                self.ended_in.store(state, Ordering::Release);
                let _cancelled = self.state.compare_exchange(
                    state,
                    ENGINE_DONE,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                );
            }
        }
    }

    /// The periodic split transaction in flight, if any.
    static ENGINE: SplitEngine = SplitEngine::new();

    /// What the calls share, used only while holding a turn.
    struct Hcd {
        controller: Controller<'static>,
        /// The bring-up step that timed out, if one did; every call then raises
        /// `controller_failed`.
        failed: Option<BringUp>,
        pipes: PipeTable,
        /// The interrupt pipes the poller Thread polls.
        poller: Poller,
        /// What the runs of each pipe's failed polls ended with, by the pipe's
        /// table slot, for the trace: which event dominates on a pipe.
        poll_failures: [EventCounts; MAX_PIPES],
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
            if let Some(counts) = hcd
                .poll_failures
                .get_mut(usize::try_from(handle & 0xffff).unwrap_or(usize::MAX))
            {
                *counts = EventCounts::ZERO;
            }
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
    /// Control transfers that failed, for the trace's back-off.
    static CONTROL_FAILURES: AtomicU32 = AtomicU32::new(0);

    /// One pass over the pipes due at `now`: each is polled at most once (a
    /// polled or skipped pipe is next due after `now`), and a pipe without a
    /// buffer to fill is skipped. Returns how long the poller sleeps after
    /// the pass ([`Poller::park_ms`]): at least a millisecond, so a pass
    /// that ran late never loops straight into the next.
    fn poll_due(hcd: &mut Hcd, now: u64) -> Option<u64> {
        let mut passes = 0;
        while passes < MAX_POLLED
            && let Some((handle, data1)) = hcd.poller.due(now)
        {
            passes += 1;
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
            hcd.controller.events = EventCounts::ZERO;
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
                    hcd.poller.polled(handle, now_ms(), Polled::Data);
                }
                // A stall stops the polls until `clear_halt`.
                Err(PipeException::Stalled) => {
                    note!("usb-hcd-dwc2: pipe {handle} stalled, polls stop");
                    hcd.poller.remove(handle);
                }
                Ok(None) => hcd.poller.polled(handle, now_ms(), Polled::Nothing),
                Err(error) => {
                    let slot = usize::try_from(handle & 0xffff).unwrap_or(0) % MAX_PIPES;
                    let this_poll = hcd.controller.events;
                    hcd.poll_failures[slot].merge(&this_poll);
                    // The 1st, 2nd, 4th, 8th... failure, so a dead endpoint cannot
                    // flood the trace.
                    let (failures, report) = counted(&POLL_FAILURES);
                    if report {
                        let status = match error {
                            PipeException::TransferFailed { status } => status,
                            _ => 0,
                        };
                        note!(
                            "usb-hcd-dwc2: poll of pipe {handle} (device {} endpoint {}{}) failed ({failures} so far): {error:?} status {status:#010x}; this poll {this_poll}; the pipe's failed polls {}",
                            pipe.device_address,
                            pipe.endpoint,
                            Hops(pipe.route),
                            hcd.poll_failures[slot]
                        );
                    }
                    hcd.poller.polled(handle, now_ms(), Polled::Nothing);
                }
            }
        }
        hcd.poller.park_ms(now_ms())
    }

    /// The poller Thread: one pass over the interrupt pipes that are due,
    /// then sleep until the next is due (at least a millisecond) or the
    /// schedule changes.
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
            // Never zero: `park_ms` is at least a millisecond.
            let timeout = wait.map_or(EventCountKey::WAIT_INFINITE, |milliseconds| {
                milliseconds.saturating_mul(1_000_000)
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
            events: EventCounts::ZERO,
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
                poll_failures: [EventCounts::ZERO; MAX_PIPES],
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
                    // A periodic split's next complete split goes out from
                    // here, at the halt of the run before it; the poller
                    // sleeps on until the transaction has its answer.
                    if ENGINE.active()
                        && events.halted()
                        && ENGINE.continue_split(
                            base,
                            || service_registers.read_frame_number() & FRAME_NUMBER_MASK,
                            events,
                        )
                    {
                        acknowledge();
                        continue;
                    }
                    STATUS.fetch_or(events.raw_value(), Ordering::AcqRel);
                    if events.halted() || events.ahb_error() {
                        ENGINE.finish();
                    }
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

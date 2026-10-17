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
//! the core into host mode, powers the root port, and resets it. It then
//! enumerates the device behind the port: each control transfer runs on host
//! channel 0 by DMA, one endpoint 0 stage at a time, and
//! `vesper_usb_protocol` drives the enumeration.
//!
//! Packet data moves by DMA through one page the driver owns: a frame retyped
//! from its own memory. The page is cacheable, so the driver cleans it before
//! the controller reads an OUT packet and invalidates it before the CPU reads
//! an IN reply. QEMU's model supports DMA only.
//!
//! The root device is the LAN9514 hub on the Raspberry Pi 3. Downstream devices
//! and the hub's interrupt endpoint come with the hub driver (P3). Completion is
//! polled, so this component has no interrupt Thread yet.

#[cfg(target_os = "vesper")]
mod component {
    use {
        aarch64_cpu::registers::{CNTFRQ_EL0, CNTVCT_EL0, Readable},
        libdevice_driver::{
            DEVICE_WINDOW_VA, DriverInit, MAX_DEVICE_PAGES, NO_BUS_ADDRESS, device_address,
            driver_compatible, driver_init, map_device_pages,
        },
        libobject::{
            FrameAttribute, FrameKey, KeyTableKey, ObjectType, RawKey, Rights, UntypedKey,
        },
        libuser::semihosting as semi,
        usb_hcd_dwc2::{
            control,
            regs::{
                Channel, ChannelInterrupts, ChannelOutcome, MmioChannel, MmioRegisterBlock, Pid,
                RegisterBlock, RxFifoSize, Target, TxFifoSize, channel_offset, channel_outcome,
                characteristics, port_speed, port_with_reset_and_power, transfer_size,
            },
        },
        vesper_usb_protocol::{
            descriptor::{self, Descriptor, TransferKind},
            enumeration::{Enumerator, Event, Failure, Speed, Step},
            setup::{
                DESCRIPTOR_CONFIGURATION, DESCRIPTOR_DEVICE, REQUEST_SET_ADDRESS, SetupPacket,
            },
        },
    };

    libuser::entry!(main);

    driver_compatible!("brcm,bcm2708-usb");

    /// The slot the test kernel installs its report Notification key at (see
    /// `kernel/tests/usb-test`). Only the test build reports.
    #[cfg(feature = "qemu")]
    const REPORT_SLOT: u32 = 250;

    /// Host channel that runs every control transfer.
    const CHANNEL: usize = 0;
    /// Address the root device is enumerated to.
    const ROOT_ADDRESS: u8 = 1;
    /// Largest reply the component reads: a configuration descriptor.
    const REPLY_CAPACITY: usize = 256;
    /// Time one transfer may take before it is abandoned, in milliseconds.
    const TRANSFER_MS: u64 = 500;
    /// Times a transfer is issued again after a NAK or a transaction error.
    const ATTEMPTS: u32 = 3;
    /// Hub device class code (USB 2.0 §11.23).
    const CLASS_HUB: u8 = 9;
    /// Where the DMA page is mapped: the first device window page past the
    /// register pages.
    const DMA_VA: u64 = DEVICE_WINDOW_VA + MAX_DEVICE_PAGES as u64 * 4096;
    /// Offset in the DMA page of the SETUP packet.
    const SETUP_OFFSET: u64 = 0;
    /// Offset in the DMA page of the data stage and of the status stage.
    const DATA_OFFSET: u64 = 64;

    /// Ticks of the virtual counter, which EL0 may read (the physical counter
    /// is trapped for EL0 in `kickstart`).
    fn ticks() -> u64 {
        CNTVCT_EL0.get()
    }

    fn ticks_per_ms() -> u64 {
        CNTFRQ_EL0.get() / 1000
    }

    /// Spin for `milliseconds`.
    fn delay_ms(milliseconds: u32) {
        let end = ticks() + ticks_per_ms() * u64::from(milliseconds);
        while ticks() < end {
            core::hint::spin_loop();
        }
    }

    /// Wait until `done` holds, or panic after `milliseconds`.
    fn wait_for(milliseconds: u64, what: &str, mut done: impl FnMut() -> bool) {
        let end = ticks() + ticks_per_ms() * milliseconds;
        while !done() {
            assert!(ticks() < end, "usb-hcd-dwc2: timed out waiting for {what}");
            core::hint::spin_loop();
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

            // The driver polls, so no interrupt reaches the CPU.
            self.registers.write_interrupt_mask(0);
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

        /// Reset the root port and report the speed of the device on it.
        fn reset_port(&mut self) -> Speed {
            // USB 2.0 §7.1.7.5: hold reset for at least 50 ms.
            let port = self.registers.read_port();
            self.registers
                .write_port(port_with_reset_and_power(port, true, true));
            delay_ms(60);

            let port = self.registers.read_port();
            self.registers
                .write_port(port_with_reset_and_power(port, false, true));
            wait_for(100, "the root port to enable", || {
                self.registers.read_port().enabled()
            });

            // Acknowledge the enable change the reset caused.
            let port = self.registers.read_port();
            self.registers
                .write_port(port.without_events().with_enable_changed(true));
            port_speed(self.registers.read_port())
                .unwrap_or_else(|| panic!("usb-hcd-dwc2: the root port reports a reserved speed"))
        }

        /// Halt host channel `index` and wait until it has.
        fn halt(&mut self, index: usize, target: Target) {
            let mut channel = channel(self.base, index);
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

        /// Run one transfer on host channel `index` to `target`, in packets of
        /// `target.max_packet`, by DMA. An IN transfer receives into `input`,
        /// an OUT one sends `out`. Returns the bytes moved.
        fn run_channel(
            &mut self,
            index: usize,
            target: Target,
            pid: Pid,
            out: &[u8],
            input: &mut [u8],
        ) -> Result<usize, Failure> {
            let bytes = if target.input { input.len() } else { out.len() };
            let packets = u16::try_from(control::packet_count(
                u32::try_from(bytes).unwrap_or(u32::MAX),
                u32::from(target.max_packet),
            ))
            .unwrap_or(u16::MAX);
            let offset = if pid == Pid::Setup {
                SETUP_OFFSET
            } else {
                DATA_OFFSET
            };
            let va = DMA_VA + offset;
            let length = u64::try_from(bytes).unwrap_or(u64::MAX);
            if target.input {
                // The controller writes the page behind the CPU's back.
                self.maintain(false, offset, offset + length);
            } else {
                // SAFETY: the DMA page is mapped read/write and only this
                // driver uses it; `bytes` fits the page, as every reply and
                // request does.
                let staging = unsafe {
                    core::slice::from_raw_parts_mut(
                        core::ptr::with_exposed_provenance_mut::<u8>(
                            usize::try_from(va).expect("a 64-bit address"),
                        ),
                        bytes,
                    )
                };
                staging.copy_from_slice(out);
                // The controller reads the page from memory, not the cache.
                self.maintain(true, offset, offset + length);
            }
            let dma_address = self
                .dma_bus
                .wrapping_add(u32::try_from(offset).unwrap_or(u32::MAX));

            let mut channel = channel(self.base, index);
            for _attempt in 0..ATTEMPTS {
                channel.write_interrupt(ChannelInterrupts::new_with_raw_value(u32::MAX));
                channel.write_interrupt_mask(ChannelInterrupts::ZERO);
                channel.write_dma_address(dma_address);
                channel.write_transfer_size(transfer_size(
                    u32::try_from(bytes).unwrap_or(u32::MAX),
                    packets,
                    pid,
                ));
                channel.write_characteristics(characteristics(target, true));

                let deadline = ticks() + ticks_per_ms() * TRANSFER_MS;
                loop {
                    let interrupts = channel.read_interrupt();
                    match channel_outcome(interrupts) {
                        Some(ChannelOutcome::Complete) => {
                            channel.write_interrupt(interrupts);
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
                                // SAFETY: as for the staging slice above; the
                                // controller has finished with the page.
                                let received = unsafe {
                                    core::slice::from_raw_parts(
                                        core::ptr::with_exposed_provenance::<u8>(
                                            usize::try_from(va).expect("a 64-bit address"),
                                        ),
                                        moved,
                                    )
                                };
                                input
                                    .get_mut(..moved)
                                    .unwrap_or_else(|| {
                                        panic!(
                                            "usb-hcd-dwc2: the device sent more data than asked for"
                                        )
                                    })
                                    .copy_from_slice(received);
                            }
                            return Ok(moved);
                        }
                        Some(ChannelOutcome::Stalled) => {
                            self.halt(index, target);
                            return Err(Failure::Stalled);
                        }
                        Some(ChannelOutcome::Failed | ChannelOutcome::Halted) => {
                            self.halt(index, target);
                            return Err(Failure::Transfer);
                        }
                        Some(ChannelOutcome::Retry) => {
                            self.halt(index, target);
                            break;
                        }
                        None => {}
                    }
                    if ticks() >= deadline {
                        self.halt(index, target);
                        return Err(Failure::Transfer);
                    }
                }
            }
            Err(Failure::Transfer)
        }

        /// Run one control transfer to device `address`: SETUP, the data stage
        /// if the request has one, then the status stage. Returns the bytes
        /// of the data stage that moved.
        fn control(
            &mut self,
            address: u8,
            max_packet: u16,
            speed: Speed,
            setup: SetupPacket,
            input: &mut [u8],
        ) -> Result<usize, Failure> {
            let stages = control::plan(setup);
            let target = |input: bool| Target {
                device_address: address,
                endpoint: 0,
                input,
                low_speed: speed == Speed::Low,
                kind: TransferKind::Control,
                max_packet,
            };
            self.run_channel(
                CHANNEL,
                target(false),
                Pid::Setup,
                &setup.to_bytes(),
                &mut [],
            )?;

            let mut moved = 0;
            if stages.has_data {
                let length = usize::from(setup.length).min(input.len());
                let data = input.get_mut(..length).unwrap_or(&mut []);
                moved = if stages.data_input {
                    self.run_channel(CHANNEL, target(true), Pid::Data1, &[], data)?
                } else {
                    self.run_channel(CHANNEL, target(false), Pid::Data1, data, &mut [])?;
                    length
                };
            }
            self.run_channel(
                CHANNEL,
                target(stages.status_input),
                Pid::Data1,
                &[],
                &mut [],
            )?;
            Ok(moved)
        }
    }

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
        };

        controller.init_core();
        controller.power_port();

        let mut enumerator = Enumerator::new(ROOT_ADDRESS).expect("a valid address");
        let mut address = 0;
        let mut max_packet = 8;
        let mut speed = Speed::Full;
        let mut device = [0; 18];
        let mut configuration = [0; REPLY_CAPACITY];
        let mut configuration_len = 0;
        let mut reply = [0; REPLY_CAPACITY];
        let mut step = enumerator.next(Event::Start);
        loop {
            step = match step {
                Step::ResetPort => {
                    speed = controller.reset_port();
                    enumerator.next(Event::PortReset(speed))
                }
                Step::Delay { milliseconds } => {
                    delay_ms(milliseconds);
                    enumerator.next(Event::Delayed)
                }
                Step::Control { setup } => {
                    let length = usize::from(setup.length).min(reply.len());
                    let data = reply.get_mut(..length).unwrap_or(&mut []);
                    match controller.control(address, max_packet, speed, setup, data) {
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
                        Err(failure) => enumerator.next(Event::ControlFailed(failure)),
                    }
                }
                Step::Configured { .. } => break,
                Step::Failed(failure) => {
                    panic!("usb-hcd-dwc2: enumeration failed: {failure:?}")
                }
            };
        }

        let Some(Ok(Descriptor::Device(root))) = descriptor::walk(&device).next() else {
            panic!("usb-hcd-dwc2: the root device's descriptor is malformed");
        };
        let hub = root.class_code == CLASS_HUB;
        let interfaces =
            descriptor::walk(configuration.get(..configuration_len).unwrap_or_default())
                .filter_map(|descriptor| match descriptor {
                    Ok(Descriptor::Interface(interface)) => Some(interface.class_code),
                    _ => None,
                })
                .count();
        semi::println!(
            "usb-hcd-dwc2: root port {speed:?}: device at address {address}, class {:#x}, vendor {:#06x} product {:#06x}, {interfaces} interfaces",
            root.class_code,
            root.vendor,
            root.product
        );
        assert!(hub, "usb-hcd-dwc2: the root device is not a hub");
        assert!(interfaces > 0, "usb-hcd-dwc2: the hub has no interface");
        semi::println!("usb-hcd-dwc2: ✅ root hub enumerated");

        #[cfg(feature = "qemu")]
        libobject::NotificationKey::from_key(libdevice_driver::local_key(init.table, REPORT_SLOT))
            .signal(0)
            .unwrap_or_else(|error| panic!("usb-hcd-dwc2: report failed: {:?}", error.code()));
        loop {
            delay_ms(1000);
        }
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}

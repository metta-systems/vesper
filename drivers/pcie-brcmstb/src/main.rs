// Host builds only check that the library builds; the component itself runs
// on Vesper.
#![cfg_attr(target_os = "vesper", no_std, no_main)]

//! EL0 driver component for the Broadcom STB PCIe root complex.
//!
//! Kickstart loads it for a `brcm,bcm2711-pcie` node (the Raspberry Pi 4's
//! bridge, with the VL805 USB controller behind it) and a `brcm,bcm2712-pcie`
//! node (the Raspberry Pi 5's enabled one, with RP1 behind it). As a PCI host
//! bridge driver (`pci_host = true`) it receives its node's windows, the
//! device memory behind the outbound ones, and the registers of its MSI
//! controller and reset controllers (`libdevice_driver::PciHost`).
//!
//! Its one Thread brings the bridge and the link up, enumerates the buses
//! behind it, assigning their BARs from the outbound windows, and logs what
//! it found. Then it tests the MSI path from software: it raises one vector
//! (in the bridge's own MSI controller on the BCM2711, in the MIP on the
//! BCM2712) and waits for its interrupt. It serves nothing yet: how child
//! drivers reach their functions is still open.

#[cfg(target_os = "vesper")]
mod component {
    use {
        libdevice_driver::{
            DriverInit, MSI_CONTROLLER, MSI_OWN, NONE_INDEX, PCI_SPACE_IO,
            PciWindow, device_address, driver_compatible, driver_init, map_device_pages,
        },
        liblog_client::log_println,
        libobject::{CapError, IrqHandlerKey, NotificationKey, RawKey},
        libpci::{BarKind, Device, MAX_BARS, enumerate},
        pcie_brcmstb::{
            Board, Bridge, Error, Mmio, RESCAL_POLLS, Setup, Soc, Window, calibration_done,
            raise_mip, set_reset, set_up_mip, start_calibration,
        },
    };

    libuser::entry!(main);

    driver_compatible!("brcm,bcm2711-pcie", "brcm,bcm2712-pcie");

    /// `DriverInit::compatible_index` of `brcm,bcm2712-pcie`.
    const BCM2712: u64 = 1;

    /// Most functions recorded.
    const MAX_FUNCTIONS: usize = 8;

    /// How long the MSI self-test waits for its interrupt.
    const MSI_WAIT_NS: u64 = 100_000_000;

    /// The registers of one register range, mapped by `map_device_pages`.
    struct Registers {
        base: u64,
    }

    impl Mmio for Registers {
        fn read(&mut self, offset: usize) -> u32 {
            // SAFETY: `map_device_pages` mapped the range read/write as
            // device memory for the driver's lifetime, and only this Thread
            // touches it; the offsets lie inside the range.
            unsafe { ((self.base + offset as u64) as *const u32).read_volatile() }
        }

        fn write(&mut self, offset: usize, value: u32) {
            // SAFETY: as in `read`.
            unsafe { ((self.base + offset as u64) as *mut u32).write_volatile(value) }
        }
    }

    /// The registers of region `index` of the init record.
    fn registers(init: &DriverInit, index: u64) -> Option<Registers> {
        let region = init.regions().get(usize::try_from(index).ok()?)?;
        Some(Registers {
            base: device_address(init, region.start)?,
        })
    }

    fn notification(init: &DriverInit) -> NotificationKey {
        NotificationKey::from_key(RawKey::from_wire(init.notification))
    }

    /// Waits and the BCM2712's resets, through the driver's own resources.
    struct Hooks<'init> {
        init: &'init DriverInit,
    }

    impl Hooks<'_> {
        /// The reset `reset-names` calls `name`, with its controller's
        /// registers.
        fn reset(&self, name: &str) -> Option<(Registers, u32)> {
            let reset = self.init.pci.reset(name)?;
            Some((
                registers(self.init, reset.region)?,
                u32::try_from(reset.id).ok()?,
            ))
        }
    }

    impl Board for Hooks<'_> {
        /// Sleep on the driver's Notification: before any interrupt line is
        /// acknowledged, nothing signals it.
        fn delay_us(&mut self, microseconds: u64) {
            let deadline = libuser::clock::now_ns() + microseconds * 1_000;
            loop {
                let now = libuser::clock::now_ns();
                if now >= deadline {
                    break;
                }
                match notification(self.init).await_bits(deadline - now) {
                    Ok(_) | Err(CapError::TimedOut { .. }) => {}
                    Err(error) => panic!("pcie: delay wait failed: {:?}", error.code()),
                }
            }
        }

        fn bridge_reset(&mut self, asserted: bool) {
            let Some((mut controller, id)) = self.reset("bridge") else {
                log_println!("pcie: no bridge reset");
                return;
            };
            set_reset(&mut controller, id, asserted);
            if !asserted {
                self.delay_us(200);
            }
        }

        fn calibrate(&mut self) -> bool {
            let Some((mut rescal, _)) = self.reset("rescal") else {
                return true;
            };
            if !start_calibration(&mut rescal) {
                return false;
            }
            for _ in 0..RESCAL_POLLS {
                if calibration_done(&mut rescal) {
                    return true;
                }
                self.delay_us(100);
            }
            false
        }
    }

    const fn window(window: &PciWindow) -> Window {
        Window {
            pci: window.pci,
            cpu: window.cpu,
            size: window.size,
        }
    }

    /// The CPU address of PCI address `pci` through the outbound windows.
    fn cpu_address(init: &DriverInit, pci: u64) -> Option<u64> {
        init.pci
            .windows()
            .iter()
            .find(|window| window.pci <= pci && pci - window.pci < window.size)
            .map(|window| window.cpu + (pci - window.pci))
    }

    /// Log one function and its BARs.
    fn report(init: &DriverInit, device: &Device) {
        log_println!(
            "pcie: {} {:04x}:{:04x} class {:#08x}{}{}{}",
            device.function,
            device.vendor,
            device.device,
            device.class,
            match device.buses {
                Some((first, last)) if first == last => " bridge to bus",
                Some(_) => " bridge to buses",
                None => "",
            },
            if device.msi.is_some() { ", MSI" } else { "" },
            if device.msix.is_some() { ", MSI-X" } else { "" },
        );
        for bar in device.bars().iter().take(MAX_BARS) {
            let width = match bar.kind {
                BarKind::Memory32 => "32-bit",
                BarKind::Memory64 => "64-bit",
                BarKind::Io => "I/O",
            };
            match (bar.assigned, cpu_address(init, bar.pci)) {
                (true, Some(cpu)) => log_println!(
                    "pcie: {} BAR{} {:#x}+{:#x} {}{}, CPU {:#x}",
                    device.function,
                    bar.index,
                    bar.pci,
                    bar.size,
                    width,
                    if bar.prefetchable { " prefetchable" } else { "" },
                    cpu
                ),
                _ => log_println!(
                    "pcie: {} BAR{} {:#x} bytes {}: no room",
                    device.function,
                    bar.index,
                    bar.size,
                    width
                ),
            }
        }
    }

    /// Where the self-test raises its MSI.
    enum MsiSource {
        /// Vector 0 of the bridge's own controller
        Own,
        /// The first vector of the MIP
        Mip(Registers, u32),
    }

    /// Raise one MSI vector from software and wait for its interrupt.
    fn msi_self_test(init: &DriverInit, bridge: &mut Bridge<Registers>) {
        let (handler, mut source) = match init.pci.msi.kind {
            // The bridge's `msi` line, the same as its `pcie` line on the
            // BCM2711, is its first.
            MSI_OWN => (init.interrupts().first().copied(), MsiSource::Own),
            MSI_CONTROLLER if init.pci.msi.interrupt != NONE_INDEX => {
                let Some(mut mip) = registers(init, init.pci.msi.region) else {
                    log_println!("pcie: MSI controller registers not mapped");
                    return;
                };
                set_up_mip(&mut mip);
                (
                    usize::try_from(init.pci.msi.interrupt)
                        .ok()
                        .and_then(|index| init.interrupts().get(index).copied()),
                    MsiSource::Mip(mip, u32::try_from(init.pci.msi.first_vector).unwrap_or(0)),
                )
            }
            _ => {
                log_println!("pcie: no MSI controller to test");
                return;
            }
        };
        let Some(handler) = handler else {
            log_println!("pcie: no MSI interrupt line");
            return;
        };
        IrqHandlerKey::from_key(RawKey::from_wire(handler.handler))
            .ack()
            .unwrap_or_else(|error| panic!("pcie: interrupt Ack failed: {:?}", error.code()));
        match &mut source {
            MsiSource::Own => bridge.raise_own_msi(0),
            MsiSource::Mip(mip, vector) => raise_mip(mip, *vector),
        }
        let arrived = match notification(init).await_bits(MSI_WAIT_NS) {
            Ok(bits) => bits & handler.bits != 0,
            Err(CapError::TimedOut { .. }) => false,
            Err(error) => panic!("pcie: MSI wait failed: {:?}", error.code()),
        };
        let pending = if init.pci.msi.kind == MSI_OWN {
            bridge.take_own_msis()
        } else {
            0
        };
        if arrived {
            log_println!(
                "pcie: MSI raised in software arrived on line {}{}",
                handler.line,
                if init.pci.msi.kind == MSI_OWN {
                    if pending & 1 == 1 { ", vector 0 pending" } else { ", no vector pending" }
                } else {
                    ""
                }
            );
        } else {
            log_println!("pcie: MSI raised in software did not arrive on line {}", handler.line);
        }
    }

    fn main(init_va: u64) -> ! {
        // SAFETY: Kickstart passes the init page's address in x0.
        let init = unsafe { driver_init(init_va) };
        map_device_pages(init)
            .unwrap_or_else(|error| panic!("pcie: mapping registers failed: {:?}", error.code()));
        let soc = if init.compatible_index == BCM2712 {
            Soc::Bcm2712
        } else {
            Soc::Bcm2711
        };
        let start = init.regions().first().map_or(0, |region| region.start);
        let registers = registers(init, 0).expect("pcie: registers mapped");
        let mut outbound = [Window {
            pci: 0,
            cpu: 0,
            size: 0,
        }; 4];
        let mut outbound_count = 0;
        for (slot, found) in outbound.iter_mut().zip(
            init.pci
                .windows()
                .iter()
                .filter(|window| window.space != PCI_SPACE_IO),
        ) {
            *slot = window(found);
            outbound_count += 1;
        }
        let mut inbound = outbound;
        let mut inbound_count = 0;
        for (slot, found) in inbound.iter_mut().zip(init.pci.inbound()) {
            *slot = window(found);
            inbound_count += 1;
        }
        let mut bridge = Bridge::new(registers, soc);
        let mut hooks = Hooks { init };
        match bridge.bring_up(
            &Setup {
                outbound: &outbound[..outbound_count],
                inbound: &inbound[..inbound_count],
                own_msi: init.pci.msi.kind == MSI_OWN,
            },
            &mut hooks,
        ) {
            Ok(link) => {
                log_println!(
                    "pcie: {start:#x} link up at {} GT/s x{}{}, revision {:#x}, {} windows, {} blocks of window memory",
                    match link.speed {
                        1 => "2.5",
                        2 => "5",
                        3 => "8",
                        _ => "?",
                    },
                    link.width,
                    if link.spread_spectrum { " with SSC" } else { "" },
                    link.revision,
                    outbound_count,
                    init.pci.blocks().len()
                );
                if let Some(target) = link.msi_target {
                    log_println!("pcie: MSIs to {target:#x}, data {:#06x} + vector", pcie_brcmstb::MSI_DATA_PATTERN);
                }
                let windows = init.pci.windows();
                let mut pci_windows = [libpci::Window {
                    pci: 0,
                    size: 0,
                    prefetchable: false,
                }; 4];
                for (slot, found) in pci_windows.iter_mut().zip(windows) {
                    *slot = libpci::Window {
                        pci: found.pci,
                        size: found.size,
                        prefetchable: found.prefetchable != 0,
                    };
                }
                let mut found = [Device::default(); MAX_FUNCTIONS];
                match enumerate(bridge.config(), &pci_windows[..windows.len()], &mut found) {
                    Ok(count) => {
                        log_println!("pcie: {count} functions");
                        for device in found.iter().take(count) {
                            report(init, device);
                        }
                    }
                    Err(error) => log_println!("pcie: enumeration failed: {error:?}"),
                }
                msi_self_test(init, &mut bridge);
            }
            Err(Error::LinkDown) => log_println!("pcie: {start:#x}: link down"),
            Err(error) => log_println!("pcie: {start:#x}: bring-up failed: {error:?}"),
        }
        loop {
            // Nothing to serve yet: the self-test's line may still signal.
            if let Err(error) = notification(init).await_bits(NotificationKey::WAIT_INFINITE) {
                panic!("pcie: idle wait failed: {:?}", error.code());
            }
        }
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}

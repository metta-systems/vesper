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
//! it found. Then it tests the MSI path from software: it raises the last
//! vector (in the bridge's own MSI controller on the BCM2711, in the MIP on
//! the BCM2712) and waits for its interrupt.
//!
//! It then serves the drivers of the functions behind it through `pci_v1`
//! (`interfaces/pci_v1.if`): `claim` carves device Frames for the caller's
//! BARs from the window memory straight into a table the caller lends, turns
//! the function's decoding and bus mastering on and, on the BCM2711, points
//! its MSI at the bridge with the caller's vector, its index among
//! `PciHost::children`. The Thread demultiplexes the bridge's MSI line: each
//! pending vector signals its child's Notification through the key Kickstart
//! minted for it. RP1's MSI-X vectors on the BCM2712 are not set up yet.

#[cfg(target_os = "vesper")]
mod component {
    use {
        core::cell::UnsafeCell,
        libdevice_driver::{
            DriverInit, INIT_VA, MAX_PCI_CHILDREN, MAX_WINDOW_BLOCKS, MSI_CONTROLLER, MSI_OWN,
            NONE_INDEX, PCI_SPACE_IO, PciWindow, Turn, Turnstile, device_address,
            driver_compatible, driver_init, map_device_pages, skip_chunks,
        },
        liblog_client::log_println,
        libmeddle_runtime::{DataArea, IoRec},
        libobject::{
            CapError, EventCountKey, IrqHandlerKey, KeyTableKey, NotificationKey, ObjectType,
            RawKey, Rights, UntypedKey, export::init_return_key, thread::ThreadReturnKey,
        },
        libpci::{BarKind, Device, Function, MAX_BARS, enumerate},
        pci::{
            claimed,
            pci_v1::{Exception, PciV1},
        },
        pcie_brcmstb::{
            Board, Bridge, Error, MSI_DATA_PATTERN, Mmio, OWN_MSI_VECTORS, RESCAL_POLLS, Setup,
            Soc, Window, calibration_done, raise_mip, set_reset, set_up_mip, start_calibration,
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

    /// The bridge's own MSI vector the self-test raises; children take
    /// vectors from 0 up.
    const SELF_TEST_VECTOR: u32 = OWN_MSI_VECTORS - 1;
    const _: () = assert!(MAX_PCI_CHILDREN < SELF_TEST_VECTOR as usize);

    /// Rights of the BAR Frames handed to a child: to map them, and to learn
    /// their extent.
    const FRAME_RIGHTS: Rights = Rights(Rights::READ | Rights::WRITE | Rights::GRANT);

    /// Size exponents of the Frames a BAR is cut into.
    const PAGE_FRAME_BITS: u8 = 12;
    const LARGE_FRAME_BITS: u8 = 21;

    /// Most Frames one Retype makes.
    const MAX_RETYPE_COUNT: u32 = 256;

    /// What the exports and the Thread share once the bridge is up: guarded
    /// by the turnstile.
    struct State {
        bridge: Option<Bridge<Registers>>,
        found: [Device; MAX_FUNCTIONS],
        count: usize,
        /// The bus address MSIs go to, when the bridge serves its own
        msi_target: Option<u64>,
        /// Which children claimed their function
        claimed: [bool; MAX_PCI_CHILDREN],
        /// The own-MSI vectors set up, one bit per child
        msi_vectors: u32,
        /// Where the watermark of each window block's Untyped stands, as an
        /// offset from its base: memory below it is gone for good
        marks: [u64; MAX_WINDOW_BLOCKS],
        /// Next free slot of the driver's own table, for the window memory
        /// skipped on the way to a BAR
        next_own_slot: u32,
    }

    struct Shared(UnsafeCell<State>);

    // SAFETY: the cell is used by the Thread before the component says it is
    // ready, and after that only by the holder of a `TURNSTILE` turn.
    unsafe impl Sync for Shared {}

    static STATE: Shared = Shared(UnsafeCell::new(State {
        bridge: None,
        found: [Device {
            function: Function {
                bus: 0,
                device: 0,
                function: 0,
            },
            vendor: 0,
            device: 0,
            class: 0,
            revision: 0,
            buses: None,
            bar_count: 0,
            bars: [libpci::Bar {
                index: 0,
                kind: BarKind::Memory32,
                pci: 0,
                size: 0,
                prefetchable: false,
                assigned: false,
            }; MAX_BARS],
            express: None,
            msi: None,
            msix: None,
        }; MAX_FUNCTIONS],
        count: 0,
        msi_target: None,
        claimed: [false; MAX_PCI_CHILDREN],
        msi_vectors: 0,
        marks: [0; MAX_WINDOW_BLOCKS],
        next_own_slot: 0,
    }));
    static TURNSTILE: Turnstile = Turnstile::new();

    #[allow(
        clippy::mut_from_ref,
        reason = "the exclusive borrow of the turn proves the holder has the cell to itself"
    )]
    fn state<'turn>(_turn: &'turn mut Turn<'_>) -> &'turn mut State {
        // SAFETY: only the holder of a turn gets here, one at a time.
        unsafe { &mut *STATE.0.get() }
    }

    fn init() -> &'static DriverInit {
        // SAFETY: Kickstart maps every driver's init record at `INIT_VA`; it
        // is read in this component's own `AddressSpace` only.
        unsafe { driver_init(INIT_VA) }
    }

    /// The function a routing ID names.
    fn function_at(routing: u64) -> Function {
        let [function_device, bus, ..] = routing.to_le_bytes();
        Function {
            bus,
            device: function_device >> 3,
            function: function_device & 0b111,
        }
    }

    struct Host;

    impl PciV1 for Host {
        fn enter(_badge: u64) {
            init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
                init().return_key,
            )));
        }

        fn address(badge: u64) -> Result<u32, Exception> {
            let init = init();
            let (_, child) = init.pci.child(badge).ok_or(Exception::NoFunction)?;
            let mut turn = TURNSTILE.enter(init);
            let state = state(&mut turn);
            let function = function_at(child.function);
            if !state.found[..state.count.min(MAX_FUNCTIONS)]
                .iter()
                .any(|device| device.function == function)
            {
                return Err(Exception::NoFunction);
            }
            u32::try_from(child.function).map_err(|_overflow| Exception::NoFunction)
        }

        fn claim(
            badge: u64,
            into: &KeyTableKey,
            found: IoRec,
            first_slot: u32,
        ) -> Result<u32, Exception> {
            let init = init();
            let (index, child) = init.pci.child(badge).ok_or(Exception::NoFunction)?;
            let (_, area) = init.area_for_badge(badge).ok_or(Exception::BadRecord)?;
            let area = DataArea {
                base: area.base,
                len: area.len,
            };
            if !area.contains(&found) || found.len < claimed::BYTES {
                return Err(Exception::BadRecord);
            }
            let mut turn = TURNSTILE.enter(init);
            let state = state(&mut turn);
            let function = function_at(child.function);
            let device = *state.found[..state.count.min(MAX_FUNCTIONS)]
                .iter()
                .find(|device| device.function == function)
                .ok_or(Exception::NoFunction)?;
            if state.claimed[index] {
                return Err(Exception::AlreadyClaimed);
            }

            // The memory BARs a window holds, in address order.
            let mut bars = [(0, 0, 0, 0); claimed::MAX_BARS];
            let mut bar_count = 0;
            for bar in device
                .bars()
                .iter()
                .filter(|bar| bar.assigned && bar.kind != BarKind::Io)
            {
                if let Some(cpu) = cpu_address(init, bar.pci) {
                    bars[bar_count] = (u64::from(bar.index), bar.pci, cpu, bar.size);
                    bar_count += 1;
                }
            }
            bars[..bar_count].sort_unstable_by_key(|&(_, _, cpu, _)| cpu);
            let mut slot = first_slot;
            let mut installed = 0;
            for &(_, _, cpu, size) in &bars[..bar_count] {
                installed += carve(init, state, into, cpu, size, slot, installed)?;
                slot = first_slot.saturating_add(installed);
            }

            let bridge = state.bridge.as_mut().ok_or(Exception::NoFunction)?;
            libpci::enable(bridge.config(), function);
            let vector = u32::try_from(index).unwrap_or(u32::MAX);
            let msi_bits = match (device.msi, state.msi_target) {
                (Some(capability), Some(target))
                    if init.pci.msi.kind == MSI_OWN && child.msi_key != 0 =>
                {
                    let data = MSI_DATA_PATTERN | u16::try_from(vector).unwrap_or(0);
                    libpci::set_up_msi(bridge.config(), function, capability, target, data);
                    state.msi_vectors |= 1 << vector;
                    child.msi_bits
                }
                _ => 0,
            };
            state.claimed[index] = true;

            let offset = found.base - area.base;
            let header = [
                u64::from(device.vendor) | u64::from(device.device) << 16,
                u64::from(device.class),
                msi_bits,
                bar_count as u64,
            ];
            let mut written = area.write_u64s(offset + claimed::IDENTITY, &header);
            for (position, &(number, pci, cpu, size)) in bars[..bar_count].iter().enumerate() {
                written &= area.write_u64s(
                    offset + claimed::BARS + (position * claimed::BAR_WORDS * 8) as u64,
                    &[number, pci, cpu, size],
                );
            }
            if !written {
                return Err(Exception::BadRecord);
            }
            Ok(installed)
        }

        fn raise_msi(badge: u64) -> Result<(), Exception> {
            let init = init();
            let (index, _) = init.pci.child(badge).ok_or(Exception::NoFunction)?;
            let mut turn = TURNSTILE.enter(init);
            let state = state(&mut turn);
            if !state.claimed[index] {
                return Err(Exception::NoFunction);
            }
            let vector = u32::try_from(index).unwrap_or(u32::MAX);
            if state.msi_vectors & 1 << vector == 0 {
                return Err(Exception::NoMsi);
            }
            state
                .bridge
                .as_mut()
                .ok_or(Exception::NoFunction)?
                .raise_own_msi(vector);
            Ok(())
        }
    }

    pci::pci_v1_export!(Host);

    /// Carve Frames for the `size` bytes of BAR memory at CPU address `cpu`
    /// from the window block holding them straight into `into`, from `slot`;
    /// how many. `installed` Frames of the call went in before, for the
    /// error. Retype allocates at an Untyped's watermark, so the block's
    /// memory below the BAR is first carved into Untypeds that stay in the
    /// driver's own table.
    fn carve(
        init: &DriverInit,
        state: &mut State,
        into: &KeyTableKey,
        cpu: u64,
        size: u64,
        slot: u32,
        installed: u32,
    ) -> Result<u32, Exception> {
        let failed = Exception::InstallFailed { installed };
        let (block_index, block) = init
            .pci
            .blocks()
            .iter()
            .enumerate()
            .find(|(_, block)| {
                block.base <= cpu && cpu + size <= block.base + (1 << block.size_bits)
            })
            .ok_or(failed)?;
        let untyped = UntypedKey::from_key(RawKey::from_wire(block.key));
        let own = KeyTableKey::from_key(RawKey::from_wire(init.table));
        let offset = cpu - block.base;
        let mark = state.marks[block_index];
        if mark > offset {
            return Err(failed);
        }
        let mut cursor = mark;
        for bits in skip_chunks(mark, offset) {
            untyped
                .retype(
                    ObjectType::UNTYPED,
                    bits,
                    0,
                    1,
                    &own,
                    state.next_own_slot,
                    Rights::all(),
                )
                .map_err(|_error| failed)?;
            state.next_own_slot += 1;
            cursor += 1 << bits;
            state.marks[block_index] = cursor;
        }
        let frame_bits = if size.is_multiple_of(1 << LARGE_FRAME_BITS) {
            LARGE_FRAME_BITS
        } else {
            PAGE_FRAME_BITS
        };
        let count = u32::try_from(size >> frame_bits).map_err(|_overflow| failed)?;
        if count == 0 || count > MAX_RETYPE_COUNT {
            return Err(failed);
        }
        untyped
            .retype(
                ObjectType::FRAME,
                frame_bits,
                0,
                count,
                into,
                slot,
                FRAME_RIGHTS,
            )
            .map_err(|_error| failed)?;
        state.marks[block_index] = offset + size;
        Ok(count)
    }

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
                    if bar.prefetchable {
                        " prefetchable"
                    } else {
                        ""
                    },
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
        /// The last vector of the bridge's own controller
        Own,
        /// The last vector of the MIP
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
                let last_vector =
                    (init.pci.msi.first_vector + init.pci.msi.vector_count).saturating_sub(1);
                (
                    usize::try_from(init.pci.msi.interrupt)
                        .ok()
                        .and_then(|index| init.interrupts().get(index).copied()),
                    MsiSource::Mip(mip, u32::try_from(last_vector).unwrap_or(0)),
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
            MsiSource::Own => bridge.raise_own_msi(SELF_TEST_VECTOR),
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
                    if pending & 1 << SELF_TEST_VECTOR != 0 {
                        ", its vector pending"
                    } else {
                        ", no vector pending"
                    }
                } else {
                    ""
                }
            );
        } else {
            log_println!(
                "pcie: MSI raised in software did not arrive on line {}",
                handler.line
            );
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
                    if link.spread_spectrum {
                        " with SSC"
                    } else {
                        ""
                    },
                    link.revision,
                    outbound_count,
                    init.pci.blocks().len()
                );
                if let Some(target) = link.msi_target {
                    log_println!(
                        "pcie: MSIs to {target:#x}, data {:#06x} + vector",
                        pcie_brcmstb::MSI_DATA_PATTERN
                    );
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
                let count =
                    match enumerate(bridge.config(), &pci_windows[..windows.len()], &mut found) {
                        Ok(count) => {
                            log_println!("pcie: {count} functions");
                            for device in found.iter().take(count) {
                                report(init, device);
                            }
                            count.min(MAX_FUNCTIONS)
                        }
                        Err(error) => {
                            log_println!("pcie: enumeration failed: {error:?}");
                            0
                        }
                    };
                msi_self_test(init, &mut bridge);
                // SAFETY: the component has not said it is ready, so no
                // caller holds a turn yet.
                let shared = unsafe { &mut *STATE.0.get() };
                shared.found = found;
                shared.count = count;
                shared.msi_target = link.msi_target;
                shared.bridge = Some(bridge);
            }
            Err(Error::LinkDown) => log_println!("pcie: {start:#x}: link down"),
            Err(error) => log_println!("pcie: {start:#x}: bring-up failed: {error:?}"),
        }
        serve(init)
    }

    /// Open the turnstile, say the component is ready, and hand on the MSIs
    /// of the bridge's own controller to the children whose vectors they
    /// are.
    fn serve(init: &DriverInit) -> ! {
        let table = KeyTableKey::from_key(RawKey::from_wire(init.table));
        let first_free = u32::try_from(init.first_free_slot).unwrap_or(u32::MAX);
        let turns = UntypedKey::from_key(RawKey::from_wire(init.memory))
            .retype(
                ObjectType::EVENT_COUNT,
                0,
                0,
                1,
                &table,
                first_free,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("pcie: Retype failed: {:?}", error.code()));
        // SAFETY: as in `main`: no caller holds a turn before the advance below.
        unsafe { &mut *STATE.0.get() }.next_own_slot = first_free + 1;
        TURNSTILE.open(turns);
        EventCountKey::from_key(RawKey::from_wire(init.event_count))
            .advance(1)
            .unwrap_or_else(|error| panic!("pcie: ready advance failed: {:?}", error.code()));
        // The bridge's own MSIs arrive on its first line (its `pcie` and `msi`
        // lines are one on the BCM2711).
        let msi_line = init
            .interrupts()
            .first()
            .copied()
            .filter(|_| init.pci.msi.kind == MSI_OWN);
        let ack = |handler: u64| {
            IrqHandlerKey::from_key(RawKey::from_wire(handler))
                .ack()
                .unwrap_or_else(|error| panic!("pcie: interrupt Ack failed: {:?}", error.code()));
        };
        if let Some(line) = msi_line {
            ack(line.handler);
        }
        loop {
            let bits = notification(init)
                .await_bits(NotificationKey::WAIT_INFINITE)
                .unwrap_or_else(|error| panic!("pcie: wait failed: {:?}", error.code()));
            let Some(line) = msi_line.filter(|line| bits & line.bits != 0) else {
                continue;
            };
            let mut turn = TURNSTILE.enter(init);
            let state = state(&mut turn);
            let pending = state.bridge.as_mut().map_or(0, Bridge::take_own_msis);
            for (vector, child) in init.pci.children().iter().enumerate() {
                if pending & state.msi_vectors & 1 << vector != 0 {
                    NotificationKey::from_key(RawKey::from_wire(child.msi_key))
                        .signal(child.msi_bits)
                        .unwrap_or_else(|error| {
                            panic!("pcie: MSI Signal failed: {:?}", error.code())
                        });
                }
            }
            drop(turn);
            ack(line.handler);
        }
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}

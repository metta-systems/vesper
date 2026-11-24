// The component runs on Vesper only; see the host stub below.
#![cfg_attr(target_os = "vesper", no_std, no_main)]

//! The spi component: the one owner of the BCM2835/BCM2711 SPI0 block, behind
//! `spi_v1` (see `interfaces/spi_v1.if`), polled.
//!
//! Kickstart loads it for the `brcm,bcm2835-spi` node and gives it a copy of
//! the device tree (`device_tree = true`), from which it reads its node's
//! chip selects (`cs-gpios`). At start it releases every chip select, routes
//! the node's pin group (MISO, MOSI and SCLK on ALT0) and learns the core
//! clock's fastest rate from the firmware: each transaction's divisor is
//! sized against it, so SCLK never exceeds the rate a device asks for, and
//! runs slower while the firmware lowers the clock.
//!
//! A device driver on the bus (its node a child of the controller's, `uses
//! spi_v1 via device parent`) writes transactions; calls take turns, a
//! transaction being a chip select, a burst of FIFO writes and a release.

// The interface crates link the syscall layer, which builds for AArch64 only,
// so a host build has nothing but this stub: the library and its host tests
// (`tests/spi.rs`) carry the logic.
#[cfg(not(target_os = "vesper"))]
fn main() {}

#[cfg(target_os = "vesper")]
mod component {
    use {
        core::sync::atomic::{AtomicU32, AtomicU64, Ordering},
        gpio_client::gpio_v1,
        libdevice_driver::{
            DriverInit, INIT_VA, Turnstile, device_address, driver_compatible, driver_init,
            map_device_pages,
        },
        libdevicetree::{
            DeviceTree, FallibleIterator, INDEX_BUFFER_BYTES, PropReader, gpio_specifier, property,
        },
        libmeddle_runtime::{DataArea, IoRec},
        libobject::{
            EventCountKey, KeyTableKey, NotificationKey, ObjectType, RawKey, Rights, UntypedKey,
            export::init_return_key, thread::ThreadReturnKey,
        },
        libserial_trace::{Trace, trace},
        mailbox_client::mailbox_v1,
        spi::spi_v1::{Exception, Mode, SpiV1},
        spi_bcm2835::{MmioRegisterBlock, RegisterBlock, Registers, idle, set_rate, write},
    };

    libuser::entry!(main);

    driver_compatible!("brcm,bcm2835-spi");

    /// Most chip selects the component drives (`cs-gpios` entries).
    const MAX_CHIP_SELECTS: usize = 4;

    /// Each chip select's GPIO: zero for none, else the pin plus one, with
    /// [`ACTIVE_LOW`] set when driving it low asserts it.
    static CHIP_SELECTS: [AtomicU32; MAX_CHIP_SELECTS] =
        [const { AtomicU32::new(0) }; MAX_CHIP_SELECTS];

    /// [`CHIP_SELECTS`]: the line is asserted low.
    const ACTIVE_LOW: u32 = 1 << 16;

    /// Where the block's registers are mapped (zero until set up).
    static REGISTERS_VA: AtomicU64 = AtomicU64::new(0);

    /// The core clock's fastest rate, which the divisors are sized against.
    static CORE_MAX_HZ: AtomicU32 = AtomicU32::new(0);

    /// Admits one transaction at a time to the block.
    static TURNSTILE: Turnstile = Turnstile::new();

    /// Bytes read from the caller's data area at a time.
    const CHUNK_BYTES: usize = 64;

    /// The fixed allowance of a transaction beyond its bytes' wire time.
    const TRANSACTION_SLACK_NS: u64 = 10_000_000;

    /// The device tree index (`libdevicetree`), built once at start.
    static mut INDEX: [u8; INDEX_BUFFER_BYTES] = [0; INDEX_BUFFER_BYTES];

    fn init() -> &'static DriverInit {
        // SAFETY: Kickstart maps every driver's init record at `INIT_VA`; it
        // is read in this component's own `AddressSpace` only.
        unsafe { driver_init(INIT_VA) }
    }

    /// The block's registers.
    ///
    /// # Safety
    ///
    /// The caller holds a turn of [`TURNSTILE`], so no other caller touches
    /// them.
    unsafe fn registers() -> MmioRegisterBlock<'static> {
        // SAFETY: `main` mapped the registers for good before the first call.
        unsafe {
            RegisterBlock::new_mmio(REGISTERS_VA.load(Ordering::Acquire) as *mut RegisterBlock)
        }
    }

    /// Wait until `main` has read the chip selects and the clock: a client's
    /// call can come before the component's own Thread finished setting up.
    fn wait_ready() {
        EventCountKey::from_key(RawKey::from_wire(init().event_count))
            .await_ge(1, EventCountKey::WAIT_INFINITE)
            .unwrap_or_else(|error| panic!("spi: ready wait failed: {:?}", error.code()));
    }

    fn gpio() -> gpio_v1::Client {
        gpio_v1::Client::new(gpio_v1::Bindings::from_imports(init().imports()))
    }

    /// The chip select `number`'s pin and whether it is asserted low.
    fn chip_select(number: u32) -> Option<(u32, bool)> {
        let entry = CHIP_SELECTS
            .get(usize::try_from(number).ok()?)?
            .load(Ordering::Acquire);
        (entry != 0).then(|| ((entry & 0xffff) - 1, entry & ACTIVE_LOW != 0))
    }

    /// Drive chip select `(pin, active_low)` asserted or released.
    fn drive(chip_select: (u32, bool), asserted: bool) {
        let (pin, active_low) = chip_select;
        // The pin is the component's own (its node's `cs-gpios`): a refusal
        // means the chip select never moves and every transaction is lost, so
        // it ends the component rather than passing for a write.
        gpio()
            .set_level(pin, asserted != active_low)
            .unwrap_or_else(|error| panic!("spi: chip select GPIO {pin}: {error:?}"));
    }

    /// The SPI mode of `mode`.
    fn spi_mode(mode: Mode) -> spi_bcm2835::Mode {
        spi_bcm2835::Mode::numbered(match mode {
            Mode::Mode0 => 0,
            Mode::Mode1 => 1,
            Mode::Mode2 => 2,
            Mode::Mode3 => 3,
        })
    }

    /// The bytes of a reference in the caller's data area, read a chunk at a
    /// time.
    struct AreaBytes {
        area: DataArea,
        offset: u64,
        end: u64,
        chunk: [u8; CHUNK_BYTES],
        at: usize,
        filled: usize,
    }

    impl Iterator for AreaBytes {
        type Item = u8;

        fn next(&mut self) -> Option<u8> {
            if self.at == self.filled {
                let remaining = usize::try_from(self.end - self.offset).ok()?;
                let count = remaining.min(CHUNK_BYTES);
                if count == 0 || !self.area.read_bytes(self.offset, &mut self.chunk[..count]) {
                    return None;
                }
                self.offset += count as u64;
                self.at = 0;
                self.filled = count;
            }
            let byte = self.chunk[self.at];
            self.at += 1;
            Some(byte)
        }
    }

    /// A waiter that gives up `allowance_ns` after its first call.
    fn deadline(allowance_ns: u64) -> impl FnMut() -> bool {
        let mut until = None;
        move || {
            let now = libuser::clock::now_ns();
            let until = *until.get_or_insert(now + allowance_ns);
            core::hint::spin_loop();
            now < until
        }
    }

    /// The bus.
    struct Spi;

    impl SpiV1 for Spi {
        fn enter(_badge: u64) {
            init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
                init().return_key,
            )));
        }

        fn write(
            badge: u64,
            chip_select_number: u32,
            max_hz: u32,
            mode: Mode,
            data: IoRec,
        ) -> Result<u32, Exception> {
            wait_ready();
            let chip_select =
                chip_select(chip_select_number).ok_or(Exception::InvalidChipSelect {
                    chip_select: chip_select_number,
                })?;
            let (_client, exported) = init().area_for_badge(badge).ok_or(Exception::InvalidData)?;
            let area = DataArea {
                base: exported.base,
                len: exported.len,
            };
            if !area.contains(&data) {
                return Err(Exception::InvalidData);
            }
            let core_max_hz = CORE_MAX_HZ.load(Ordering::Acquire);
            let divisor = spi_bcm2835::divisor(core_max_hz, max_hz);
            let hz = spi_bcm2835::rate(core_max_hz, divisor);
            if data.len == 0 {
                return Ok(hz);
            }
            let _turn = TURNSTILE.enter(init());
            // SAFETY: this caller holds the turn.
            let mut registers = unsafe { registers() };
            set_rate(&mut registers, core_max_hz, max_hz);
            let wire_ns = data.len * 8 * 1_000_000_000 / u64::from(hz.max(1));
            let offset = data.base - area.base;
            let bytes = AreaBytes {
                area,
                offset,
                end: offset + data.len,
                chunk: [0; CHUNK_BYTES],
                at: 0,
                filled: 0,
            };
            drive(chip_select, true);
            let written = write(
                &mut Registers(&mut registers),
                spi_mode(mode),
                bytes,
                deadline(2 * wire_ns + TRANSACTION_SLACK_NS),
            );
            drive(chip_select, false);
            written
                .map(|()| hz)
                .map_err(|_timed_out| Exception::Timeout)
        }
    }

    spi::spi_v1_export!(Spi);

    /// Read the node's chip selects from the device tree copy into
    /// [`CHIP_SELECTS`], and log the devices on the bus (its child nodes);
    /// returns how many chip selects it names.
    fn read_chip_selects(trace: &mut Trace, init: &DriverInit) -> usize {
        let (Some(blob), Some(path)) = (init.device_tree(), init.node_path()) else {
            trace!(trace, "spi: no device tree copy: no chip selects");
            return 0;
        };
        // SAFETY: Kickstart mapped the copy read-only for good; the index
        // buffer is used here once, before the first call.
        let tree = unsafe {
            DeviceTree::blob_from_raw(blob.as_ptr())
                .and_then(|raw| DeviceTree::new(raw, &mut *core::ptr::addr_of_mut!(INDEX)))
        };
        let Ok(tree) = tree else {
            trace!(trace, "spi: the device tree copy does not parse");
            return 0;
        };
        let Some(node) = tree.node(path) else {
            trace!(trace, "spi: {path} is not in the device tree copy");
            return 0;
        };
        // What sits on the bus, as the tree describes it: a device driver is
        // loaded for a child whose `compatible` names it.
        for child in node.children() {
            let name = child.name().unwrap_or("?");
            let status = property(&child, "status")
                .and_then(|prop| prop.str().ok())
                .unwrap_or("okay");
            trace!(trace, "spi: device {name} ({status})");
            if let Some(prop) = property(&child, "compatible") {
                let mut strings = prop.iter_str();
                while let Ok(Some(compatible)) = strings.next() {
                    trace!(trace, "spi:   compatible \"{compatible}\"");
                }
            }
        }
        let mut count = 0;
        for (number, entry) in CHIP_SELECTS.iter().enumerate() {
            let Some(specifier) = gpio_specifier(tree.index(), &node, "cs-gpios", number) else {
                break;
            };
            let active_low = if specifier.active_low() {
                ACTIVE_LOW
            } else {
                0
            };
            entry.store((specifier.pin + 1) | active_low, Ordering::Release);
            trace!(
                trace,
                "spi: chip select {number} is GPIO {}, active {}",
                specifier.pin,
                if specifier.active_low() {
                    "low"
                } else {
                    "high"
                }
            );
            count += 1;
        }
        count
    }

    /// Route the node's pin group (MISO, MOSI and SCLK on ALT0, the chip
    /// selects as outputs).
    fn route_pins(trace: &mut Trace, init: &DriverInit) {
        let gpio = gpio();
        for setting in init.pins() {
            let routed = gpio_v1::PinFunction::from_select_code(setting.function)
                .is_some_and(|function| gpio.set_function(setting.pin, function).is_ok());
            trace!(
                trace,
                "spi: GPIO {} to function {}: {}",
                setting.pin,
                setting.function,
                if routed { "routed" } else { "FAILED" }
            );
        }
    }

    /// Map the block, read the chip selects and the clock, set up the
    /// turnstile, then park: everything else happens in calls.
    fn main(init_va: u64) -> ! {
        // SAFETY: Kickstart passes the init page's address in x0.
        let init = unsafe { driver_init(init_va) };
        let slot = map_device_pages(init)
            .unwrap_or_else(|error| panic!("spi: mapping registers failed: {:?}", error.code()));
        REGISTERS_VA.store(
            device_address(init, init.regions()[0].start).expect("spi: registers mapped"),
            Ordering::Release,
        );
        let mut trace = Trace::new(init);
        // Release every chip select before the pin group makes them outputs,
        // so no device sees one asserted.
        let chip_selects = read_chip_selects(&mut trace, init);
        for number in 0..u32::try_from(chip_selects).unwrap_or(0) {
            if let Some(chip_select) = chip_select(number) {
                drive(chip_select, false);
            }
        }
        route_pins(&mut trace, init);
        let mailbox = mailbox_v1::Client::new(mailbox_v1::Bindings::from_imports(init.imports()));
        let core_max_hz = mailbox
            .get_max_clock_rate(mailbox_v1::Clock::Core)
            .unwrap_or_else(|error| panic!("spi: the core clock's fastest rate: {error:?}"));
        CORE_MAX_HZ.store(core_max_hz, Ordering::Release);
        // SAFETY: no call is served yet; nothing else touches the block.
        let mut registers = unsafe { registers() };
        registers.write_control_status(idle(spi_bcm2835::Mode::numbered(0)));
        trace!(
            trace,
            "spi: {chip_selects} chip selects, the core clock at most {core_max_hz} Hz"
        );
        let table = KeyTableKey::from_key(RawKey::from_wire(init.table));
        let turns = UntypedKey::from_key(RawKey::from_wire(init.memory))
            .retype(
                ObjectType::EVENT_COUNT,
                0,
                0,
                1,
                &table,
                u32::try_from(slot).unwrap_or(u32::MAX),
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("spi: Retype failed: {:?}", error.code()));
        TURNSTILE.open(turns);
        EventCountKey::from_key(RawKey::from_wire(init.event_count))
            .advance(1)
            .unwrap_or_else(|error| panic!("spi: ready advance failed: {:?}", error.code()));
        let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
        loop {
            let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
        }
    }
}

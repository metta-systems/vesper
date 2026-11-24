// The component runs on Vesper only; see the host stub below.
#![cfg_attr(target_os = "vesper", no_std, no_main)]

//! The ILI9341 panel driver: the MSP2202 2.2" 240x320 SPI module on the
//! Raspberry Pi 4's SPI0 (the `mipi-dbi-spi` overlay's `panel@0`). Its node
//! names the controller first: `ili9341` as the overlay's `compatible=`
//! parameter can carry it (the firmware splits parameter values at commas),
//! or `ilitek,ili9341` as a tree written in full names it.
//!
//! Kickstart loads it for the panel node, which sits on the SPI controller's
//! node, links its `spi_v1` through that parent and gives it a copy of the
//! device tree (`device_tree = true`). It reads its node: its chip select
//! (`reg`), its clock (`spi-max-frequency`), its D/C and reset lines
//! (`dc-gpios`, `reset-gpios`), its backlight (the `gpios` of the node its
//! `backlight` names), its resolution (`panel-timing`) and its size in mm.
//!
//! It drives the panel with `mipidsi`, over embedded-hal implementations of
//! Vesper's own: `SpiDevice` writes each buffer through `spi_v1.write` from
//! the shared data area, `OutputPin` sets a level through `gpio_v1`, and
//! `DelayNs` waits on the driver's Notification, which nothing signals. It
//! resets and initializes the panel, turns the backlight on and fills the
//! screen with colour bars, logging how long a full screen took; then it
//! parks. A panel interface for the compositor comes later.

// The interface crates link the syscall layer, which builds for AArch64 only.
#[cfg(not(target_os = "vesper"))]
fn main() {}

#[cfg(target_os = "vesper")]
mod component {
    use {
        core::{
            iter,
            sync::atomic::{AtomicU32, Ordering},
        },
        embedded_graphics_core::pixelcolor::{Rgb565, RgbColor},
        embedded_hal::{
            delay::DelayNs,
            digital::{self, OutputPin},
            spi::{self, Operation, SpiDevice},
        },
        gpio_client::gpio_v1,
        libdevice_driver::{DriverInit, driver_compatible, driver_init},
        libdevicetree::{
            DeviceTree, INDEX_BUFFER_BYTES, PropReader, gpio_specifier, linked_phandle,
            node_by_phandle, property,
        },
        libmeddle_runtime::DataArea,
        libobject::{NotificationKey, RawKey},
        libserial_trace::{Trace, trace},
        mipidsi::{Builder, interface::SpiInterface, models::ILI9341Rgb565},
        spi_client::spi_v1::{self, Mode},
    };

    libuser::entry!(main);

    driver_compatible!("ilitek,ili9341", "ili9341");

    /// The device tree index (`libdevicetree`).
    static mut INDEX: [u8; INDEX_BUFFER_BYTES] = [0; INDEX_BUFFER_BYTES];

    /// The bytes of one `spi_v1.write`: the whole data area.
    const BUFFER_BYTES: usize = 4096;

    /// mipidsi's pixel buffer, flushed through one write each time it fills.
    static mut PIXELS: [u8; BUFFER_BYTES] = [0; BUFFER_BYTES];

    /// Below this, a delay spins on the clock; above, it waits on the
    /// Notification.
    const SPIN_LIMIT_NS: u64 = 1_000_000;

    /// A GPIO line of the panel: a pin and whether it is asserted low.
    #[derive(Clone, Copy)]
    struct Line {
        pin: u32,
        active_low: bool,
    }

    /// What the panel's node says.
    struct Node {
        chip_select: u32,
        max_hz: u32,
        dc: Line,
        reset: Option<Line>,
        backlight: Option<Line>,
        width: u16,
        height: u16,
        width_mm: u32,
        height_mm: u32,
    }

    /// Read the panel's node from the device tree copy.
    fn read_node(init: &DriverInit) -> Node {
        let blob = init.device_tree().expect("panel: a device tree copy");
        let path = init.node_path().expect("panel: a node");
        // SAFETY: Kickstart mapped the copy read-only for good; the index
        // buffer is used here once.
        let tree = unsafe {
            DeviceTree::blob_from_raw(blob.as_ptr())
                .and_then(|raw| DeviceTree::new(raw, &mut *core::ptr::addr_of_mut!(INDEX)))
        }
        .unwrap_or_else(|error| panic!("panel: the device tree copy: {error:?}"));
        let index = tree.index();
        let node = tree
            .node(path)
            .unwrap_or_else(|| panic!("panel: {path} is not in the copy"));
        let number = |node: &libdevicetree::Node<'_, '_, '_>, name: &str| {
            property(node, name).and_then(|prop| prop.u32(0).ok())
        };
        let line = |node: &libdevicetree::Node<'_, '_, '_>, name: &str| {
            gpio_specifier(index, node, name, 0).map(|specifier| Line {
                pin: specifier.pin,
                active_low: specifier.active_low(),
            })
        };
        let timing = node
            .children()
            .find(|child| child.name() == Ok("panel-timing"))
            .expect("panel: panel-timing");
        let backlight = linked_phandle(&node, "backlight")
            .and_then(|linked| node_by_phandle(index, linked))
            .and_then(|backlight| line(&backlight, "gpios"));
        Node {
            chip_select: number(&node, "reg").expect("panel: reg"),
            max_hz: number(&node, "spi-max-frequency").unwrap_or(10_000_000),
            dc: line(&node, "dc-gpios").expect("panel: dc-gpios"),
            reset: line(&node, "reset-gpios"),
            backlight,
            width: u16::try_from(number(&timing, "hactive").expect("panel: hactive"))
                .unwrap_or(u16::MAX),
            height: u16::try_from(number(&timing, "vactive").expect("panel: vactive"))
                .unwrap_or(u16::MAX),
            width_mm: number(&node, "width-mm").unwrap_or(0),
            height_mm: number(&node, "height-mm").unwrap_or(0),
        }
    }

    /// The bus failed a write.
    #[derive(Debug, Clone, Copy)]
    struct BusError;

    impl spi::Error for BusError {
        fn kind(&self) -> spi::ErrorKind {
            spi::ErrorKind::Other
        }
    }

    /// The ILI9341's rated write clock: 100 ns serial write cycle (datasheet
    /// section 18.3.4, `twc`). The initialization commands go at this rate and
    /// only pixels at the node's `spi-max-frequency`, as Linux's
    /// `panel-mipi-dbi` does, so a command garbled at an overclocked rate
    /// cannot leave the controller misconfigured.
    const SETUP_HZ: u32 = 10_000_000;

    /// The rate writes ask for: [`SETUP_HZ`] (or less) until the panel is
    /// initialized, then the node's `spi-max-frequency`.
    static WRITE_HZ: AtomicU32 = AtomicU32::new(SETUP_HZ);

    /// The panel's device on the SPI bus: each write goes through
    /// `spi_v1.write` from the shared data area, a data area's worth at a time,
    /// at [`WRITE_HZ`].
    struct Bus {
        spi: spi_v1::Client,
        area: DataArea,
        chip_select: u32,
    }

    /// The rate the last write ran at.
    static LAST_HZ: AtomicU32 = AtomicU32::new(0);

    impl Bus {
        /// Write `bytes` a data area's worth at a time.
        fn write_chunks(&mut self, bytes: &[u8]) -> Result<(), BusError> {
            let max_hz = WRITE_HZ.load(Ordering::Relaxed);
            for chunk in bytes.chunks(BUFFER_BYTES) {
                let len = chunk.len() as u64;
                if !self.area.write_bytes(0, chunk) {
                    return Err(BusError);
                }
                let reference = self.area.reference(0, len).ok_or(BusError)?;
                let hz = self
                    .spi
                    .write(self.chip_select, max_hz, Mode::Mode0, reference)
                    .map_err(|_error| BusError)?;
                LAST_HZ.store(hz, Ordering::Relaxed);
            }
            Ok(())
        }
    }

    impl spi::ErrorType for Bus {
        type Error = BusError;
    }

    impl SpiDevice for Bus {
        fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), BusError> {
            for operation in operations {
                match operation {
                    Operation::Write(bytes) => self.write_chunks(bytes)?,
                    Operation::DelayNs(ns) => Delay.delay_ns(*ns),
                    // The panel is write-only (MISO is not wired).
                    Operation::Read(_)
                    | Operation::Transfer(..)
                    | Operation::TransferInPlace(_) => {
                        return Err(BusError);
                    }
                }
            }
            Ok(())
        }
    }

    /// The gpio component refused a level.
    #[derive(Debug, Clone, Copy)]
    struct PinError;

    impl digital::Error for PinError {
        fn kind(&self) -> digital::ErrorKind {
            digital::ErrorKind::Other
        }
    }

    /// A panel line driven through `gpio_v1`.
    struct Pin {
        gpio: gpio_v1::Client,
        line: Line,
    }

    impl Pin {
        /// `line`, made an output and released.
        fn output(line: Line) -> Self {
            let pin = Self { gpio: gpio(), line };
            let _released = pin.gpio.set_level(line.pin, line.active_low);
            pin.gpio
                .set_function(line.pin, gpio_v1::PinFunction::Output)
                .unwrap_or_else(|error| panic!("panel: GPIO {} as an output: {error:?}", line.pin));
            pin
        }
    }

    impl digital::ErrorType for Pin {
        type Error = PinError;
    }

    impl OutputPin for Pin {
        fn set_low(&mut self) -> Result<(), PinError> {
            self.gpio
                .set_level(self.line.pin, false)
                .map_err(|_error| PinError)
        }

        fn set_high(&mut self) -> Result<(), PinError> {
            self.gpio
                .set_level(self.line.pin, true)
                .map_err(|_error| PinError)
        }
    }

    /// Delays on the clock, and on the driver's Notification for long ones.
    struct Delay;

    impl DelayNs for Delay {
        fn delay_ns(&mut self, ns: u32) {
            let ns = u64::from(ns);
            if ns >= SPIN_LIMIT_NS {
                // Nothing signals the Notification: the wait ends at its
                // timeout.
                let park = NotificationKey::from_key(RawKey::from_wire(init().notification));
                let _timed_out = park.await_bits(ns);
                return;
            }
            let until = libuser::clock::now_ns() + ns;
            while libuser::clock::now_ns() < until {
                core::hint::spin_loop();
            }
        }
    }

    fn init() -> &'static DriverInit {
        // SAFETY: Kickstart maps every driver's init record at `INIT_VA`.
        unsafe { driver_init(libdevice_driver::INIT_VA) }
    }

    fn gpio() -> gpio_v1::Client {
        gpio_v1::Client::new(gpio_v1::Bindings::from_imports(init().imports()))
    }

    /// The number of colour bars.
    const BAR_COUNT: u16 = 8;

    /// The colour bars, top to bottom.
    const BARS: [Rgb565; BAR_COUNT as usize] = [
        Rgb565::RED,
        Rgb565::GREEN,
        Rgb565::BLUE,
        Rgb565::WHITE,
        Rgb565::BLACK,
        Rgb565::YELLOW,
        Rgb565::CYAN,
        Rgb565::MAGENTA,
    ];

    fn main(init_va: u64) -> ! {
        // SAFETY: Kickstart passes the init page's address in x0.
        let init = unsafe { driver_init(init_va) };
        let mut trace = Trace::new(init);
        let node = read_node(init);
        trace!(
            trace,
            "panel: {}x{} ({}x{} mm) on chip select {} at up to {} Hz; D/C GPIO {}, reset {:?}, backlight {:?}",
            node.width,
            node.height,
            node.width_mm,
            node.height_mm,
            node.chip_select,
            node.max_hz,
            node.dc.pin,
            node.reset.map(|reset| reset.pin),
            node.backlight.map(|backlight| backlight.pin)
        );

        let spi = spi_v1::Client::new(spi_v1::Bindings::from_imports(init.imports()));
        let area = spi.data_area().expect("panel: spi_v1 has a data area");
        // The setup rate, or the node's own if that is slower.
        WRITE_HZ.store(SETUP_HZ.min(node.max_hz), Ordering::Relaxed);
        let bus = Bus {
            spi,
            area,
            chip_select: node.chip_select,
        };
        let dc = Pin::output(node.dc);
        let backlight = node.backlight.map(Pin::output);
        // SAFETY: the buffer is lent to the interface for good, here only.
        let pixels = unsafe { &mut *core::ptr::addr_of_mut!(PIXELS) };
        let interface = SpiInterface::new(bus, dc, pixels);
        let builder = Builder::new(ILI9341Rgb565, interface).display_size(node.width, node.height);
        // The builder's type changes with the reset pin: light the panel
        // through either.
        match node.reset {
            Some(reset) => light(
                &mut trace,
                builder.reset_pin(Pin::output(reset)),
                &node,
                backlight,
            ),
            None => light(&mut trace, builder, &node, backlight),
        }
        let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
        loop {
            let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
        }
    }

    /// Initialize the panel, turn its backlight on and draw [`BARS`], logging
    /// how long each took.
    fn light<RST: OutputPin>(
        trace: &mut Trace,
        builder: Builder<SpiInterface<'static, Bus, Pin>, ILI9341Rgb565, RST>,
        node: &Node,
        backlight: Option<Pin>,
    ) where
        RST::Error: core::fmt::Debug,
    {
        let started = libuser::clock::now_ns();
        let mut display = match builder.init(&mut Delay) {
            Ok(display) => display,
            Err(error) => {
                trace!(trace, "panel: initialization FAILED: {error:?}");
                return;
            }
        };
        trace!(
            trace,
            "panel: initialized in {} ms, commands at {} Hz",
            (libuser::clock::now_ns() - started) / 1_000_000,
            LAST_HZ.load(Ordering::Relaxed)
        );
        // From here on only pixels go out (and the address window commands
        // before them): at the node's rate.
        WRITE_HZ.store(node.max_hz, Ordering::Relaxed);
        if let Some(mut backlight) = backlight {
            let lit = backlight.set_high().is_ok();
            trace!(trace, "panel: backlight on: {lit}");
        }
        match draw_bars(&mut display, node.width, node.height) {
            Ok(us) => {
                let bytes = u64::from(node.width) * u64::from(node.height) * 2;
                trace!(
                    trace,
                    "panel: {} colour bars over the full screen ({bytes} bytes) in {} us, {} KiB/s, pixels at {} Hz",
                    BARS.len(),
                    us,
                    bytes * 1_000_000 / us.max(1) / 1024,
                    LAST_HZ.load(Ordering::Relaxed)
                );
            }
            Err(error) => trace!(trace, "panel: drawing FAILED: {error:?}"),
        }
    }

    /// Fill the screen with [`BARS`], one band per colour; returns the time
    /// the full screen took, in microseconds.
    fn draw_bars<RST: OutputPin>(
        display: &mut mipidsi::Display<SpiInterface<'static, Bus, Pin>, ILI9341Rgb565, RST>,
        width: u16,
        height: u16,
    ) -> Result<u64, mipidsi::interface::SpiError<BusError, PinError>> {
        let started = libuser::clock::now_ns();
        let band = height / BAR_COUNT;
        for (number, colour) in (0..BAR_COUNT).zip(BARS.iter()) {
            let top = band * number;
            let pixels = usize::from(width) * usize::from(band);
            display.set_pixels(
                0,
                top,
                width - 1,
                top + band - 1,
                iter::repeat_n(*colour, pixels),
            )?;
        }
        Ok((libuser::clock::now_ns() - started) / 1000)
    }
}

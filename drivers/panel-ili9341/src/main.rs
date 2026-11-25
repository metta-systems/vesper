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
//! resets and initializes the panel, clears it to black and turns the
//! backlight on.
//!
//! Then it serves `display_v1` as a flush output (see
//! `drivers/display/interfaces/display_v1.if`): a client writes the screen in
//! big-endian RGB565 in its data area, and `flush` sends a rectangle of it.
//! The panel has one mode, turns itself (`rotate`, the controller's MADCTL)
//! and has no memory of its own to hand out (`frames` is unsupported). Calls
//! take turns: one transaction on the bus at a time. The screen geometry is
//! the library's (`panel_ili9341`), with host tests in `tests/panel.rs`.

// The interface crates link the syscall layer, which builds for AArch64 only,
// so a host build has nothing but this stub: the library and its host tests
// carry the logic.
#[cfg(not(target_os = "vesper"))]
fn main() {}

#[cfg(target_os = "vesper")]
mod component {
    use {
        core::{
            cell::UnsafeCell,
            iter,
            sync::atomic::{AtomicU32, Ordering},
        },
        display::display_v1::{self, DisplayV1, Exception, Format},
        embedded_graphics_core::pixelcolor::{Rgb565, RgbColor, raw::RawU16},
        embedded_hal::{
            delay::DelayNs,
            digital::{self, OutputPin},
            spi::{self, Operation, SpiDevice},
        },
        gpio_client::gpio_v1,
        libdevice_driver::{DriverInit, Turnstile, driver_compatible, driver_init},
        libdevicetree::{
            DeviceTree, INDEX_BUFFER_BYTES, PropReader, gpio_specifier, linked_phandle,
            node_by_phandle, property,
        },
        libmeddle_runtime::{DataArea, IoRec},
        libobject::{
            EventCountKey, KeyTableKey, NotificationKey, ObjectType, RawKey, Rights, UntypedKey,
            export::init_return_key, thread::ThreadReturnKey,
        },
        libserial_trace::{Trace, trace},
        mipidsi::{
            Builder, Display, NoResetPin,
            interface::{SpiError, SpiInterface},
            models::ILI9341Rgb565,
            options::{ColorOrder, Orientation, Rotation},
        },
        panel_ili9341::{ROTATIONS, Rectangle, Screen, rgb565_be},
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
    /// section 18.3.4, `twc`). Every command and its parameters go at this
    /// rate and only pixels at the node's `spi-max-frequency`, as Linux's
    /// MIPI DBI helper does, so a command garbled at an overclocked rate
    /// cannot leave the controller misconfigured. That covers MADCTL
    /// (`rotate`) and the column and page addresses before every flush, not
    /// only the initialization.
    const SETUP_HZ: u32 = 10_000_000;

    /// The longest write sent at [`SETUP_HZ`]: a command byte, or its
    /// parameters (MADCTL has one, CASET and PASET four). A longer write is
    /// pixel data. Linux's MIPI DBI helper draws the line at the same length
    /// (`mipi_dbi_spi_cmd_max_speed`); a few pixels of a tiny rectangle going
    /// slower costs nothing.
    const COMMAND_BYTES: usize = 64;

    /// The rate pixel data asks for: [`SETUP_HZ`] (or less) until the panel is
    /// initialized, then the node's `spi-max-frequency`.
    static WRITE_HZ: AtomicU32 = AtomicU32::new(SETUP_HZ);

    /// The panel's device on the SPI bus: each write goes through
    /// `spi_v1.write` from the shared data area, a data area's worth at a time:
    /// a command's at [`SETUP_HZ`] (or the node's rate, if slower), pixels at
    /// [`WRITE_HZ`].
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
            let pixel_hz = WRITE_HZ.load(Ordering::Relaxed);
            let max_hz = if bytes.len() <= COMMAND_BYTES {
                SETUP_HZ.min(pixel_hz)
            } else {
                pixel_hz
            };
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

    /// The interface mipidsi drives the controller through.
    type Interface = SpiInterface<'static, Bus, Pin>;

    /// What the interface's writes fail with.
    type InterfaceError = SpiError<BusError, PinError>;

    /// The initialized controller: mipidsi's display, whose type changes with
    /// whether the node names a reset line.
    enum Controller {
        WithReset(Display<Interface, ILI9341Rgb565, Pin>),
        WithoutReset(Display<Interface, ILI9341Rgb565, NoResetPin>),
    }

    impl Controller {
        /// Write `colours` to the rectangle from (`x`, `y`) to (`last_x`,
        /// `last_y`), inclusive, row by row.
        fn set_pixels(
            &mut self,
            (x, y): (u16, u16),
            (last_x, last_y): (u16, u16),
            colours: impl IntoIterator<Item = Rgb565>,
        ) -> Result<(), InterfaceError> {
            match self {
                Self::WithReset(display) => display.set_pixels(x, y, last_x, last_y, colours),
                Self::WithoutReset(display) => display.set_pixels(x, y, last_x, last_y, colours),
            }
        }

        /// Turn the controller's scan direction (MADCTL).
        fn set_orientation(&mut self, orientation: Orientation) -> Result<(), InterfaceError> {
            match self {
                Self::WithReset(display) => display.set_orientation(orientation),
                Self::WithoutReset(display) => display.set_orientation(orientation),
            }
        }
    }

    /// The panel as `display_v1` serves it.
    struct Panel {
        controller: Controller,
        /// The screen unturned: the node's `panel-timing`.
        native: Screen,
        width_mm: u32,
        height_mm: u32,
        /// Quarter turns clockwise, as `rotate` last set them.
        quarter_turns: u32,
    }

    impl Panel {
        /// The screen as the panel is turned now.
        const fn screen(&self) -> Screen {
            self.native.turned(self.quarter_turns)
        }
    }

    /// The panel, set once by `main` before the component says it is ready,
    /// then reached only by a caller holding a turn of [`TURNSTILE`].
    struct Shared(UnsafeCell<Option<Panel>>);

    // SAFETY: `main` writes the cell before any call can enter (the turnstile
    // waits for the ready advance), and every call after holds a turn, so no
    // two Threads touch it at once.
    unsafe impl Sync for Shared {}

    static PANEL: Shared = Shared(UnsafeCell::new(None));

    /// Admits one call at a time to the panel and its bus.
    static TURNSTILE: Turnstile = Turnstile::new();

    /// The output's generation: it changes whenever the description or the
    /// mode does (only `rotate` changes anything so far).
    static GENERATION: AtomicU32 = AtomicU32::new(1);

    /// Run `body` on the panel, holding a turn; a panel that failed to
    /// initialize is not present.
    fn with_panel<T>(
        body: impl FnOnce(&mut Panel) -> Result<T, Exception>,
    ) -> Result<T, Exception> {
        let _turn = TURNSTILE.enter(init());
        // SAFETY: this caller holds the turn, and `main` stored the panel (or
        // left it out) before the turnstile let anyone in.
        let panel = unsafe { (*PANEL.0.get()).as_mut() }.ok_or(Exception::NotPresent {
            generation: GENERATION.load(Ordering::Acquire),
        })?;
        body(panel)
    }

    /// `get_mode`'s answer for `screen`.
    const fn mode_of(screen: Screen) -> (u32, u32, u32, Format) {
        (
            screen.width,
            screen.height,
            screen.pitch(),
            Format::Rgb565Be,
        )
    }

    /// How many quarter turns `rotation` is.
    const fn quarter_turns(rotation: display_v1::Rotation) -> u32 {
        match rotation {
            display_v1::Rotation::R0 => 0,
            display_v1::Rotation::R90 => 1,
            display_v1::Rotation::R180 => 2,
            display_v1::Rotation::R270 => 3,
        }
    }

    /// mipidsi's orientation for `quarter_turns`, mirrored: the module's
    /// glass shows columns in reverse, as Linux's ILI9341 driver
    /// (`drivers/gpu/drm/tiny/ili9341.c`, `yx240qv29_enable`) sets it, with
    /// the address mode (MADCTL, 36h) MX upright, MV a quarter turn, MY half a
    /// turn and MV, MY and MX three quarters. mipidsi's mirrored orientations
    /// set those same bits (`MemoryMapping::from_orientation`). Unmirrored,
    /// the picture showed flipped left to right on the board (2026-11-25).
    const fn orientation(quarter_turns: u32) -> Orientation {
        let rotation = match quarter_turns {
            1 => Rotation::Deg90,
            2 => Rotation::Deg180,
            3 => Rotation::Deg270,
            _ => Rotation::Deg0,
        };
        Orientation {
            rotation,
            mirrored: true,
        }
    }

    /// The module's colour filter order: BGR (MADCTL, 36h, bit 3), as Linux's
    /// ILI9341 driver sets it. With RGB, red pixels showed blue on the board
    /// (2026-11-25).
    const COLOR_ORDER: ColorOrder = ColorOrder::Bgr;

    /// Bytes read from the caller's data area at a time: a whole number of
    /// pixels.
    const CHUNK_BYTES: usize = 64;

    /// The pixels of a rectangle's rows in the caller's data area, read a
    /// chunk at a time. A read the area refuses ends the pixels early, which
    /// [`flush`](DisplayV1::flush) rules out beforehand by checking the
    /// reference.
    struct RowPixels<Rows> {
        area: DataArea,
        /// Offset of the screen's first byte in the area.
        screen: u64,
        rows: Rows,
        /// The rest of the current row: its offset in the area and the bytes
        /// left.
        row: (u64, u64),
        chunk: [u8; CHUNK_BYTES],
        at: usize,
        filled: usize,
    }

    impl<Rows: Iterator<Item = (u64, u64)>> Iterator for RowPixels<Rows> {
        type Item = Rgb565;

        fn next(&mut self) -> Option<Rgb565> {
            if self.at == self.filled {
                while self.row.1 == 0 {
                    let (offset, length) = self.rows.next()?;
                    self.row = (self.screen + offset, length);
                }
                let count = usize::try_from(self.row.1).ok()?.min(CHUNK_BYTES);
                if !self.area.read_bytes(self.row.0, &mut self.chunk[..count]) {
                    return None;
                }
                self.row = (self.row.0 + count as u64, self.row.1 - count as u64);
                self.at = 0;
                self.filled = count;
            }
            let pixel = rgb565_be([self.chunk[self.at], self.chunk[self.at + 1]]);
            self.at += 2;
            Some(Rgb565::from(RawU16::new(pixel)))
        }
    }

    /// A coordinate the controller's address window takes.
    fn coordinate(value: u32) -> Result<u16, Exception> {
        u16::try_from(value).map_err(|_too_far| Exception::BadRectangle)
    }

    /// The panel, as one `display_v1` output.
    struct Output;

    impl DisplayV1 for Output {
        fn enter(_badge: u64) {
            init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
                init().return_key,
            )));
        }

        fn describe(_badge: u64) -> Result<(u32, bool, u32, u32, u32), Exception> {
            with_panel(|panel| {
                Ok((
                    GENERATION.load(Ordering::Acquire),
                    false,
                    panel.width_mm,
                    panel.height_mm,
                    ROTATIONS,
                ))
            })
        }

        fn modes(_badge: u64) -> Result<u32, Exception> {
            with_panel(|_panel| Ok(1))
        }

        fn mode(_badge: u64, index: u32) -> Result<(u32, u32, Format, u32), Exception> {
            with_panel(|panel| {
                if index != 0 {
                    return Err(Exception::NoMode { index });
                }
                // The panel does not say its refresh rate.
                Ok((panel.native.width, panel.native.height, Format::Rgb565Be, 0))
            })
        }

        fn get_mode(_badge: u64) -> Result<(u32, u32, u32, Format), Exception> {
            with_panel(|panel| Ok(mode_of(panel.screen())))
        }

        fn set_mode(_badge: u64, index: u32) -> Result<(u32, u32, u32, Format), Exception> {
            with_panel(|panel| {
                if index != 0 {
                    return Err(Exception::NoMode { index });
                }
                // The only mode is the current one: nothing changes.
                Ok(mode_of(panel.screen()))
            })
        }

        fn frames(
            _badge: u64,
            _into: &libmeddle_runtime::KeyTableKey,
            _first_slot: u32,
        ) -> Result<(u32, u32), Exception> {
            // A flush output: its pixels are the caller's data area.
            Err(Exception::Unsupported)
        }

        fn rotate(_badge: u64, rotation: display_v1::Rotation) -> Result<(u32, u32), Exception> {
            let quarter_turns = quarter_turns(rotation);
            with_panel(|panel| {
                panel
                    .controller
                    .set_orientation(orientation(quarter_turns))
                    .map_err(|_error| Exception::Failed)?;
                if panel.quarter_turns != quarter_turns {
                    panel.quarter_turns = quarter_turns;
                    GENERATION.fetch_add(1, Ordering::AcqRel);
                }
                let screen = panel.screen();
                Ok((screen.width, screen.height))
            })
        }

        fn flush(
            badge: u64,
            x: u32,
            y: u32,
            width: u32,
            height: u32,
            screen: IoRec,
        ) -> Result<(), Exception> {
            let (_client, exported) = init()
                .area_for_badge(badge)
                .ok_or(Exception::BadRectangle)?;
            let area = DataArea {
                base: exported.base,
                len: exported.len,
            };
            if !area.contains(&screen) {
                return Err(Exception::BadRectangle);
            }
            let rectangle = Rectangle {
                x,
                y,
                width,
                height,
            };
            with_panel(|panel| {
                let geometry = panel.screen();
                if screen.len < geometry.bytes() || !rectangle.within(geometry) {
                    return Err(Exception::BadRectangle);
                }
                let (last_x, last_y) = rectangle.last();
                let pixels = RowPixels {
                    area,
                    screen: screen.base - area.base,
                    rows: rectangle.rows(geometry),
                    row: (0, 0),
                    chunk: [0; CHUNK_BYTES],
                    at: 0,
                    filled: 0,
                };
                panel
                    .controller
                    .set_pixels(
                        (coordinate(x)?, coordinate(y)?),
                        (coordinate(last_x)?, coordinate(last_y)?),
                        pixels,
                    )
                    .map_err(|_error| Exception::Failed)
            })
        }
    }

    display::display_v1_export!(Output);

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
        let builder = Builder::new(ILI9341Rgb565, interface)
            .display_size(node.width, node.height)
            .orientation(orientation(0))
            .color_order(COLOR_ORDER);
        let started = libuser::clock::now_ns();
        let initialized = match node.reset {
            Some(reset) => builder
                .reset_pin(Pin::output(reset))
                .init(&mut Delay)
                .map(Controller::WithReset)
                .map_err(|error| trace!(trace, "panel: initialization FAILED: {error:?}")),
            None => builder
                .init(&mut Delay)
                .map(Controller::WithoutReset)
                .map_err(|error| trace!(trace, "panel: initialization FAILED: {error:?}")),
        };
        // A panel that failed to initialize is not served: every call raises
        // `not_present`.
        let panel = initialized.ok().map(|mut controller| {
            trace!(
                trace,
                "panel: initialized in {} ms, commands at {} Hz",
                (libuser::clock::now_ns() - started) / 1_000_000,
                LAST_HZ.load(Ordering::Relaxed)
            );
            // From here on pixel data goes at the node's rate; commands stay
            // at the rated one.
            WRITE_HZ.store(node.max_hz, Ordering::Relaxed);
            clear(&mut trace, &mut controller, &node);
            if let Some(mut backlight) = backlight {
                let lit = backlight.set_high().is_ok();
                trace!(trace, "panel: backlight on: {lit}");
            }
            Panel {
                controller,
                native: Screen {
                    width: u32::from(node.width),
                    height: u32::from(node.height),
                },
                width_mm: node.width_mm,
                height_mm: node.height_mm,
                quarter_turns: 0,
            }
        });
        let serving = panel.is_some();
        // SAFETY: no call has entered yet: the turnstile waits for the ready
        // advance below.
        unsafe {
            *PANEL.0.get() = panel;
        }
        let table = KeyTableKey::from_key(RawKey::from_wire(init.table));
        let turns = UntypedKey::from_key(RawKey::from_wire(init.memory))
            .retype(
                ObjectType::EVENT_COUNT,
                0,
                0,
                1,
                &table,
                u32::try_from(init.first_free_slot).unwrap_or(u32::MAX),
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("panel: turnstile Retype failed: {:?}", error.code()));
        TURNSTILE.open(turns);
        EventCountKey::from_key(RawKey::from_wire(init.event_count))
            .advance(1)
            .unwrap_or_else(|error| panic!("panel: ready advance failed: {:?}", error.code()));
        if serving {
            trace!(trace, "panel: serving display_v1");
        } else {
            trace!(trace, "panel: no panel: display_v1 calls raise not_present");
        }
        let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
        loop {
            let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
        }
    }

    /// Fill the screen with black, so nothing the controller held before
    /// shows when the backlight comes on, logging how long a full screen took.
    fn clear(trace: &mut Trace, controller: &mut Controller, node: &Node) {
        let started = libuser::clock::now_ns();
        let pixels = usize::from(node.width) * usize::from(node.height);
        match controller.set_pixels(
            (0, 0),
            (node.width - 1, node.height - 1),
            iter::repeat_n(Rgb565::BLACK, pixels),
        ) {
            Ok(()) => {
                let us = (libuser::clock::now_ns() - started) / 1000;
                let bytes = u64::from(node.width) * u64::from(node.height) * 2;
                trace!(
                    trace,
                    "panel: cleared ({bytes} bytes) in {us} us, {} KiB/s, pixels at {} Hz",
                    bytes * 1_000_000 / us.max(1) / 1024,
                    LAST_HZ.load(Ordering::Relaxed)
                );
            }
            Err(error) => trace!(trace, "panel: clearing FAILED: {error:?}"),
        }
    }
}

#![no_std]
#![no_main]

//! panel-test's program: the ILI9341 panel through `display_v1`, as the
//! compositor will drive it, on a board.
//!
//! It checks what the panel says about itself:
//! - a flush output with all four turns, its size in millimetres;
//! - one mode, 240x320 big-endian RGB565;
//! - the refusals: a mode that does not exist, a rectangle off the screen,
//!   and a screen reference too short for the mode.
//!
//! Then it draws in its data area and flushes, pausing between steps so the
//! panel can be watched:
//! 1. eight colour bars across the screen with a white marker in its top
//!    left corner, timed;
//! 2. a white square moving across a black screen and back, twice, each step
//!    flushing only the strip the square left and entered;
//! 3. the screen turned a quarter (320x240): the bars run across the longer
//!    side and the marker moves with the origin; then turned back.
//!
//! Any check that fails panics: `liblog_client` reports `PANICKED:` on the
//! UART.

use {
    display::display_v1::{Exception, Format, Rotation},
    display_client::display_v1::{Bindings, Client, Error},
    libdevice_driver::driver_init,
    liblog_client::log_println,
    libmeddle_runtime::{DataArea, IoRec},
    libobject::{EventCountKey, RawKey},
};

libuser::entry!(main);

/// How long each step stays on the panel.
const PAUSE_NS: u64 = 2_000_000_000;

/// The colour bars, left to right across the screen as it is turned (so a
/// turn shows: upright they are 8 columns across the 240-pixel side, turned
/// they run across the 320-pixel one), RGB565.
const BARS: [u16; 8] = [
    0xf800, // red
    0x07e0, // green
    0x001f, // blue
    0xffff, // white
    0x0000, // black
    0xffe0, // yellow
    0x07ff, // cyan
    0xf81f, // magenta
];

/// The moving square's side, how far it moves each step, and how long each
/// step stays: a flush of its strip takes about 1.4 ms, so without the wait
/// a pass is over in 35 ms and only its end is seen.
const SQUARE: u32 = 40;
const STEP: u32 = 8;
const STEP_PAUSE_NS: u64 = 40_000_000;

/// The side of the marker in the screen's top left corner (as turned): white
/// on the red first bar, so a turn shows where the origin went.
const MARKER: u32 = 32;

/// The screen as the panel is turned: its size and the bytes of a row.
#[derive(Clone, Copy)]
struct Screen {
    width: u32,
    height: u32,
    pitch: u32,
}

impl Screen {
    fn bytes(self) -> u64 {
        u64::from(self.pitch) * u64::from(self.height)
    }
}

/// The bytes of one row, the longest a turn makes: 320 pixels.
const ROW_BYTES: usize = 320 * 2;

/// Fill the rectangle of `screen` in `area` with `colour` for each pixel
/// `paint(x, y)` names, a row at a time.
fn paint(
    area: &DataArea,
    screen: Screen,
    (x, y, width, height): (u32, u32, u32, u32),
    colour: impl Fn(u32, u32) -> u16,
) {
    let mut row = [0_u8; ROW_BYTES];
    for line in y..y + height {
        for column in x..x + width {
            let at = usize::try_from((column - x) * 2).unwrap_or(0);
            row[at..at + 2].copy_from_slice(&colour(column, line).to_be_bytes());
        }
        let offset = u64::from(line) * u64::from(screen.pitch) + u64::from(x) * 2;
        let length = usize::try_from(width * 2).unwrap_or(0);
        assert!(
            area.write_bytes(offset, &row[..length]),
            "panel-probe: the data area holds the screen"
        );
    }
}

/// The colour under (`x`, `y`): the [`BARS`] across the screen, the
/// [`MARKER`] in its top left corner.
fn bar(screen: Screen, x: u32, y: u32) -> u16 {
    if x < MARKER && y < MARKER {
        return 0xffff;
    }
    BARS[usize::try_from(x * 8 / screen.width).unwrap_or(0).min(7)]
}

/// Sleep for `ns` on the component's own `EventCount`, which nobody advances.
fn pause(event_count: &EventCountKey, ns: u64) {
    let _slept = event_count.sleep(ns);
}

/// The `display_v1` exception `result` raised, if it raised one.
fn raised<T>(result: Result<T, Error>) -> Option<Exception> {
    match result.err()? {
        Error::Exception(exception) => Some(exception),
        _ => None,
    }
}

/// Flush the whole of `screen` and return how long it took, in microseconds.
fn flush_all(panel: &Client, screen: Screen, reference: IoRec) -> u64 {
    let started = libuser::clock::now_ns();
    panel
        .flush(0, 0, screen.width, screen.height, reference)
        .unwrap_or_else(|error| panic!("panel-probe: full flush failed: {error:?}"));
    (libuser::clock::now_ns() - started) / 1000
}

/// The current mode, as a screen; checks the format.
fn screen_of(panel: &Client) -> Screen {
    let (width, height, pitch, format) = panel
        .get_mode()
        .unwrap_or_else(|error| panic!("panel-probe: get_mode failed: {error:?}"));
    assert_eq!(format, Format::Rgb565Be, "panel-probe: the format");
    assert_eq!(pitch, width * 2, "panel-probe: rows are packed");
    Screen {
        width,
        height,
        pitch,
    }
}

fn main(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    let event_count = EventCountKey::from_key(RawKey::from_wire(init.event_count));
    let panel = Client::new(Bindings::from_imports(init.imports()));
    let area = panel
        .data_area()
        .expect("panel-probe: display_v1 has a data area");

    let (generation, scanout, width_mm, height_mm, rotations) = panel
        .describe()
        .unwrap_or_else(|error| panic!("panel-probe: describe failed: {error:?}"));
    log_println!(
        "panel-probe: generation {generation}, {} output, {width_mm}x{height_mm} mm, rotations {rotations:#06b}, data area {} bytes",
        if scanout { "a scanout" } else { "a flush" },
        area.len
    );
    assert!(!scanout, "panel-probe: the panel needs flushing");
    assert_eq!(rotations, 0b1111, "panel-probe: the panel turns itself");

    assert_eq!(panel.modes(), Ok(1), "panel-probe: one mode");
    assert_eq!(
        panel.mode(0),
        Ok((240, 320, Format::Rgb565Be, 0)),
        "panel-probe: the mode"
    );
    assert_eq!(
        raised(panel.mode(1)),
        Some(Exception::NoMode { index: 1 }),
        "panel-probe: no second mode"
    );
    assert_eq!(
        panel.set_mode(0),
        Ok((240, 320, 480, Format::Rgb565Be)),
        "panel-probe: setting the only mode"
    );
    let screen = screen_of(&panel);
    assert!(
        screen.bytes() <= area.len,
        "panel-probe: the screen fits the area"
    );
    let whole = area
        .reference(0, screen.bytes())
        .expect("panel-probe: the screen's reference");

    // The refusals: off the screen, empty, and a reference too short.
    assert_eq!(
        raised(panel.flush(0, 0, screen.width + 1, 1, whole)),
        Some(Exception::BadRectangle),
        "panel-probe: a rectangle wider than the screen"
    );
    assert_eq!(
        raised(panel.flush(0, 0, 0, 1, whole)),
        Some(Exception::BadRectangle),
        "panel-probe: an empty rectangle"
    );
    let short = area
        .reference(0, screen.bytes() - 2)
        .expect("panel-probe: a short reference");
    assert_eq!(
        raised(panel.flush(0, 0, 1, 1, short)),
        Some(Exception::BadRectangle),
        "panel-probe: a screen reference too short for the mode"
    );
    log_println!("panel-probe: description, modes and refusals as expected");

    // 1. The colour bars over the whole screen.
    paint(
        &area,
        screen,
        (0, 0, screen.width, screen.height),
        |x, y| bar(screen, x, y),
    );
    let us = flush_all(&panel, screen, whole);
    log_println!(
        "panel-probe: colour bars, full screen ({} bytes) in {us} us, {} KiB/s",
        screen.bytes(),
        screen.bytes() * 1_000_000 / us.max(1) / 1024
    );
    pause(&event_count, PAUSE_NS);

    // 2. A white square moving across a black screen, there and back twice:
    // each step flushes only the strip the square left and entered.
    let top = (screen.height - SQUARE) / 2;
    paint(
        &area,
        screen,
        (0, 0, screen.width, screen.height),
        |x, y| {
            if x < SQUARE && (top..top + SQUARE).contains(&y) {
                0xffff
            } else {
                0
            }
        },
    );
    flush_all(&panel, screen, whole);
    let far = screen.width - SQUARE;
    let mut flushing_ns = 0;
    let mut steps = 0_u64;
    let mut left = 0;
    for target in [far, 0, far, 0] {
        while left != target {
            let next = if target > left {
                (left + STEP).min(target)
            } else {
                left.saturating_sub(STEP).max(target)
            };
            // The strip the square covers before and after the step.
            let strip = left.min(next);
            let strip_width = SQUARE + left.abs_diff(next);
            paint(&area, screen, (strip, top, strip_width, SQUARE), |x, _| {
                if (next..next + SQUARE).contains(&x) {
                    0xffff
                } else {
                    0
                }
            });
            let started = libuser::clock::now_ns();
            panel
                .flush(strip, top, strip_width, SQUARE, whole)
                .unwrap_or_else(|error| panic!("panel-probe: square flush failed: {error:?}"));
            flushing_ns += libuser::clock::now_ns() - started;
            left = next;
            steps += 1;
            pause(&event_count, STEP_PAUSE_NS);
        }
    }
    log_println!(
        "panel-probe: moving square, {steps} partial flushes of {}x{SQUARE}, {} us each",
        SQUARE + STEP,
        flushing_ns / 1000 / steps.max(1)
    );
    pause(&event_count, PAUSE_NS);

    // 3. Turned a quarter clockwise: the bars run across the 320-pixel side,
    // and the marker moves to the panel's top right corner (as it is held
    // upright).
    let (width, height) = panel
        .rotate(Rotation::R90)
        .unwrap_or_else(|error| panic!("panel-probe: rotate failed: {error:?}"));
    assert_eq!((width, height), (320, 240), "panel-probe: turned size");
    let (turned_generation, ..) = panel
        .describe()
        .unwrap_or_else(|error| panic!("panel-probe: describe failed: {error:?}"));
    assert_ne!(
        turned_generation, generation,
        "panel-probe: a turn advances the generation"
    );
    let turned = screen_of(&panel);
    assert_eq!(turned.pitch, 640, "panel-probe: turned rows");
    let turned_whole = area
        .reference(0, turned.bytes())
        .expect("panel-probe: the turned screen's reference");
    paint(
        &area,
        turned,
        (0, 0, turned.width, turned.height),
        |x, y| bar(turned, x, y),
    );
    let us = flush_all(&panel, turned, turned_whole);
    log_println!("panel-probe: turned 90 degrees, {width}x{height}, bars across in {us} us");
    pause(&event_count, PAUSE_NS);

    // Back upright: bars across the 240-pixel side, marker top left again.
    assert_eq!(panel.rotate(Rotation::R0), Ok((240, 320)));
    paint(
        &area,
        screen,
        (0, 0, screen.width, screen.height),
        |x, y| bar(screen, x, y),
    );
    flush_all(&panel, screen, whole);
    log_println!("panel-probe: ✅ display_v1 on the panel works");
    loop {
        pause(&event_count, u64::MAX);
    }
}

#![no_std]
#![no_main]

//! display-test's program: the fb component through `display_v1`.
//!
//! It logs every mode the output names (on a board, the sizes of the HDMI
//! display's EDID, with its millimetres; under QEMU, which gives no EDID, the
//! one fallback mode) through the log server, which writes the lines on the
//! UART (and, under QEMU, on semihosting too). It checks what a scanout
//! output says about itself and what it refuses:
//! - a scanout output, upright only, already in a mode when it is first asked
//!   (the fb component configures its first mode at start);
//! - at least one mode; the current one is mode 0 in size and format, with
//!   rows of at least 4 bytes per pixel; there is no mode past the last;
//! - it turns only upright, `flush` returns at once, and setting the current
//!   mode again changes nothing, not even the generation.
//!
//! Then it reports on the key display-test installed. Any failure panics,
//! which ends the QEMU run and logs `PANICKED:` on a board.

use {
    display::display_v1::{Exception, Format, Rotation},
    display_client::display_v1::{Bindings, Client, Error},
    driver_protocol::report_key,
    libdevice_driver::driver_init,
    liblog_client::log_println,
    libobject::{NotificationKey, RawKey},
};

libuser::entry!(main);

/// The `display_v1` exception `result` raised, if it raised one.
fn raised<T>(result: Result<T, Error>) -> Option<Exception> {
    match result.err()? {
        Error::Exception(exception) => Some(exception),
        _ => None,
    }
}

fn main(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    let display = Client::new(Bindings::from_imports(init.imports()));
    // The first call waits for the fb component to finish its start (its
    // EDID and first mode): a line before it shows when that never ends.
    log_println!("display-probe: asking the fb component what it is");

    let (generation, scanout, width_mm, height_mm, rotations) = display
        .describe()
        .unwrap_or_else(|error| panic!("describe failed: {error:?}"));
    assert!(scanout, "the fb component shows its memory by itself");
    assert_eq!(rotations, 0b0001, "upright only");

    let count = display
        .modes()
        .unwrap_or_else(|error| panic!("modes failed: {error:?}"));
    assert!(count >= 1, "at least one mode");
    let (width, height, pitch, format) = display
        .get_mode()
        .unwrap_or_else(|error| panic!("get_mode failed: {error:?}"));
    let (first_width, first_height, first_format, _first_refresh_mhz) = display
        .mode(0)
        .unwrap_or_else(|error| panic!("mode 0 failed: {error:?}"));
    assert_eq!(
        (width, height, format),
        (first_width, first_height, first_format),
        "the mode configured at start is the first, preferred one"
    );
    assert!(
        matches!(format, Format::Xrgb8888 | Format::Xbgr8888),
        "32-bit pixels, not {format:?}"
    );
    assert!(pitch >= width * 4, "a row holds its pixels");
    assert_eq!(
        raised(display.mode(count)),
        Some(Exception::NoMode { index: count }),
        "no mode past the last"
    );
    log_println!(
        "display-probe: generation {generation}, {width_mm}x{height_mm} mm, {count} modes; configured {width}x{height}, pitch {pitch}, {format:?}"
    );
    for index in 0..count {
        let (mode_width, mode_height, _format, mode_refresh_mhz) = display
            .mode(index)
            .unwrap_or_else(|error| panic!("mode {index} failed: {error:?}"));
        log_println!(
            "display-probe:   mode {index}: {mode_width}x{mode_height} at {}.{:03} Hz",
            mode_refresh_mhz / 1000,
            mode_refresh_mhz % 1000
        );
    }

    assert_eq!(display.rotate(Rotation::R0), Ok((width, height)));
    for rotation in [Rotation::R90, Rotation::R180, Rotation::R270] {
        assert_eq!(
            raised(display.rotate(rotation)),
            Some(Exception::Unsupported),
            "{rotation:?}"
        );
    }
    let area = display.data_area().expect("display_v1 has a data area");
    let screen = area.reference(0, 0).expect("an empty screen reference");
    assert_eq!(
        display.flush(0, 0, width, height, screen),
        Ok(()),
        "a scanout output's flush returns at once"
    );
    assert_eq!(
        display.set_mode(0),
        Ok((width, height, pitch, format)),
        "the current mode again"
    );
    let (unchanged, ..) = display
        .describe()
        .unwrap_or_else(|error| panic!("describe failed: {error:?}"));
    assert_eq!(unchanged, generation, "nothing changed");
    log_println!("display-probe: ✅ rotations, flush and set_mode as expected");

    NotificationKey::from_key(report_key(init.table))
        .signal(0)
        .unwrap_or_else(|error| panic!("report failed: {:?}", error.code()));
    let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
    }
}

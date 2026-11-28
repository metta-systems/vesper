//! The Threads that draw: the frame Thread for the scanout outputs and a
//! presenter Thread for each flush output (MULTI-DISPLAY-8), each at most once
//! every frame time.
//!
//! Each sleeps until its outputs have damage, then draws and waits one frame
//! time (a timer that runs only while there is damage). Drawing an output
//! composes each damaged rectangle in the output's back buffer, a cacheable
//! region of RAM the compositor owns, in the output's own coordinates, and
//! converts the finished rectangles to the output's format:
//! - into a scanout output's write-combining memory, which then shows;
//! - into a flush output's data area, after which the presenter calls
//!   `flush` for each rectangle and waits for it, so only the presenter ever
//!   waits for the glass.
//!
//! Nothing is ever read back from an output on the drawing path, and each back
//! buffer is drawn by one Thread only.

use {
    crate::shared::{self, Display, Memory, init, wake_key},
    core::sync::atomic::{AtomicU32, AtomicU64, Ordering},
    display::display_v1::Exception as DisplayException,
    display_client::display_v1::{
        Bindings as DisplayBindings, Client as DisplayClient, Error as DisplayError,
    },
    libcompositor_core::{
        blit::{Canvas, Format, Layer, compose, present_turned},
        damage::Damage,
        layout::{MAX_OUTPUTS, native_rect},
        windows::Rect,
    },
    libdevice_driver::SURFACE_SLOTS,
    libgraphics::{Color, PixelOrder},
    libmeddle_runtime::DataArea,
    libobject::EventCountKey,
};

/// Time between frames while an output has damage.
const FRAME_NS: u64 = 33_000_000;

/// The desktop's color.
const BACKGROUND_COLOR: Color = Color::rgb(24, 40, 72);

/// The background as a pixel: opaque.
#[must_use]
pub fn background(order: PixelOrder) -> u32 {
    0xff00_0000 | BACKGROUND_COLOR.encode(order)
}

libdevice_driver::driver_threads!(frame_main, presenter_0, presenter_1);

// One presenter Thread for each output index.
const _: () = assert!(MAX_OUTPUTS == 2);

/// One window to draw, copied out of the state: its rectangle in the output's
/// coordinates and its surface, as the compositor mapped it.
#[derive(Clone, Copy)]
struct LayerInfo {
    rect: Rect,
    base: usize,
    pixels: usize,
    pitch: usize,
}

/// What drawing one output takes, copied out of the state so that nothing is
/// locked while drawing.
struct Snapshot {
    display: Display,
    /// The damage, in the output's coordinates.
    damage: Damage,
    layers: [Option<LayerInfo>; SURFACE_SLOTS],
    layer_count: usize,
    /// The pointer, in the output's coordinates.
    cursor: (i32, i32),
    canvas: Format,
    background: u32,
}

/// Take output `index`'s damage and what is needed to draw it, if it has
/// damage.
fn take(index: usize) -> Option<Snapshot> {
    let mut turn = shared::enter();
    let state = shared::state(&mut turn);
    let display = state.displays.get(index).copied().flatten()?;
    let layout = *state.router.layout();
    let viewport = layout.viewport(index)?;
    let held = state.damage.get_mut(index)?;
    if held.is_empty() {
        return None;
    }
    let mut damage = Damage::new();
    for rect in held.rects() {
        if let Some(local) = layout.local(index, rect) {
            damage.add(local);
        }
    }
    held.clear();
    let mut layers = [None; SURFACE_SLOTS];
    let mut layer_count = 0;
    for window in state.router.windows().bottom_up() {
        let (Some(surface), Some(rect)) = (
            state.surface_of(window.id),
            layout.to_local(index, window.rect),
        ) else {
            continue;
        };
        if let Some(slot) = layers.get_mut(layer_count) {
            *slot = Some(LayerInfo {
                rect,
                base: surface.base,
                pixels: surface.pitch_pixels() * surface.height as usize,
                pitch: surface.pitch_pixels(),
            });
            layer_count += 1;
        }
    }
    let (x, y) = state.router.pointer();
    Some(Snapshot {
        display,
        damage,
        layers,
        layer_count,
        cursor: (x.saturating_sub(viewport.x), y.saturating_sub(viewport.y)),
        canvas: state.canvas,
        background: background(state.order()),
    })
}

/// Compose the damage of `snapshot` in its output's back buffer and convert
/// it into the output's memory.
fn draw(snapshot: &Snapshot) {
    let display = snapshot.display;
    let pixel_count = display.width as usize * display.height as usize;
    // SAFETY: the back buffer is the compositor's own memory, mapped before
    // the compositor said it was ready, `width * height` pixels, and only the
    // Thread that draws this output uses it.
    let pixels = unsafe {
        core::slice::from_raw_parts_mut(
            core::ptr::with_exposed_provenance_mut::<u32>(
                usize::try_from(display.back_buffer).unwrap_or(0),
            ),
            pixel_count,
        )
    };
    let mut canvas = Canvas {
        pixels,
        pitch: display.width as usize,
        width: display.width,
        height: display.height,
    };
    let layers: [Layer<'_>; SURFACE_SLOTS] = core::array::from_fn(|index| {
        let info = snapshot.layers[index].filter(|_| index < snapshot.layer_count);
        let Some(info) = info else {
            return Layer {
                rect: Rect {
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                },
                pixels: &[],
                pitch: 0,
            };
        };
        // SAFETY: the surface stays mapped in this `AddressSpace` for good
        // (surfaces are never recycled). The client writes it while it is
        // read here; a row read while it changes is drawn again after the
        // client's next commit of it.
        let pixels = unsafe {
            core::slice::from_raw_parts(
                core::ptr::with_exposed_provenance::<u32>(info.base),
                info.pixels,
            )
        };
        Layer {
            rect: info.rect,
            pixels,
            pitch: info.pitch,
        }
    });
    let (output, pitch) = output_bytes(&display);
    for rect in snapshot.damage.rects() {
        compose(
            &mut canvas,
            rect,
            snapshot.background,
            &layers[..snapshot.layer_count],
            Some(snapshot.cursor),
        );
        // An output that cannot turn itself stands upright: the compositor
        // turns the pixels on their way (MULTI-DISPLAY-5).
        present_turned(
            &canvas,
            snapshot.canvas,
            output,
            pitch,
            display.format,
            rect,
            display.software_turns,
            display.native,
        );
    }
}

/// The memory `display`'s pixels go to, and its pitch in bytes.
fn output_bytes(display: &Display) -> (&'static mut [u8], usize) {
    match display.memory {
        // SAFETY: only the frame Thread draws a scanout output.
        Memory::Scanout(framebuffer) => (unsafe { framebuffer.bytes() }, framebuffer.pitch_bytes()),
        Memory::Flush { base, len, pitch } => {
            let bytes = (pitch * display.height as usize).min(usize::try_from(len).unwrap_or(0));
            // SAFETY: Kickstart mapped the data area read/write at `base` for
            // `len` bytes, for the compositor's lifetime; only this output's
            // presenter writes it, and the output reads it only during the
            // presenter's `flush` call.
            let slice = unsafe {
                core::slice::from_raw_parts_mut(
                    core::ptr::with_exposed_provenance_mut::<u8>(
                        usize::try_from(base).unwrap_or(0),
                    ),
                    bytes,
                )
            };
            (slice, pitch)
        }
    }
}

/// Wait for the first advance of the wake `EventCount`: the compositor is set
/// up. Returns the count seen.
fn await_ready() -> u64 {
    wake_key()
        .await_ge(1, EventCountKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("compositor: ready wait failed: {:?}", error.code()))
}

/// Sleep until something is damaged: the wake count passes `seen`.
fn await_damage(seen: u64) -> u64 {
    wake_key()
        .await_ge(seen + 1, EventCountKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("compositor: wake wait failed: {:?}", error.code()))
}

/// Wait one frame time.
fn frame_time() {
    // The count never reaches this target: the wait is the frame timer.
    let _timed_out = wake_key().await_ge(u64::MAX, FRAME_NS);
}

/// The frame Thread's entry: it draws the scanout outputs.
pub fn frame_main(_init_va: u64) -> ! {
    let mut seen = await_ready();
    let scanout: [bool; MAX_OUTPUTS] = {
        let mut turn = shared::enter();
        let state = shared::state(&mut turn);
        core::array::from_fn(|index| {
            state.displays[index]
                .is_some_and(|display| matches!(display.memory, Memory::Scanout(_)))
        })
    };
    #[cfg(feature = "qemu")]
    let mut frames = 0_u32;
    loop {
        let mut drew = false;
        for index in (0..MAX_OUTPUTS).filter(|&index| scanout[index]) {
            let Some(snapshot) = take(index) else {
                continue;
            };
            draw(&snapshot);
            drew = true;
            #[cfg(feature = "qemu")]
            if index == 0 {
                frames += 1;
                report(&snapshot, frames);
            }
        }
        if drew {
            frame_time();
        } else {
            seen = await_damage(seen);
        }
    }
}

/// The presenter Thread of output 0, if it is a flush output.
fn presenter_0(_init_va: u64) -> ! {
    presenter(0)
}

/// The presenter Thread of output 1, if it is a flush output.
fn presenter_1(_init_va: u64) -> ! {
    presenter(1)
}

/// Draw flush output `index` and send its damage, one batch at a time: new
/// damage waits until the previous `flush` calls returned (MULTI-DISPLAY-8). A
/// Thread whose output is not a flush output parks for good.
fn presenter(index: usize) -> ! {
    let mut seen = await_ready();
    let display = {
        let mut turn = shared::enter();
        shared::state(&mut turn).displays[index]
    };
    let Some(Display {
        instance,
        memory: Memory::Flush { base, len, .. },
        ..
    }) = display
    else {
        loop {
            let _parked = wake_key().await_ge(u64::MAX, EventCountKey::WAIT_INFINITE);
        }
    };
    let client = DisplayClient::new(DisplayBindings::from_imports_nth(
        init().imports(),
        instance,
    ));
    let area = DataArea { base, len };
    loop {
        let Some(snapshot) = take(index) else {
            seen = await_damage(seen);
            continue;
        };
        draw(&snapshot);
        let Some(screen) = area.reference(0, len) else {
            continue;
        };
        for rect in snapshot.damage.rects() {
            // The output's own rectangle, where the compositor turns it.
            let sent = native_rect(
                *rect,
                snapshot.display.software_turns,
                snapshot.display.native,
            );
            let (Ok(x), Ok(y)) = (u32::try_from(sent.x), u32::try_from(sent.y)) else {
                continue;
            };
            if let Err(error) = client.flush(x, y, sent.width, sent.height, screen) {
                // The glass is as it was there: count the failure for the
                // input Thread to report, and send the rectangle again with
                // the next batch.
                FLUSH_FAILURES.fetch_add(1, Ordering::AcqRel);
                LAST_FLUSH_FAILURE.store(failure_code(&error), Ordering::Release);
                requeue(index, rect);
            }
        }
        frame_time();
    }
}

/// Flushes that failed so far, and the last failure ([`failure_code`]).
static FLUSH_FAILURES: AtomicU32 = AtomicU32::new(0);
static LAST_FLUSH_FAILURE: AtomicU64 = AtomicU64::new(0);

/// How many flushes failed so far, and how the last one failed: the kind in
/// the high word (1 `not_present` with the generation below it, 2
/// `bad_rectangle`, 3 `failed`, 4 another exception, 5 the call itself with
/// its status below it, 6 not granted or anything else).
#[must_use]
pub fn flush_failures() -> (u32, u64) {
    (
        FLUSH_FAILURES.load(Ordering::Acquire),
        LAST_FLUSH_FAILURE.load(Ordering::Acquire),
    )
}

/// The code [`flush_failures`] reports for `error`.
fn failure_code(error: &DisplayError) -> u64 {
    match error {
        DisplayError::Exception(DisplayException::NotPresent { generation }) => {
            1 << 32 | u64::from(*generation)
        }
        DisplayError::Exception(DisplayException::BadRectangle) => 2 << 32,
        DisplayError::Exception(DisplayException::Failed) => 3 << 32,
        DisplayError::Exception(_) => 4 << 32,
        DisplayError::Call { status, .. } => 5 << 32 | (status & 0xffff_ffff),
        _ => 6 << 32,
    }
}

/// Put `rect` of output `index`, in the output's coordinates, back into its
/// damage.
fn requeue(index: usize, rect: &Rect) {
    let mut turn = shared::enter();
    let state = shared::state(&mut turn);
    let Some(viewport) = state.router.layout().viewport(index) else {
        return;
    };
    if let Some(damage) = state.damage.get_mut(index) {
        damage.add(Rect {
            x: rect.x.saturating_add(viewport.x),
            y: rect.y.saturating_add(viewport.y),
            ..*rect
        });
    }
}

/// Say what the first frames drew on output 0, for the test that boots the
/// compositor.
#[cfg(feature = "qemu")]
fn report(snapshot: &Snapshot, frames: u32) {
    use libuser::semihosting as semi;
    let Memory::Scanout(framebuffer) = snapshot.display.memory else {
        return;
    };
    let pitch_pixels = framebuffer.pitch_bytes() / 4;
    if frames == 1 {
        let corner = framebuffer.read(5 * pitch_pixels + 5);
        semi::println!(
            "compositor: frame 1 drew the background: {}",
            corner == snapshot.background
        );
    }
    let Some(layer) = snapshot.layers[..snapshot.layer_count]
        .iter()
        .flatten()
        .next()
    else {
        return;
    };
    // Sample the framebuffer inside the first window: the client's pixels
    // must have reached the screen.
    let mut lit = 0_u32;
    let rect = layer.rect;
    for row in (0..rect.height).step_by(8) {
        for column in (0..rect.width).step_by(8) {
            let x = rect.x.saturating_add_unsigned(column);
            let y = rect.y.saturating_add_unsigned(row);
            let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else {
                continue;
            };
            if x >= framebuffer.width() as usize {
                continue;
            }
            if framebuffer.read(y * pitch_pixels + x) != snapshot.background {
                lit += 1;
            }
        }
    }
    if frames.is_power_of_two() || lit > 0 && frames.is_multiple_of(32) {
        semi::println!(
            "compositor: frame {frames}: {} windows, {lit} sampled pixels differ from the background",
            snapshot.layer_count
        );
    }
}

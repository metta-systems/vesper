//! The shell's display: a window of the compositor, the static scene `draw`
//! paints in it, and the background Thread that keeps animating lava blobs
//! beside it.
//!
//! `draw` runs on the shell's first Thread. The first time, it asks the
//! compositor for a window (`compositor_v1.create_window`), lending a table for
//! the Frames of the window's surface, and maps them in the surface window
//! Kickstart provisioned (`SURFACE_WINDOW_VA`). Every time, it paints the static
//! scene in the surface (some text and shapes) and tells the compositor what
//! changed. After the first, it releases the extra Thread (`driver_threads!`),
//! which waits on the component's `EventCount` and then draws the lava lamp
//! continuously, a frame every few tens of milliseconds, while the prompt stays
//! available.

use {
    compositor_client::compositor_v1::{
        Bindings as CompositorBindings, Client as CompositorClient, Error as CompositorError,
        PixelOrder as WireOrder,
    },
    core::sync::atomic::{AtomicU64, Ordering},
    libcompositor_core::surface::{FRAME_BYTES, frames, pitch_bytes},
    libdevice_driver::{DriverInit, SURFACE_WINDOW_VA, driver_init, local_key},
    libgraphics::{Color, Font, Lava, Painter, PixelOrder, Surface},
    libobject::{
        EventCountKey, FrameAttribute, FrameKey, KeySlot, KeyTableKey, NotificationKey, ObjectType,
        RawKey, Rights, UntypedKey,
    },
    shell::Failure,
};

/// The size of the window `draw` asks for. A surface is whole 2 MiB Frames and
/// this one fits in one.
const WIDTH: u32 = 1000;
const HEIGHT: u32 = 500;

/// The window's title.
const TITLE: &str = "shell";

/// Size exponent of the table the Frames are lent into, and its guard.
const FRAME_TABLE_BITS: u8 = 4;
const FRAME_TABLE_GUARD: u32 = 0x7_AB1E;

/// The lava lamp's place in the window, right of the static scene, and its
/// resolution: cells of `LAMP_CELL` pixels.
const LAMP_X: u32 = 300;
const LAMP_Y: u32 = 10;
const LAMP_CELL: u32 = 4;
const LAMP_COLUMNS: u32 = 170;
const LAMP_ROWS: u32 = 120;
/// Time between lamp frames.
const FRAME_NS: u64 = 40_000_000;

/// `Failure::status` of a display step that failed; `detail` is the step in
/// the high half and what the call answered in the low half.
const DISPLAY_FAILED: u64 = 0x4000;
const ORDER: u64 = 1;
const TABLE: u64 = 2;
const WINDOW: u64 = 3;
const MAPPING: u64 = 4;
const COMMIT: u64 = 5;
const FONT: u64 = 6;
const READ_BACK: u64 = 7;

/// What the first Thread publishes for the drawing Thread: the window's id and
/// whether the pixel order is RGB. Written before the `EventCount` advance that
/// announces them. The surface is mapped at `SURFACE_WINDOW_VA`.
static WINDOW_ID: AtomicU64 = AtomicU64::new(0);
static RGB: AtomicU64 = AtomicU64::new(0);

/// Whether the window exists already.
static CREATED: AtomicU64 = AtomicU64::new(0);

fn failure(step: u64, answer: u64) -> Failure {
    Failure {
        status: DISPLAY_FAILED,
        detail: step << 32 | answer & 0xffff_ffff,
    }
}

fn compositor_failure(step: u64, error: &CompositorError) -> Failure {
    failure(
        step,
        match error {
            CompositorError::Call { status, .. } => *status,
            CompositorError::Exception(_) => 0xffff,
            _ => 0xfffe,
        },
    )
}

fn capability_failure(step: u64, error: libobject::CapError) -> Failure {
    let (status, _, _) = error.code();
    failure(step, status)
}

/// Paint the static scene, creating the window and releasing the lamp Thread
/// on the first call.
pub fn draw(init: &DriverInit, compositor: &CompositorClient) -> Result<(), Failure> {
    let first = CREATED.load(Ordering::Acquire) == 0;
    if first {
        create_window(init, compositor)?;
        CREATED.store(1, Ordering::Release);
    }
    let font = init
        .resource(0)
        .and_then(Font::new)
        .ok_or_else(|| failure(FONT, 0))?;
    let mut screen = mapped_screen();
    paint(&mut screen, order(), &font);
    // The box's corner reads back as painted: the mapping is the surface.
    let painted = BOX.encode(order());
    if screen.get(20, 20) != painted {
        return Err(failure(READ_BACK, u64::from(screen.get(20, 20))));
    }
    compositor
        .commit(window_id(), 0, 0, WIDTH, HEIGHT)
        .map_err(|error| compositor_failure(COMMIT, &error))?;
    if first {
        EventCountKey::from_key(RawKey::from_wire(init.event_count))
            .advance(1)
            .map_err(|error| capability_failure(WINDOW, error))?;
    }
    Ok(())
}

/// Ask the compositor for a window, receive the Frames of its surface and map
/// them; publish the window.
fn create_window(init: &DriverInit, compositor: &CompositorClient) -> Result<(), Failure> {
    let order = compositor
        .pixel_order()
        .map_err(|error| compositor_failure(ORDER, &error))?;

    let own = KeyTableKey::from_key(RawKey::from_wire(init.table));
    let mut own_self =
        KeyTableKey::from_key(local_key(init.table, libobject::KeySlot::SELF_KEYTABLE.0));
    let table_slot = u32::try_from(init.first_free_slot).unwrap_or(u32::MAX);
    let copy_slot = table_slot + 1;
    let table = KeyTableKey::from_key(
        UntypedKey::from_key(RawKey::from_wire(init.memory))
            .retype(
                ObjectType::KEY_TABLE,
                FRAME_TABLE_BITS,
                FRAME_TABLE_GUARD,
                1,
                &own,
                table_slot,
                Rights::all(),
            )
            .map_err(|error| capability_failure(TABLE, error))?,
    );
    let window = compositor
        .create_window(&table, WIDTH, HEIGHT, 0)
        .map_err(|error| compositor_failure(WINDOW, &error))?;
    // A title is a courtesy: the window works without one.
    if let Some(area) = compositor.data_area()
        && area.write_bytes(0, TITLE.as_bytes())
        && let Some(stage) = area.reference(0, TITLE.len() as u64)
    {
        let _titled = compositor.set_title(window, stage);
    }

    // The Frames are in the table in address order, one for each 2 MiB.
    let count = frames(WIDTH, HEIGHT).ok_or_else(|| failure(WINDOW, 0))?;
    let address_space = RawKey::from_wire(init.address_space);
    for index in 0..count {
        let source = RawKey::new(KeySlot((FRAME_TABLE_GUARD << FRAME_TABLE_BITS) | index), 1);
        let copy = table
            .copy_derive(
                source,
                &own,
                copy_slot,
                Rights(Rights::READ | Rights::WRITE | Rights::GRANT),
            )
            .map_err(|error| capability_failure(MAPPING, error))?;
        FrameKey::from_key(copy)
            .map(
                address_space,
                SURFACE_WINDOW_VA + u64::from(index) * FRAME_BYTES,
                Rights(Rights::READ | Rights::WRITE),
                FrameAttribute::Default,
            )
            .map_err(|error| capability_failure(MAPPING, error))?;
        // The mapping stays; the lent-into table keeps the Frame itself.
        own_self
            .delete(copy)
            .map_err(|error| capability_failure(MAPPING, error))?;
    }

    WINDOW_ID.store(u64::from(window), Ordering::Relaxed);
    RGB.store(u64::from(order == WireOrder::Rgb), Ordering::Relaxed);
    Ok(())
}

fn window_id() -> u32 {
    u32::try_from(WINDOW_ID.load(Ordering::Relaxed)).unwrap_or(0)
}

/// The window's surface as a drawing surface. Its pixels are premultiplied ARGB,
/// so every pixel stored is opaque.
struct Screen {
    base: *mut u32,
    pitch_pixels: u32,
}

impl Surface for Screen {
    fn width(&self) -> u32 {
        WIDTH
    }

    fn height(&self) -> u32 {
        HEIGHT
    }

    fn put(&mut self, x: u32, y: u32, pixel: u32) {
        let index = y as usize * self.pitch_pixels as usize + x as usize;
        // SAFETY: the surface lies inside the mapped Frames: the painter keeps
        // (x, y) inside WIDTH by HEIGHT, which the window was made with.
        unsafe {
            self.base.add(index).write_volatile(pixel | OPAQUE);
        }
    }
}

/// The alpha byte of a pixel that shows fully.
const OPAQUE: u32 = 0xff00_0000;

impl Screen {
    /// The color channels of the pixel at (`x`, `y`).
    fn get(&self, x: u32, y: u32) -> u32 {
        let index = y as usize * self.pitch_pixels as usize + x as usize;
        // SAFETY: inside the mapped surface, as for `put`.
        unsafe { self.base.add(index).read_volatile() & !OPAQUE }
    }
}

// FIXME: use normal formatting fns
/// Write `value` in decimal into `buffer`, after `prefix`; the text.
fn labelled<'buffer>(buffer: &'buffer mut [u8; 32], prefix: &str, value: u32) -> &'buffer str {
    let mut length = prefix.len();
    buffer[..length].copy_from_slice(prefix.as_bytes());
    let mut digits = [0_u8; 10];
    let mut count = 0;
    let mut rest = value;
    loop {
        digits[count] = b'0' + (rest % 10) as u8;
        count += 1;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    for digit in digits[..count].iter().rev() {
        buffer[length] = *digit;
        length += 1;
    }
    core::str::from_utf8(&buffer[..length]).unwrap_or("")
}

/// The static scene's box.
const BOX: Color = Color::rgb(32, 96, 64);

/// The mapped surface, as `draw` published it.
fn mapped_screen() -> Screen {
    Screen {
        base: core::ptr::with_exposed_provenance_mut(
            usize::try_from(SURFACE_WINDOW_VA).unwrap_or(0),
        ),
        pitch_pixels: u32::try_from(pitch_bytes(WIDTH) / 4).unwrap_or(0),
    }
}

fn order() -> PixelOrder {
    if RGB.load(Ordering::Relaxed) == 1 {
        PixelOrder::Rgb
    } else {
        PixelOrder::Bgr
    }
}

/// The static scene: a background, a box and some text.
fn paint(screen: &mut Screen, order: PixelOrder, font: &Font<'_>) {
    let mut painter = Painter::new(screen, order);
    painter.clear(Color::black());
    painter.rect(10, 10, 250, 250, BOX);
    let mut buffer = [0; 32];
    let width = labelled(&mut buffer, "Display width ", WIDTH);
    // Every string is printable ASCII, which the font has.
    let _drawn = painter
        .text(50, 50, "Hello there!", font, Color::rgb(128, 192, 255))
        .and_then(|()| painter.text(50, 150, width, font, Color::white()))
        .and_then(|()| painter.text(150, 50, "RED", font, Color::red()))
        .and_then(|()| painter.text(160, 60, "GREEN", font, Color::green()))
        .and_then(|()| painter.text(170, 70, "BLUE", font, Color::blue()));
}

libdevice_driver::driver_threads!(lamp);

/// The shell's second Thread: once `draw` has released it, animate the lava
/// lamp beside the static scene, for good.
fn lamp(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    let announce = EventCountKey::from_key(RawKey::from_wire(init.event_count));
    if announce.await_ge(1, EventCountKey::WAIT_INFINITE).is_ok() {
        let compositor = CompositorClient::new(CompositorBindings::from_imports(init.imports()));
        let mut screen = mapped_screen();
        let mut painter = Painter::new(&mut screen, order());
        #[cfg(feature = "qemu")]
        let reader = mapped_screen();
        #[cfg(feature = "qemu")]
        let mut frames = 0_u32;
        let mut lava = Lava::new(LAMP_COLUMNS, LAMP_ROWS, LAMP_CELL, 0x5eed_1a7a);
        loop {
            lava.step();
            lava.draw(&mut painter, LAMP_X, LAMP_Y);
            // Tell the compositor the lamp's rectangle changed.
            let _committed = compositor.commit(
                window_id(),
                i32::try_from(LAMP_X).unwrap_or(0),
                i32::try_from(LAMP_Y).unwrap_or(0),
                LAMP_COLUMNS * LAMP_CELL,
                LAMP_ROWS * LAMP_CELL,
            );
            #[cfg(feature = "qemu")]
            {
                frames += 1;
            }
            #[cfg(feature = "qemu")]
            if frames == 1 || frames == 20 {
                let lit = (0..LAMP_ROWS)
                    .step_by(8)
                    .flat_map(|row| {
                        (0..LAMP_COLUMNS)
                            .step_by(8)
                            .map(move |column| (column, row))
                    })
                    .filter(|&(column, row)| {
                        reader.get(LAMP_X + column * LAMP_CELL, LAMP_Y + row * LAMP_CELL) != 0
                    })
                    .count();
                libuser::semihosting::println!("lamp: frame {frames}, {lit} sampled cells lit");
            }
            // The count never reaches this target: the wait is the frame timer.
            let _timed_out = announce.await_ge(u64::MAX, FRAME_NS);
        }
    }
    let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
    }
}

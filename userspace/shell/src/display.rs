//! The shell's display: setting up the framebuffer through `fb_v1` and mapping
//! its Frames, the static scene `draw` paints, and the background Thread that
//! keeps animating lava blobs beside it.
//!
//! `draw` runs on the shell's first Thread. The first time, it configures the
//! framebuffer, has the fb component install the Frames that cover it into a
//! table the shell lends for the call (`fb_v1.frames`), maps them in the
//! framebuffer window Kickstart provisioned (`DriverInit::fb_window`) and
//! publishes where the pixels are. Every time, it paints the static scene:
//! some text and shapes. After the first, it releases the extra Thread
//! (`driver_threads!`), which waits on the component's `EventCount` and then
//! draws the lava lamp continuously, a frame every few tens of milliseconds,
//! while the prompt stays available.

use {
    core::sync::atomic::{AtomicU64, Ordering},
    fb_client::fb_v1::{
        Client as FbClient, Error as FbError, Exception as FbException, PixelOrder as FbOrder,
    },
    libdevice_driver::{DriverInit, FB_WINDOW_SPANS, FB_WINDOW_VA, driver_init, local_key},
    libgraphics::{Color, Font, Lava, Painter, PixelOrder, Surface},
    libobject::{
        EventCountKey, FrameKey, KeySlot, KeyTableKey, NotificationKey, ObjectType, PageTableKey,
        RawKey, Rights, UntypedKey,
    },
    shell::Failure,
};

/// The mode `draw` sets.
const WIDTH: u32 = 1024;
const HEIGHT: u32 = 768;
const DEPTH: u32 = 32;

/// Size exponent of the table the Frames are lent into: 1024 slots, enough
/// for a 3 MiB framebuffer made entirely of 4 KiB Frames.
const FRAME_TABLE_BITS: u8 = 10;
const FRAME_TABLE_GUARD: u32 = 0x7_AB1E;

const SPAN: u64 = 2 << 20;

/// The lava lamp's place on the screen, right of the static scene, and its
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
const CONFIGURE: u64 = 1;
const ADDRESS: u64 = 2;
const SIZE: u64 = 3;
const ORDER: u64 = 4;
const TABLE: u64 = 5;
const FRAMES: u64 = 6;
const MAPPING: u64 = 7;
const WINDOW: u64 = 8;
const FONT: u64 = 9;
const READ_BACK: u64 = 10;

/// What the first Thread publishes for the drawing Thread: where the first
/// pixel is, the pitch in bytes and whether the order is RGB. Written before
/// the `EventCount` advance that announces them.
static BASE: AtomicU64 = AtomicU64::new(0);
static PITCH: AtomicU64 = AtomicU64::new(0);
static RGB: AtomicU64 = AtomicU64::new(0);

/// Whether the framebuffer is mapped already (its Frames are handed out once).
static MAPPED: AtomicU64 = AtomicU64::new(0);

fn failure(step: u64, answer: u64) -> Failure {
    Failure {
        status: DISPLAY_FAILED,
        detail: step << 32 | answer & 0xffff_ffff,
    }
}

fn fb_failure(step: u64, error: &FbError) -> Failure {
    failure(
        step,
        match error {
            FbError::Call { status, .. } => *status,
            FbError::Exception(FbException::FirmwareError { code }) => u64::from(*code),
            FbError::Exception(_) => 0xffff,
            _ => 0xfffe,
        },
    )
}

fn capability_failure(step: u64, error: libobject::CapError) -> Failure {
    let (status, _, _) = error.code();
    failure(step, status)
}

/// Paint the static scene, setting the framebuffer up and releasing the lamp
/// Thread on the first call.
pub fn draw(init: &DriverInit, fb: &FbClient) -> Result<(), Failure> {
    let first = MAPPED.load(Ordering::Acquire) == 0;
    if first {
        map_framebuffer(init, fb)?;
        MAPPED.store(1, Ordering::Release);
    }
    let font = init
        .resource(0)
        .and_then(Font::new)
        .ok_or_else(|| failure(FONT, 0))?;
    let mut screen = mapped_screen();
    paint(&mut screen, order(), &font);
    // The box's corner reads back as painted: the mapping is the framebuffer.
    let painted = BOX.encode(order());
    if screen.get(20, 20) != painted {
        return Err(failure(READ_BACK, u64::from(screen.get(20, 20))));
    }
    if first {
        EventCountKey::from_key(RawKey::from_wire(init.event_count))
            .advance(1)
            .map_err(|error| capability_failure(WINDOW, error))?;
    }
    Ok(())
}

/// Configure the framebuffer, receive its Frames and map them in the window;
/// publish where its first pixel is.
fn map_framebuffer(init: &DriverInit, fb: &FbClient) -> Result<(), Failure> {
    let pitch = fb
        .configure(WIDTH, HEIGHT, DEPTH)
        .map_err(|error| fb_failure(CONFIGURE, &error))?;
    let address = fb.address().map_err(|error| fb_failure(ADDRESS, &error))?;
    let bytes = fb.size().map_err(|error| fb_failure(SIZE, &error))?;
    let order = fb
        .pixel_order()
        .map_err(|error| fb_failure(ORDER, &error))?;

    let own = KeyTableKey::from_key(RawKey::from_wire(init.table));
    let mut own_self = KeyTableKey::from_key(local_key(init.table, KeySlot::SELF_KEYTABLE.0));
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
    let installed = fb
        .frames(&table, 0)
        .map_err(|error| fb_failure(FRAMES, &error))?;

    // The Frames follow the framebuffer's pages in address order. A Frame
    // maps at the window address with the same offset inside a 2 MiB span as
    // its physical address, so every Frame is aligned for its size.
    let span_base = address & !(SPAN - 1);
    let end = (address + u64::from(bytes)).next_multiple_of(0x1000);
    if end - span_base > FB_WINDOW_SPANS as u64 * SPAN {
        return Err(failure(WINDOW, 0));
    }
    let address_space = RawKey::from_wire(init.address_space);
    let l2 = RawKey::from_wire(init.fb_window.l2);
    let mut tables_used = 0;
    let mut table_installed = [false; FB_WINDOW_SPANS];
    for index in 0..installed {
        let source = RawKey::new(KeySlot((FRAME_TABLE_GUARD << FRAME_TABLE_BITS) | index), 1);
        let copy = table
            .copy_derive(
                source,
                &own,
                copy_slot,
                Rights(Rights::READ | Rights::WRITE | Rights::GRANT),
            )
            .map_err(|error| capability_failure(MAPPING, error))?;
        let frame = FrameKey::from_key(copy);
        let (base, size) = frame
            .get_extent()
            .map_err(|error| capability_failure(MAPPING, error))?;
        let virtual_address = FB_WINDOW_VA + (base - span_base);
        if size == 4096 {
            let span = usize::try_from((base - span_base) / SPAN).unwrap_or(usize::MAX);
            if !table_installed[span] {
                let table_key = init.fb_window.tables[tables_used];
                tables_used += 1;
                PageTableKey::from_key(RawKey::from_wire(table_key))
                    .map(l2, FB_WINDOW_VA + span as u64 * SPAN)
                    .map_err(|error| capability_failure(WINDOW, error))?;
                table_installed[span] = true;
            }
        }
        frame
            .map(
                address_space,
                virtual_address,
                Rights(Rights::READ | Rights::WRITE),
                0,
            )
            .map_err(|error| capability_failure(MAPPING, error))?;
        // The mapping stays; the lent-into table keeps the Frame itself.
        own_self
            .delete(copy)
            .map_err(|error| capability_failure(MAPPING, error))?;
    }

    BASE.store(FB_WINDOW_VA + (address - span_base), Ordering::Relaxed);
    PITCH.store(u64::from(pitch), Ordering::Relaxed);
    RGB.store(u64::from(order == FbOrder::Rgb), Ordering::Relaxed);
    Ok(())
}

/// The mapped framebuffer as a drawing surface.
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
        // SAFETY: the surface lies inside the mapped framebuffer: the
        // painter keeps (x, y) inside WIDTH by HEIGHT, which `draw` set.
        unsafe {
            self.base.add(index).write_volatile(pixel);
        }
    }
}

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

impl Screen {
    fn get(&self, x: u32, y: u32) -> u32 {
        let index = y as usize * self.pitch_pixels as usize + x as usize;
        // SAFETY: inside the mapped framebuffer, as for `put`.
        unsafe { self.base.add(index).read_volatile() }
    }
}

/// The static scene's box.
const BOX: Color = Color::rgb(32, 96, 64);

/// The mapped framebuffer, as `draw` published it.
fn mapped_screen() -> Screen {
    Screen {
        base: BASE.load(Ordering::Relaxed) as *mut u32,
        pitch_pixels: u32::try_from(PITCH.load(Ordering::Relaxed) / 4).unwrap_or(0),
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
        let _bits = park.wait(NotificationKey::WAIT_INFINITE);
    }
}

//! The frame Thread: draws what is damaged, at most once every frame time.
//!
//! It sleeps until something is damaged, then draws and waits one frame time
//! (a timer that runs only while there is damage). A frame composes every
//! damaged rectangle in the back buffer, a cacheable region of RAM the
//! compositor owns, and copies the finished rectangles to the write-combining
//! framebuffer. Nothing is ever read back from the framebuffer on the frame
//! path.

use {
    crate::shared::{self, wake_key},
    libcompositor_core::{
        blit::{Canvas, Layer, PixelRead, PixelWrite, compose, present},
        damage::Damage,
        windows::Rect,
    },
    libdevice_driver::{BACK_BUFFER_VA, SURFACE_SLOTS},
    libframebuffer::Framebuffer,
    libgraphics::{Color, PixelOrder},
    libobject::EventCountKey,
};

/// Time between frames while the screen has damage.
const FRAME_NS: u64 = 33_000_000;

/// The desktop's color.
const BACKGROUND_COLOR: Color = Color::rgb(24, 40, 72);

/// The background as a pixel: opaque.
#[must_use]
pub fn background(order: PixelOrder) -> u32 {
    0xff00_0000 | BACKGROUND_COLOR.encode(order)
}

libdevice_driver::driver_threads!(frame_main);

/// A surface the compositor mapped, read with volatile loads because the client
/// writes it while the compositor reads.
#[derive(Clone, Copy)]
struct Mapped {
    base: usize,
    pixels: usize,
}

impl PixelRead for Mapped {
    fn pixel(&self, index: usize) -> u32 {
        if index >= self.pixels {
            return 0;
        }
        // SAFETY: the index is inside the surface, which stays mapped in this
        // `AddressSpace` for good.
        unsafe {
            core::ptr::with_exposed_provenance::<u32>(self.base)
                .add(index)
                .read_volatile()
        }
    }
}

/// The write-combining framebuffer as somewhere to copy to.
struct Display(Framebuffer);

impl PixelWrite for Display {
    fn set_pixel(&mut self, index: usize, value: u32) {
        self.0.write(index, value);
    }
}

/// One window to draw, copied out of the state.
#[derive(Clone, Copy)]
struct LayerInfo {
    rect: Rect,
    surface: Mapped,
    pitch: usize,
}

/// What a frame draws, copied out of the state so that nothing is locked
/// while drawing.
struct Snapshot {
    damage: Damage,
    layers: [Option<LayerInfo>; SURFACE_SLOTS],
    layer_count: usize,
    cursor: (i32, i32),
    framebuffer: Framebuffer,
    order: PixelOrder,
}

/// Take the damage and what is needed to draw it, if there is damage.
fn take() -> Option<Snapshot> {
    let mut turn = shared::enter();
    let state = shared::state(&mut turn);
    if state.damage.is_empty() {
        return None;
    }
    let damage = state.damage;
    state.damage.clear();
    let mut layers = [None; SURFACE_SLOTS];
    let mut layer_count = 0;
    for window in state.router.windows().bottom_up() {
        let Some(surface) = state.surface_of(window.id) else {
            continue;
        };
        if let Some(slot) = layers.get_mut(layer_count) {
            *slot = Some(LayerInfo {
                rect: window.rect,
                surface: Mapped {
                    base: surface.base,
                    pixels: surface.pitch_pixels() * surface.height as usize,
                },
                pitch: surface.pitch_pixels(),
            });
            layer_count += 1;
        }
    }
    Some(Snapshot {
        damage,
        layers,
        layer_count,
        cursor: state.router.pointer(),
        framebuffer: state.framebuffer,
        order: state.order,
    })
}

/// The frame Thread's entry.
pub fn frame_main(_init_va: u64) -> ! {
    let wake = wake_key();
    // The first advance says the compositor is set up.
    let mut seen = wake
        .await_ge(1, EventCountKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("compositor: ready wait failed: {:?}", error.code()));
    let (width, height) = {
        let mut turn = shared::enter();
        let state = shared::state(&mut turn);
        (state.width, state.height)
    };
    // SAFETY: the back buffer is the compositor's own memory, mapped by the
    // first Thread before it said the compositor was ready, large enough for
    // `width * height` pixels, and only this Thread uses it.
    let pixels = unsafe {
        core::slice::from_raw_parts_mut(
            core::ptr::with_exposed_provenance_mut::<u32>(
                usize::try_from(BACK_BUFFER_VA).unwrap_or(0),
            ),
            width as usize * height as usize,
        )
    };
    let mut canvas = Canvas {
        pixels,
        pitch: width as usize,
        width,
        height,
    };
    #[cfg(feature = "qemu")]
    let mut frames = 0_u32;
    loop {
        let Some(snapshot) = take() else {
            // Nothing to draw: sleep until something is damaged.
            seen = wake
                .await_ge(seen + 1, EventCountKey::WAIT_INFINITE)
                .unwrap_or_else(|error| panic!("compositor: wake wait failed: {:?}", error.code()));
            continue;
        };
        draw(&mut canvas, &snapshot);
        #[cfg(feature = "qemu")]
        {
            frames += 1;
            report(&snapshot, frames);
        }
        // The count never reaches this target: the wait is the frame timer.
        let _timed_out = wake.await_ge(u64::MAX, FRAME_NS);
    }
}

/// Draw the damage of `snapshot` in the back buffer and copy it to the display.
fn draw(canvas: &mut Canvas<'_>, snapshot: &Snapshot) {
    let background = background(snapshot.order);
    let mapped: [Mapped; SURFACE_SLOTS] = core::array::from_fn(|index| {
        snapshot.layers[index].map_or(Mapped { base: 0, pixels: 0 }, |layer| layer.surface)
    });
    let layers: [Layer<'_>; SURFACE_SLOTS] = core::array::from_fn(|index| {
        let (rect, pitch) = snapshot.layers[index].map_or(
            (
                Rect {
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                },
                0,
            ),
            |layer| (layer.rect, layer.pitch),
        );
        Layer {
            rect,
            pixels: &mapped[index],
            pitch,
        }
    });
    let mut display = Display(snapshot.framebuffer);
    let pitch = snapshot.framebuffer.pitch_pixels();
    for rect in snapshot.damage.rects() {
        compose(
            canvas,
            rect,
            background,
            &layers[..snapshot.layer_count],
            Some(snapshot.cursor),
        );
        present(canvas, &mut display, pitch, rect);
    }
}

/// Say what the first frames drew, for the test that boots the compositor.
#[cfg(feature = "qemu")]
fn report(snapshot: &Snapshot, frames: u32) {
    use libuser::semihosting as semi;
    let framebuffer = snapshot.framebuffer;
    let background = background(snapshot.order);
    if frames == 1 {
        let corner = framebuffer.read(5 * framebuffer.pitch_pixels() + 5);
        semi::println!(
            "compositor: frame 1 drew the background: {}",
            corner == background
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
            if framebuffer.read(y * framebuffer.pitch_pixels() + x) != background {
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

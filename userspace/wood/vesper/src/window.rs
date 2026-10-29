//! Wood's window: a window of the compositor the size of the engine's frame,
//! which the engine draws its frames straight into.
//!
//! As the shell's `draw` does, Wood asks the compositor for the window
//! (`compositor_v1.create_window`), lending a table for the Frames of its
//! surface, and maps them in its surface window (`SURFACE_WINDOW_VA`). The
//! engine writes each frame into the surface with ordinary stores, in the
//! display's pixel order and opaque ([`Window::pixel`]), and Wood commits
//! it whole; the compositor deals with its write-combining mapping of the
//! surface when it composes the window.

use {
    compositor_client::compositor_v1::{
        Client as CompositorClient, Error as CompositorError, PixelOrder,
    },
    core::sync::atomic::{Ordering, fence},
    libcompositor_core::surface::{FRAME_BYTES, frames, pitch_bytes},
    libdevice_driver::{DriverInit, SURFACE_WINDOW_VA, local_key},
    libobject::{
        CapError, FrameAttribute, FrameKey, KeySlot, KeyTableKey, ObjectType, RawKey, Rights,
        UntypedKey,
    },
    room::engine::platform::{DOOMGENERIC_RESX, DOOMGENERIC_RESY},
    vesper_input_event::record::{EVENT_SIZE, Event},
};

/// The window's size: the engine's frame, unscaled.
#[expect(clippy::cast_possible_truncation, reason = "640 fits")]
const WIDTH: u32 = DOOMGENERIC_RESX as u32;
#[expect(clippy::cast_possible_truncation, reason = "400 fits")]
const HEIGHT: u32 = DOOMGENERIC_RESY as u32;

/// Where the window goes on the 1024x768 screen: the lower right corner,
/// over the lower part of the shell's window, leaving most of its lava lamp
/// in sight.
const PLACE_X: i32 = 370;
const PLACE_Y: i32 = 350;

/// The window's title until the engine names the game.
const TITLE: &str = "Wood";

/// Bytes of a title the compositor keeps.
const MAX_TITLE: usize = 64;

/// Size exponent of the table the Frames are lent into, and its guard.
const FRAME_TABLE_BITS: u8 = 4;
const FRAME_TABLE_GUARD: u32 = 0x7_00D;

/// The alpha byte of a pixel that shows fully.
const OPAQUE: u32 = 0xff00_0000;

/// Panic for the capability operation `step` that failed.
fn capability_failed<Value>(step: &'static str) -> impl FnOnce(CapError) -> Value {
    move |error| panic!("wood: {step} failed: {:?}", error.code())
}

/// The compositor's window the engine's frames go to.
pub struct Window {
    compositor: CompositorClient,
    id: u32,
    /// Whether the display's pixels have red in the low byte (else in the
    /// third, with blue low).
    red_low: bool,
    surface: *mut u32,
    pitch_pixels: usize,
}

impl Window {
    /// Ask the compositor, whose pixels are in `order`, for the window,
    /// receive the Frames of its surface into a table carved at
    /// `table_slot` (and copied through `table_slot + 1`) and map them.
    /// Wood cannot run without its window, so a failure panics.
    pub fn create(
        init: &DriverInit,
        compositor: CompositorClient,
        order: PixelOrder,
        table_slot: u32,
    ) -> Self {
        let own = KeyTableKey::from_key(RawKey::from_wire(init.table));
        let mut own_self = KeyTableKey::from_key(local_key(init.table, KeySlot::SELF_KEYTABLE.0));
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
                .unwrap_or_else(capability_failed("frame table Retype")),
        );
        let id = compositor
            .create_window(&table, WIDTH, HEIGHT, 0)
            .unwrap_or_else(|error| panic!("wood: create_window failed: {error:?}"));
        // A place is a courtesy: the window works anywhere.
        let _moved = compositor.move_window(id, PLACE_X, PLACE_Y);

        // The Frames are in the table in address order, one for each 2 MiB.
        let count = frames(WIDTH, HEIGHT).unwrap_or(0);
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
                .unwrap_or_else(capability_failed("surface Frame copy"));
            FrameKey::from_key(copy)
                .map(
                    address_space,
                    SURFACE_WINDOW_VA + u64::from(index) * FRAME_BYTES,
                    Rights(Rights::READ | Rights::WRITE),
                    FrameAttribute::Default,
                )
                .unwrap_or_else(capability_failed("surface Frame Map"));
            // The mapping stays; the lent-into table keeps the Frame itself.
            own_self
                .delete(copy)
                .unwrap_or_else(capability_failed("surface Frame copy Delete"));
        }

        let window = Self {
            compositor,
            id,
            red_low: order == PixelOrder::Rgb,
            surface: core::ptr::with_exposed_provenance_mut(
                usize::try_from(SURFACE_WINDOW_VA).unwrap_or(0),
            ),
            pitch_pixels: usize::try_from(pitch_bytes(WIDTH) / 4).unwrap_or(0),
        };
        window.set_title(TITLE);
        window
    }

    /// Set the window's title to `title`, cut to what the compositor keeps.
    /// A title is a courtesy: the window works without one.
    pub fn set_title(&self, title: &str) {
        let bytes = &title.as_bytes()[..title.len().min(MAX_TITLE)];
        if let Some(area) = self.compositor.data_area()
            && area.write_bytes(0, bytes)
            && let Some(stage) = area.reference(0, bytes.len() as u64)
        {
            let _titled = self.compositor.set_title(self.id, stage);
        }
    }

    /// The records that rebuild what the compositor sent Wood's window
    /// (`compositor_v1.snapshot`), into `into`. Returns how many were read;
    /// none when the call fails.
    pub fn snapshot(&self, into: &mut [Event]) -> usize {
        let bytes = (into.len() * EVENT_SIZE) as u64;
        let Some(area) = self.compositor.data_area() else {
            return 0;
        };
        let Some(stage) = area.reference(0, bytes) else {
            return 0;
        };
        let Ok(length) = self.compositor.snapshot(stage) else {
            return 0;
        };
        let length = usize::try_from(length)
            .unwrap_or(0)
            .min(into.len() * EVENT_SIZE);
        let mut record = [0_u8; EVENT_SIZE];
        let mut count = 0;
        for offset in (0..length).step_by(EVENT_SIZE) {
            if !area.read_bytes(offset as u64, &mut record) {
                break;
            }
            if let Ok(event) = Event::from_bytes(&record) {
                into[count] = event;
                count += 1;
            }
        }
        count
    }

    /// The opaque surface pixel for the colour `red`, `green`, `blue`, in
    /// the display's order.
    pub fn pixel(&self, red: u8, green: u8, blue: u8) -> u32 {
        let (low, high) = if self.red_low {
            (red, blue)
        } else {
            (blue, red)
        };
        OPAQUE | u32::from(high) << 16 | u32::from(green) << 8 | u32::from(low)
    }

    /// The surface's pixel count: `HEIGHT` rows of `pitch_pixels`.
    fn surface_pixels(&self) -> usize {
        self.pitch_pixels * HEIGHT as usize
    }

    /// Let `draw` write a frame into the surface (its pixels and pitch in
    /// pixels, `HEIGHT` rows of at least `WIDTH`), then commit it whole.
    pub fn draw(&self, draw: &mut dyn FnMut(&mut [u32], usize)) -> Result<(), CompositorError> {
        // SAFETY: `create` mapped the surface read/write at `surface`, at
        // least `HEIGHT` rows of `pitch_pixels`, for Wood alone to write;
        // only this engine Thread uses it, and no other reference to it
        // lives while `draw` runs.
        let pixels =
            unsafe { core::slice::from_raw_parts_mut(self.surface, self.surface_pixels()) };
        draw(pixels, self.pitch_pixels);
        // The frame's stores come before the commit that has the compositor
        // read them.
        fence(Ordering::Release);
        self.compositor.commit(self.id, 0, 0, WIDTH, HEIGHT)
    }

    /// A checksum of the picture in the surface; changing sums show the
    /// picture changing. Reads the whole surface: for rare diagnostics.
    pub fn checksum(&self) -> u32 {
        // SAFETY: as in `draw`; nothing writes the surface meanwhile.
        let pixels = unsafe { core::slice::from_raw_parts(self.surface, self.surface_pixels()) };
        // (A plain rotate and xor cancels out: the engine doubles every row.)
        pixels
            .chunks_exact(self.pitch_pixels)
            .flat_map(|row| &row[..WIDTH as usize])
            .fold(0_u32, |sum, &pixel| {
                sum.wrapping_mul(31).wrapping_add(pixel)
            })
    }
}

#![no_std]

//! The display's framebuffer, mapped.
//!
//! [`Framebuffer::map`] configures the mode through `fb_v1`, has the fb
//! component install the Frames that cover the framebuffer into a table the
//! caller lends for the call (`fb_v1.frames`, which hands them out once), and
//! maps them write-combining in the window Kickstart provisioned
//! (`DriverInit::map_window`, `FB_WINDOW_VA`). Only one component can hold the
//! framebuffer, so this belongs to the one that owns the screen.

use {
    core::fmt,
    fb_client::fb_v1::{
        Client as FbClient, Error as FbError, Exception as FbException, PixelOrder as FbOrder,
    },
    libdevice_driver::{DriverInit, FB_WINDOW_SPANS, FB_WINDOW_VA, local_key},
    libgraphics::PixelOrder,
    libobject::{
        CapError, FrameAttribute, FrameKey, KeySlot, KeyTableKey, ObjectType, PageTableKey, RawKey,
        Rights, UntypedKey,
    },
};

/// Size exponent of the table the Frames are lent into: 1024 slots, enough for
/// a 3 MiB framebuffer made entirely of 4 KiB Frames.
const FRAME_TABLE_BITS: u8 = 10;
const FRAME_TABLE_GUARD: u32 = 0x7_AB1E;

const SPAN: u64 = 2 << 20;

/// Slots of the component's table that [`Framebuffer::map`] uses from the one
/// it is given: the table of Frames, and a slot to copy each Frame into.
pub const SLOTS_USED: u32 = 2;

/// The step of mapping the framebuffer that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Configure = 1,
    Address,
    Size,
    Order,
    Table,
    Frames,
    Mapping,
    Window,
}

impl Step {
    /// What the step does, in a few words.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Configure => "configuring the mode",
            Self::Address => "asking for the address",
            Self::Size => "asking for the size",
            Self::Order => "asking for the pixel order",
            Self::Table => "making the table of Frames",
            Self::Frames => "receiving the Frames",
            Self::Mapping => "mapping a Frame",
            Self::Window => "mapping in the window",
        }
    }
}

/// Mapping the framebuffer failed in `step`; `answer` is what the call said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error {
    pub step: Step,
    pub answer: u64,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} failed with {:#x}", self.step.name(), self.answer)
    }
}

fn fb_error(step: Step, error: &FbError) -> Error {
    Error {
        step,
        answer: match error {
            FbError::Call { status, .. } => *status,
            FbError::Exception(FbException::FirmwareError { code }) => u64::from(*code),
            FbError::Exception(_) => 0xffff,
            _ => 0xfffe,
        },
    }
}

fn capability_error(step: Step, error: CapError) -> Error {
    let (status, _, _) = error.code();
    Error {
        step,
        answer: status,
    }
}

/// A mapped framebuffer: where its first pixel is and how its pixels are laid
/// out. Every pixel is a 32-bit word.
#[derive(Clone, Copy, Debug)]
pub struct Framebuffer {
    base: *mut u32,
    pitch_bytes: u32,
    width: u32,
    height: u32,
    order: PixelOrder,
}

// SAFETY: the mapping is the component's for good and the pointer is only
// dereferenced through the methods below, by the component's Threads that
// agree among themselves who draws.
unsafe impl Send for Framebuffer {}
// SAFETY: as for `Send`.
unsafe impl Sync for Framebuffer {}

impl Framebuffer {
    /// Configure a mode of `width` by `height` pixels of `depth` bits, receive
    /// the framebuffer's Frames and map them. `first_slot` and the slot after
    /// it ([`SLOTS_USED`]) of the component's table must be free; the
    /// component's memory pays for the table of Frames.
    ///
    /// # Errors
    ///
    /// Any step can fail; the error names it.
    pub fn map(
        init: &DriverInit,
        fb: &FbClient,
        first_slot: u32,
        width: u32,
        height: u32,
        depth: u32,
    ) -> Result<Self, Error> {
        let pitch = fb
            .configure(width, height, depth)
            .map_err(|error| fb_error(Step::Configure, &error))?;
        let address = fb
            .address()
            .map_err(|error| fb_error(Step::Address, &error))?;
        let bytes = fb.size().map_err(|error| fb_error(Step::Size, &error))?;
        let order = fb
            .pixel_order()
            .map_err(|error| fb_error(Step::Order, &error))?;

        let own = KeyTableKey::from_key(RawKey::from_wire(init.table));
        let mut own_self = KeyTableKey::from_key(local_key(init.table, KeySlot::SELF_KEYTABLE.0));
        let copy_slot = first_slot + 1;
        let table = KeyTableKey::from_key(
            UntypedKey::from_key(RawKey::from_wire(init.memory))
                .retype(
                    ObjectType::KEY_TABLE,
                    FRAME_TABLE_BITS,
                    FRAME_TABLE_GUARD,
                    1,
                    &own,
                    first_slot,
                    Rights::all(),
                )
                .map_err(|error| capability_error(Step::Table, error))?,
        );
        let installed = fb
            .frames(&table, 0)
            .map_err(|error| fb_error(Step::Frames, &error))?;

        // The Frames follow the framebuffer's pages in address order. A Frame
        // maps at the window address with the same offset inside a 2 MiB span as
        // its physical address, so every Frame is aligned for its size.
        let span_base = address & !(SPAN - 1);
        let end = (address + u64::from(bytes)).next_multiple_of(0x1000);
        if end - span_base > FB_WINDOW_SPANS as u64 * SPAN {
            return Err(Error {
                step: Step::Window,
                answer: 0,
            });
        }
        let address_space = RawKey::from_wire(init.address_space);
        let l2 = RawKey::from_wire(init.map_window.l2);
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
                .map_err(|error| capability_error(Step::Mapping, error))?;
            let frame = FrameKey::from_key(copy);
            let (base, size) = frame
                .get_extent()
                .map_err(|error| capability_error(Step::Mapping, error))?;
            let virtual_address = FB_WINDOW_VA + (base - span_base);
            if size == 4096 {
                let span = usize::try_from((base - span_base) / SPAN).unwrap_or(usize::MAX);
                if !table_installed[span] {
                    let table_key = init.map_window.tables[tables_used];
                    tables_used += 1;
                    PageTableKey::from_key(RawKey::from_wire(table_key))
                        .map(l2, FB_WINDOW_VA + span as u64 * SPAN)
                        .map_err(|error| capability_error(Step::Window, error))?;
                    table_installed[span] = true;
                }
            }
            frame
                .map(
                    address_space,
                    virtual_address,
                    Rights(Rights::READ | Rights::WRITE),
                    FrameAttribute::WriteCombining,
                )
                .map_err(|error| capability_error(Step::Mapping, error))?;
            // The mapping stays; the lent-into table keeps the Frame itself.
            own_self
                .delete(copy)
                .map_err(|error| capability_error(Step::Mapping, error))?;
        }

        Ok(Self {
            base: core::ptr::with_exposed_provenance_mut(
                usize::try_from(FB_WINDOW_VA + (address - span_base)).unwrap_or(0),
            ),
            pitch_bytes: pitch,
            width,
            height,
            order: if order == FbOrder::Rgb {
                PixelOrder::Rgb
            } else {
                PixelOrder::Bgr
            },
        })
    }

    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Pixels from the start of one row to the start of the next.
    #[must_use]
    pub const fn pitch_pixels(&self) -> usize {
        (self.pitch_bytes / 4) as usize
    }

    #[must_use]
    pub const fn order(&self) -> PixelOrder {
        self.order
    }

    /// Store `pixel` at pixel `index` (row times pitch plus column) with a
    /// volatile write; an index past the framebuffer is ignored.
    pub fn write(&self, index: usize, pixel: u32) {
        if index < self.pitch_pixels() * self.height as usize {
            // SAFETY: the index is inside the mapped framebuffer, which is the
            // component's for good.
            unsafe {
                self.base.add(index).write_volatile(pixel);
            }
        }
    }

    /// The pixel at pixel `index`, 0 for an index past the framebuffer.
    #[must_use]
    pub fn read(&self, index: usize) -> u32 {
        if index < self.pitch_pixels() * self.height as usize {
            // SAFETY: as for `write`.
            unsafe { self.base.add(index).read_volatile() }
        } else {
            0
        }
    }
}

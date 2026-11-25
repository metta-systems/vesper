#![no_std]

//! A scanout output's memory, mapped.
//!
//! [`Framebuffer::map`] reads the current mode of a `display_v1` scanout
//! output (the mailbox framebuffer), has the output install the Frames that
//! cover its memory into a table the caller lends for the call
//! (`display_v1.frames`, which hands them out once per generation), and maps
//! them write-combining in the window Kickstart provisioned
//! (`DriverInit::map_window`, `FB_WINDOW_VA`). Only one component can hold an
//! output's memory, so this belongs to the one that owns the screen.

use {
    core::fmt,
    display::display_v1::{Exception as DisplayException, Format},
    display_client::display_v1::{Client as DisplayClient, Error as DisplayError},
    libdevice_driver::{DriverInit, FB_WINDOW_SPANS, FB_WINDOW_VA, local_key},
    libobject::{
        CapError, FrameAttribute, FrameKey, KeySlot, KeyTableKey, ObjectType, PageTableKey, RawKey,
        Rights, UntypedKey,
    },
};

/// Size exponent of the table the Frames are lent into: 2048 slots, enough for
/// the 4 KiB Frames at both ends of a framebuffer that fills the window, and
/// the 2 MiB Frames between them.
const FRAME_TABLE_BITS: u8 = 11;
const FRAME_TABLE_GUARD: u32 = 0x7_AB1E;

const SPAN: u64 = 2 << 20;

/// Slots of the component's table that [`Framebuffer::map`] uses from the one
/// it is given: the table of Frames, and a slot to copy each Frame into.
pub const SLOTS_USED: u32 = 2;

/// The step of mapping the framebuffer that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Mode = 1,
    Table,
    Frames,
    Mapping,
    Window,
}

impl Step {
    /// What the step does, for messages.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Mode => "asking for the mode",
            Self::Table => "making the table of Frames",
            Self::Frames => "receiving the Frames",
            Self::Mapping => "mapping a Frame",
            Self::Window => "mapping in the window",
        }
    }
}

/// Mapping the framebuffer failed at `step`; `answer` is the output's or the
/// capability call's status, or 0 when the framebuffer does not fit the
/// window.
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

fn display_error(step: Step, error: &DisplayError) -> Error {
    Error {
        step,
        answer: match error {
            DisplayError::Call { status, .. } => *status,
            DisplayError::Exception(DisplayException::InstallFailed { installed }) => {
                u64::from(*installed)
            }
            DisplayError::Exception(_) => 0xffff,
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

/// A scanout output's memory, mapped write-combining.
#[derive(Clone, Copy, Debug)]
pub struct Framebuffer {
    base: *mut u8,
    pitch_bytes: u32,
    width: u32,
    height: u32,
    format: Format,
}

// SAFETY: the framebuffer is memory mapped for good in this `AddressSpace`;
// the pointer is only an address into it.
unsafe impl Send for Framebuffer {}
// SAFETY: as above; the compositor draws on an output from one Thread.
unsafe impl Sync for Framebuffer {}

impl Framebuffer {
    /// Map the memory of `display`, a scanout output, in its current mode.
    /// Two slots from `first_slot` of the component's table are used
    /// ([`SLOTS_USED`]).
    ///
    /// # Errors
    ///
    /// The step that failed, with the output's or the kernel's answer.
    pub fn map(init: &DriverInit, display: &DisplayClient, first_slot: u32) -> Result<Self, Error> {
        let (width, height, pitch, format) = display
            .get_mode()
            .map_err(|error| display_error(Step::Mode, &error))?;

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
        let (installed, offset) = display
            .frames(&table, 0)
            .map_err(|error| display_error(Step::Frames, &error))?;

        let address_space = RawKey::from_wire(init.address_space);
        let l2 = RawKey::from_wire(init.map_window.l2);
        let mut tables_used = 0;
        let mut table_installed = [false; FB_WINDOW_SPANS];
        // The first Frame's address: the window starts at the beginning of
        // its 2 MiB span.
        let mut first_frame = None;
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
            let span_base = *first_frame.get_or_insert(base) & !(SPAN - 1);
            let last_span =
                usize::try_from((base + size - 1 - span_base) / SPAN).unwrap_or(usize::MAX);
            if last_span >= FB_WINDOW_SPANS {
                return Err(Error {
                    step: Step::Window,
                    answer: 0,
                });
            }
            // A 4 KiB Frame needs its span's last-level table; a 2 MiB Frame
            // is a block entry of the second-level table itself.
            if size == 4096 && !table_installed[last_span] {
                let table_key = init.map_window.tables[tables_used];
                tables_used += 1;
                PageTableKey::from_key(RawKey::from_wire(table_key))
                    .map(l2, FB_WINDOW_VA + last_span as u64 * SPAN)
                    .map_err(|error| capability_error(Step::Window, error))?;
                table_installed[last_span] = true;
            }
            frame
                .map(
                    address_space,
                    FB_WINDOW_VA + (base - span_base),
                    Rights(Rights::READ | Rights::WRITE),
                    FrameAttribute::WriteCombining,
                )
                .map_err(|error| capability_error(Step::Mapping, error))?;
            own_self
                .delete(copy)
                .map_err(|error| capability_error(Step::Mapping, error))?;
        }
        let Some(first_frame) = first_frame else {
            return Err(Error {
                step: Step::Frames,
                answer: 0,
            });
        };
        // The first Frame sits this far into the window, and the first pixel
        // `offset` bytes into the first Frame.
        let first_pixel = FB_WINDOW_VA + (first_frame & (SPAN - 1)) + u64::from(offset);
        Ok(Self {
            base: core::ptr::with_exposed_provenance_mut(usize::try_from(first_pixel).unwrap_or(0)),
            pitch_bytes: pitch,
            width,
            height,
            format,
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

    /// Bytes from the start of one row to the start of the next.
    #[must_use]
    pub const fn pitch_bytes(&self) -> usize {
        self.pitch_bytes as usize
    }

    /// How the pixels sit in memory.
    #[must_use]
    pub const fn format(&self) -> Format {
        self.format
    }

    /// The framebuffer's bytes: `pitch_bytes` times `height` of them.
    ///
    /// # Safety
    ///
    /// Only one caller may hold the slice at a time: the compositor draws on
    /// an output from one Thread.
    #[must_use]
    pub unsafe fn bytes(&self) -> &'static mut [u8] {
        // SAFETY: `map` mapped the whole framebuffer at `base` for good, and
        // the caller holds the only slice.
        unsafe {
            core::slice::from_raw_parts_mut(self.base, self.pitch_bytes() * self.height as usize)
        }
    }

    /// The 32-bit pixel at `index` (row times pitch in pixels plus column),
    /// read back; 0 outside the framebuffer. For tests only: reading
    /// write-combining memory is slow.
    #[must_use]
    pub fn read(&self, index: usize) -> u32 {
        if index * 4 + 4 > self.pitch_bytes() * self.height as usize {
            return 0;
        }
        // SAFETY: inside the framebuffer, which stays mapped.
        let bytes = unsafe { self.base.add(index * 4).cast::<[u8; 4]>().read_volatile() };
        u32::from_le_bytes(bytes)
    }
}

#![no_std]
#![no_main]
// libqemu's semihosting `println!` (the frame Thread's progress under QEMU)
// expands to it.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! The compositor program: it owns the outputs and the input, and clients
//! draw into windows.
//!
//! It drives every `display_v1` output the image has, up to `MAX_OUTPUTS`
//! (`via every`): each shows a viewport of one desktop, by default left to
//! right in import order. It is the only component that maps a scanout
//! output's memory or writes a flush output's data area. It draws a
//! background, the windows that clients created through `compositor_v1` (each
//! a surface of compositor-owned memory lent to the client) and the pointer's
//! cursor: the frame Thread draws the scanout outputs once every frame time
//! while something is damaged, and a presenter Thread draws and flushes each
//! flush output at its own pace (`frame`). The first Thread reads the USB HID
//! component's input stream, if there is one, folds it into the router and
//! sends each client the events of its windows on the client's event channel
//! (`input`), at input priority (`libdevice_driver::Priority::INPUT`); calls
//! from clients enter the compositor on their own stacks (`export`). The state
//! they share is behind a turnstile (`shared`).

mod arrange;
mod export;
mod frame;
mod input;
mod shared;

use {
    display::display_v1::Format as DisplayFormat,
    display_client::display_v1::{Bindings as DisplayBindings, Client as DisplayClient},
    input_client::input_v1::{Bindings as InputBindings, Client as InputClient},
    libcompositor_core::{
        blit::Format,
        layout::{Layout, MAX_OUTPUTS},
        surface::{FRAME_BITS, FRAME_BYTES, frames},
    },
    libdevice_driver::{BACK_BUFFER_VA, DriverInit, SURFACE_SPANS, local_key},
    libframebuffer::{Framebuffer, SLOTS_USED},
    libobject::{
        AddressSpaceKey, EventCountKey, FrameAttribute, FrameKey, KeyTableKey, ObjectType, RawKey,
        Rights, StackPoolHandle, UntypedKey,
    },
    libserial_trace::{Trace, trace},
    libuser as user,
    shared::{Display, LayoutMasters, Memory},
};

user::entry!(main);

// The first Thread routes the input: it blocks between producer frames and
// does little for each, so it runs ahead of the CPU-bound programs. Drawing is
// the frame Thread's and the presenters', which stay normal.
libdevice_driver::entry_priority!(libdevice_driver::Priority::INPUT);

fn main(_init_va: u64) -> ! {
    let init = shared::init();
    let mut trace = Trace::new(init);
    trace!(trace, "compositor: starting");
    set_up(init, &mut trace);

    let client = InputClient::new(InputBindings::from_imports(init.imports()));
    input::run(init, &mut trace, &client);
    // No input: the compositor still serves its clients.
    loop {
        let _parked = shared::wake_key().await_ge(u64::MAX, EventCountKey::WAIT_INFINITE);
    }
}

/// The compositor's own format for a `display_v1` format.
const fn format(display: DisplayFormat) -> Format {
    match display {
        DisplayFormat::Xrgb8888 => Format::Xrgb8888,
        DisplayFormat::Xbgr8888 => Format::Xbgr8888,
        DisplayFormat::Rgb565Be => Format::Rgb565Be,
        DisplayFormat::Rgb565 => Format::Rgb565,
    }
}

/// Where the next output's back buffer goes, and the next free slot.
struct Cursor {
    next_slot: u32,
    back_frames: u32,
}

/// Find the outputs, map their memory and a back buffer for each, open the
/// shared state and say the exports can be served. Kept out of `main`, as
/// `input::run` is, so the stack this takes once (the state is built there
/// before it moves into its static) is free again for the input loop and the
/// router under it.
#[inline(never)]
fn set_up(init: &DriverInit, trace: &mut Trace) {
    // The slots after the first free one: the turnstile's EventCount, then
    // for each scanout output a table and a slot to copy its Frames through,
    // and then whatever the back buffers and the surfaces need.
    let own = KeyTableKey::from_key(RawKey::from_wire(init.table));
    let turns_slot = u32::try_from(init.first_free_slot).expect("a slot fits in u32");
    let turns = UntypedKey::from_key(RawKey::from_wire(init.memory))
        .retype(
            ObjectType::EVENT_COUNT,
            0,
            0,
            1,
            &own,
            turns_slot,
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("compositor: turns Retype failed: {:?}", error.code()));
    let mut cursor = Cursor {
        next_slot: turns_slot + 1,
        back_frames: 0,
    };

    let imports = init.imports();
    let instances = DisplayBindings::instances(imports);
    let mut displays = [None; MAX_OUTPUTS];
    for (index, slot) in displays.iter_mut().enumerate().take(instances) {
        *slot = open_display(init, trace, index, &mut cursor);
    }
    if instances > MAX_OUTPUTS {
        trace!(
            trace,
            "compositor: {instances} outputs; driving the first {MAX_OUTPUTS}"
        );
    }
    // The primary output is the largest scanout output: scanout outputs come
    // first, then larger before smaller, in import order where they tie.
    displays.sort_unstable_by_key(|display| {
        display.map_or((2, 0, 0), |display: Display| {
            let scanout = matches!(display.memory, Memory::Scanout(_));
            let area = u64::from(display.width) * u64::from(display.height);
            (u8::from(!scanout), u64::MAX - area, display.instance)
        })
    });
    let sizes: [Option<(u32, u32)>; MAX_OUTPUTS] = core::array::from_fn(|index| {
        displays[index].map(|display: Display| (display.width, display.height))
    });
    let layout = Layout::row(&sizes);
    assert!(!layout.is_empty(), "compositor: no display output");
    for (index, viewport) in layout.viewports() {
        if let Some(display) = displays[index] {
            trace!(
                trace,
                "compositor: output {index}: instance {}, {}x{} {:?}, {}, at {},{}",
                display.instance,
                display.width,
                display.height,
                display.format,
                if matches!(display.memory, Memory::Scanout(_)) {
                    "scanout"
                } else {
                    "flush"
                },
                viewport.x,
                viewport.y
            );
        }
    }
    // Surfaces and back buffers in the first 32-bit output's format, so that
    // it shows them by copying.
    let canvas = displays
        .iter()
        .flatten()
        .map(|display| display.format)
        .find(|format| !format.is_rgb565())
        .unwrap_or(Format::Xrgb8888);
    trace!(trace, "compositor: drawing in {canvas:?}");

    // The masters of the display_layout_v1 keys: calls through them run on
    // the stack pool Kickstart made for the compositor's clients.
    let layout_masters = StackPoolHandle::from_wire(init.export_pool).map(|pool| {
        let masters = LayoutMasters::new(
            AddressSpaceKey::from_key(RawKey::from_wire(init.address_space)),
            KeyTableKey::from_key(RawKey::from_wire(init.table)),
            pool,
            cursor.next_slot,
            arrange::display_layout_v1_entries(),
        );
        cursor.next_slot += LayoutMasters::SLOTS;
        masters
    });

    shared::open(
        shared::State::new(displays, layout, canvas, cursor.next_slot, layout_masters),
        turns,
    );
    // Say the drawing Threads can draw and the exports can be served.
    EventCountKey::from_key(RawKey::from_wire(init.event_count))
        .advance(1)
        .unwrap_or_else(|error| panic!("compositor: ready advance failed: {:?}", error.code()));
}

/// Ask `display_v1` instance `index` what it is, map its memory and a back
/// buffer for it. An output that is not present, or whose memory cannot be
/// mapped, is left out.
fn open_display(
    init: &DriverInit,
    trace: &mut Trace,
    index: usize,
    cursor: &mut Cursor,
) -> Option<Display> {
    let display = DisplayClient::new(DisplayBindings::from_imports_nth(init.imports(), index));
    let (_generation, scanout, width_mm, height_mm, rotations) = display
        .describe()
        .inspect_err(|error| {
            trace!(
                trace,
                "compositor: display_v1 instance {index} left out: {error:?}"
            );
        })
        .ok()?;
    let (width, height, pitch, display_format) = display
        .get_mode()
        .inspect_err(|error| {
            trace!(
                trace,
                "compositor: display_v1 instance {index} left out: {error:?}"
            );
        })
        .ok()?;
    let format = format(display_format);
    let memory = if scanout {
        let framebuffer = Framebuffer::map(init, &display, cursor.next_slot)
            .inspect_err(|error| {
                trace!(
                    trace,
                    "compositor: display_v1 instance {index} left out: {error}"
                );
            })
            .ok()?;
        cursor.next_slot += SLOTS_USED;
        Memory::Scanout(framebuffer)
    } else {
        let area = display.data_area()?;
        let pitch = pitch as usize;
        if (pitch * height as usize) as u64 > area.len {
            trace!(
                trace,
                "compositor: display_v1 instance {index} left out: {width}x{height} does not fit its data area"
            );
            return None;
        }
        Memory::Flush {
            base: area.base,
            len: area.len,
            pitch,
        }
    };
    let back_buffer = map_back_buffer(init, width, height, cursor)?;
    trace!(
        trace,
        "compositor: display_v1 instance {index}: {width}x{height} {format:?}, {}, {width_mm}x{height_mm} mm",
        if scanout { "scanout" } else { "flush" }
    );
    // Every output starts upright: the layout is not kept (M4).
    Some(Display {
        instance: index,
        width,
        height,
        format,
        memory,
        back_buffer,
        upright: (width, height),
        native: (width, height),
        hardware_turns: 0,
        software_turns: 0,
        rotations,
        width_mm,
        height_mm,
    })
}

/// Make and map a back buffer of `width` by `height` pixels: memory the
/// compositor draws in before it converts to an output, cacheable and its own.
/// The back buffers sit one after another from `BACK_BUFFER_VA`, in at most
/// `SURFACE_SPANS` Frames together.
fn map_back_buffer(init: &DriverInit, width: u32, height: u32, cursor: &mut Cursor) -> Option<u64> {
    let count = frames(width, height)?;
    if (cursor.back_frames + count) as usize > SURFACE_SPANS {
        return None;
    }
    let own = KeyTableKey::from_key(RawKey::from_wire(init.table));
    UntypedKey::from_key(RawKey::from_wire(init.memory))
        .retype(
            ObjectType::FRAME,
            FRAME_BITS,
            0,
            count,
            &own,
            cursor.next_slot,
            Rights::all(),
        )
        .unwrap_or_else(|error| {
            panic!("compositor: back buffer Retype failed: {:?}", error.code())
        });
    let base = BACK_BUFFER_VA + u64::from(cursor.back_frames) * FRAME_BYTES;
    for index in 0..count {
        FrameKey::from_key(local_key(init.table, cursor.next_slot + index))
            .map(
                RawKey::from_wire(init.address_space),
                base + u64::from(index) * FRAME_BYTES,
                Rights(Rights::READ | Rights::WRITE),
                FrameAttribute::Default,
            )
            .unwrap_or_else(|error| {
                panic!("compositor: back buffer Map failed: {:?}", error.code())
            });
    }
    cursor.next_slot += count;
    cursor.back_frames += count;
    Some(base)
}

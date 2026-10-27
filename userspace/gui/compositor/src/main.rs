#![no_std]
#![no_main]
// libqemu's semihosting `println!` (the frame Thread's progress under QEMU)
// expands to it.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! The compositor program: it owns the screen and the input, and clients draw
//! into windows.
//!
//! It is the only component that maps the framebuffer. It draws a background,
//! the windows that clients created through `compositor_v1` (each a surface of
//! compositor-owned memory lent to the client) and the pointer's cursor, once
//! every frame time while something is damaged (the frame Thread, `frame`).
//! The first Thread reads the USB HID component's input stream, if there is
//! one, folds it into the router and sends each client the events of its
//! windows on the client's event channel (`input`); calls from clients enter the
//! compositor on their own stacks (`export`). The state they share is behind a
//! turnstile (`shared`).

mod export;
mod frame;
mod input;
mod shared;

use {
    fb_client::fb_v1::{Bindings as FbBindings, Client as FbClient},
    input_client::input_v1::{Bindings as InputBindings, Client as InputClient},
    libcompositor_core::surface::{FRAME_BITS, FRAME_BYTES, frames},
    libdevice_driver::{BACK_BUFFER_VA, DriverInit, local_key},
    libframebuffer::{Framebuffer, SLOTS_USED},
    libobject::{
        EventCountKey, FrameAttribute, FrameKey, KeyTableKey, ObjectType, RawKey, Rights,
        UntypedKey,
    },
    libserial_trace::{Trace, trace},
};

libuser::entry!(main);

/// The mode the display is set to.
const WIDTH: u32 = 1024;
const HEIGHT: u32 = 768;
const DEPTH: u32 = 32;

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

/// Map the display and the back buffer, open the shared state and say the
/// exports can be served. Kept out of `main`, as `input::run` is, so the
/// stack this takes once (the state is built there before it moves into its
/// static) is free again for the input loop and the router under it.
#[inline(never)]
fn set_up(init: &DriverInit, trace: &mut Trace) {
    // The slots after the first free one: the turnstile's EventCount, the
    // framebuffer's table and a slot to copy its Frames through, and then
    // whatever the back buffer and the surfaces need.
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

    let fb = FbClient::new(FbBindings::from_imports(init.imports()));
    let framebuffer = Framebuffer::map(init, &fb, turns_slot + 1, WIDTH, HEIGHT, DEPTH)
        .unwrap_or_else(|error| panic!("compositor: the framebuffer failed: {error}"));
    let mut next_slot = turns_slot + 1 + SLOTS_USED;

    // The back buffer: memory the compositor draws in before it copies to the
    // framebuffer, cacheable and its own.
    let back_frames = frames(WIDTH, HEIGHT).expect("the display fits the back buffer");
    UntypedKey::from_key(RawKey::from_wire(init.memory))
        .retype(
            ObjectType::FRAME,
            FRAME_BITS,
            0,
            back_frames,
            &own,
            next_slot,
            Rights::all(),
        )
        .unwrap_or_else(|error| {
            panic!("compositor: back buffer Retype failed: {:?}", error.code())
        });
    for index in 0..back_frames {
        FrameKey::from_key(local_key(init.table, next_slot + index))
            .map(
                RawKey::from_wire(init.address_space),
                BACK_BUFFER_VA + u64::from(index) * FRAME_BYTES,
                Rights(Rights::READ | Rights::WRITE),
                FrameAttribute::Default,
            )
            .unwrap_or_else(|error| {
                panic!("compositor: back buffer Map failed: {:?}", error.code())
            });
    }
    next_slot += back_frames;

    trace!(
        trace,
        "compositor: display {}x{} pitch {} bytes, {:?}",
        framebuffer.width(),
        framebuffer.height(),
        framebuffer.pitch_pixels() * 4,
        framebuffer.order()
    );

    shared::open(shared::State::new(framebuffer, next_slot), turns);
    // Say the frame Thread can draw and the exports can be served.
    EventCountKey::from_key(RawKey::from_wire(init.event_count))
        .advance(1)
        .unwrap_or_else(|error| panic!("compositor: ready advance failed: {:?}", error.code()));
}

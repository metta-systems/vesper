#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! layout-test's client, loaded twice: as `arranger`, whose manifest entry
//! says `layout = true`, and as `reader`, whose does not.
//!
//! Each asks the compositor for the keys of `display_layout_v1`
//! (`compositor_v1.display_layout`) and reads the layout: one output, the
//! mailbox framebuffer, upright at the desktop origin. Then each tries to turn
//! it, and so learns which it is:
//! - the reader is refused (`not_permitted`), for moving it too;
//! - the arranger turns it a quarter, which the framebuffer cannot do itself,
//!   so the compositor turns its pixels: the output reads back 768x1024,
//!   turned `r90`, in a new generation. Moved away from the origin, it reads
//!   back at the origin again (the origin follows it). Turned half and back
//!   upright, it reads back 1024x768. An output or window that does not exist
//!   is refused.
//!
//! Each then signals the report key layout-test installed in its table, with
//! its own bit. Any failure panics, which ends the QEMU run.

use {
    compositor_client::{
        compositor_v1::{Bindings as CompositorBindings, Client as CompositorClient},
        display_layout_v1::{Bindings, Client, Error, Exception, Rotation},
    },
    driver_protocol::report_key,
    libdevice_driver::driver_init,
    libobject::{EventCountKey, NotificationKey, RawKey},
    libuser::semihosting as semi,
};

libuser::entry!(main);

/// Keys of `display_layout_v1`: one per function.
const LAYOUT_KEYS: usize = 7;

/// Nanoseconds between looks for the compositor.
const POLL_NS: u64 = 10_000_000;

/// The framebuffer's mode under QEMU, which gives no EDID.
const SIZE: (u32, u32) = (1024, 768);

/// The exception `result` raised, if it raised one.
fn raised<T>(result: Result<T, Error>) -> Option<Exception> {
    match result.err()? {
        Error::Exception(exception) => Some(exception),
        _ => None,
    }
}

fn main(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    let compositor = CompositorClient::new(CompositorBindings::from_imports(init.imports()));
    // The component's own EventCount, which nothing advances: a timer.
    let timer = EventCountKey::from_key(RawKey::from_wire(init.event_count));
    while compositor.pixel_order().is_err() {
        let _slept = timer.sleep(POLL_NS);
    }

    // The keys go into this program's own table, from its first free slot;
    // their wire keys come back in the data area.
    let own = libobject::KeyTableKey::from_key(RawKey::from_wire(init.table));
    let first_slot = u32::try_from(init.first_free_slot).expect("a slot fits in u32");
    compositor
        .display_layout(&own, first_slot)
        .unwrap_or_else(|error| panic!("display_layout failed: {error:?}"));
    let area = compositor
        .data_area()
        .expect("compositor_v1 has a data area");
    let mut keys = [0_u64; LAYOUT_KEYS];
    assert!(area.read_u64s(0, &mut keys), "the keys in the area");
    let layout = Client::new(Bindings::from_installed(&keys, None).expect("one key per function"));

    assert_eq!(layout.count(), Ok(1), "the framebuffer only");
    let (id, _x, _y, _width, _height) = layout
        .output(0)
        .unwrap_or_else(|error| panic!("output 0 failed: {error:?}"));
    assert_eq!(raised(layout.output(1)), Some(Exception::NoOutput));
    assert_eq!(
        raised(layout.window_output(999)),
        Some(Exception::NoOutput),
        "no such window of this client"
    );

    let arranges = match layout.set_rotation(id, Rotation::R0) {
        Ok(()) => true,
        Err(Error::Exception(Exception::NotPermitted)) => false,
        Err(error) => panic!("set_rotation failed: {error:?}"),
    };
    let bit = if arranges {
        arrange(&layout, id);
        1
    } else {
        assert_eq!(
            raised(layout.set_position(id, 10, 10)),
            Some(Exception::NotPermitted),
            "a reader does not move outputs"
        );
        semi::println!("layout-client: a reader reads the layout and may not change it");
        2
    };

    NotificationKey::from_key(report_key(init.table))
        .signal(bit)
        .unwrap_or_else(|error| panic!("report failed: {:?}", error.code()));
    let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
    }
}

/// The arranger's part: turn and move output `id` and read it back.
fn arrange(layout: &Client, id: u32) {
    let output = |expected: &str| {
        layout
            .output(0)
            .unwrap_or_else(|error| panic!("output 0 failed ({expected}): {error:?}"))
    };
    let rotation = || {
        layout
            .output_details(0)
            .unwrap_or_else(|error| panic!("output_details failed: {error:?}"))
            .0
    };
    assert_eq!(output("upright"), (id, 0, 0, SIZE.0, SIZE.1));
    assert_eq!(rotation(), Rotation::R0);

    let before = layout.generation();
    layout
        .set_rotation(id, Rotation::R90)
        .unwrap_or_else(|error| panic!("set_rotation r90 failed: {error:?}"));
    assert_eq!(
        output("a quarter turn"),
        (id, 0, 0, SIZE.1, SIZE.0),
        "the turned size"
    );
    assert_eq!(rotation(), Rotation::R90);
    assert_ne!(layout.generation(), before, "a new generation");

    layout
        .set_position(id, 100, 50)
        .unwrap_or_else(|error| panic!("set_position failed: {error:?}"));
    assert_eq!(
        output("moved"),
        (id, 0, 0, SIZE.1, SIZE.0),
        "the origin follows the only output"
    );

    layout
        .set_rotation(id, Rotation::R180)
        .unwrap_or_else(|error| panic!("set_rotation r180 failed: {error:?}"));
    assert_eq!(output("half a turn"), (id, 0, 0, SIZE.0, SIZE.1));
    assert_eq!(rotation(), Rotation::R180);
    layout
        .set_rotation(id, Rotation::R0)
        .unwrap_or_else(|error| panic!("set_rotation r0 failed: {error:?}"));
    assert_eq!(output("upright again"), (id, 0, 0, SIZE.0, SIZE.1));

    assert_eq!(
        raised(layout.set_rotation(id + 7, Rotation::R90)),
        Some(Exception::NoOutput)
    );
    assert_eq!(
        raised(layout.set_position(id + 7, 0, 0)),
        Some(Exception::NoOutput)
    );
    semi::println!(
        "layout-client: the arranger turned output {id} a quarter (768x1024), moved it and turned it back"
    );
}

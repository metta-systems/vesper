#![no_std]
#![no_main]
// libqemu's semihosting `println!` expands to it.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! window-events: the client end of the compositor's event channel, checked
//! against a scripted sequence (`just test-window-events`).
//!
//! The program posts the buffers of its event channel and makes two windows:
//! A, 400x200 at 312,284 under the pointer's starting place (the middle of the
//! 1024x768 screen), and B, 200x200 at 720,284 to its right. Each new window
//! takes the focus, so B has it. Then it says it is ready, and the recipe
//! drives the USB mouse and keyboard through the QEMU monitor: a click on A
//! (the focus moves to A), a drag 300 pixels to the right, over B, released
//! there (A keeps the pointer's events until the release, then B gets them),
//! and the key `a` (to A, which has the focus).
//!
//! The program checks that each window receives its part, in order, with its
//! own id in `subject`, that B gets no key, and that nothing is dropped. It
//! exits QEMU with status 0 once everything arrived, and 1 on a wrong record
//! or after `TIMEOUT_NS`.

use {
    compositor_client::compositor_v1::{Bindings, Client},
    libcompositor_core::events::{EventChannel, Receiver, data_area},
    libdevice_driver::driver_init,
    libobject::{EventCountKey, KeyTableKey, ObjectType, RawKey, Rights, UntypedKey},
    vesper_input_event::{
        record::{Event, KEY_DOWN, KEY_UP, Kind},
        usage::{code, generic_desktop, page},
    },
};

libuser::entry!(main);

/// Where the windows go and their sizes.
const A_PLACE: (i32, i32) = (312, 284);
const A_SIZE: (u32, u32) = (400, 200);
const B_PLACE: (i32, i32) = (720, 284);
const B_SIZE: (u32, u32) = (200, 200);

/// Size exponent of the table the surfaces' Frames are lent into, and its guard.
const FRAME_TABLE_BITS: u8 = 4;
const FRAME_TABLE_GUARD: u32 = 0x7_00E;

/// Nanoseconds between two looks at the channel, and before giving up.
const POLL_NS: u64 = 10_000_000;
const TIMEOUT_NS: u64 = 120_000_000_000;

const LEFT_BUTTON: u32 = code(page::BUTTON, 1);
const KEY_A: u32 = code(page::KEYBOARD, 0x04);
const POINTER_X: u32 = code(page::GENERIC_DESKTOP, generic_desktop::X);

/// What one window must receive, in order; other records may come between.
#[derive(Clone, Copy)]
enum Expect {
    /// A record of `kind` with `code` and `value`.
    Exact(Kind, u32, i32),
    /// The pointer's X position in the window at least this far right.
    XAtLeast(i32),
}

impl Expect {
    fn matches(self, event: &Event) -> bool {
        match self {
            Self::Exact(kind, code, value) => {
                event.kind == kind && event.code == code && event.value == value
            }
            Self::XAtLeast(least) => {
                event.kind == Kind::Abs && event.code == POINTER_X && event.value >= least
            }
        }
    }
}

/// Window A's part: the focus on creation and losing it to B, the click
/// that gives it back, the drag past its right edge, the release, the
/// pointer leaving, and the key.
const A_EXPECTED: [Expect; 9] = [
    Expect::Exact(Kind::Focus, 0, 1),
    Expect::Exact(Kind::Focus, 0, 0),
    Expect::Exact(Kind::Focus, 0, 1),
    Expect::Exact(Kind::Key, LEFT_BUTTON, KEY_DOWN),
    #[expect(clippy::cast_possible_wrap, reason = "400 fits")]
    Expect::XAtLeast(A_SIZE.0 as i32),
    Expect::Exact(Kind::Key, LEFT_BUTTON, KEY_UP),
    Expect::Exact(Kind::Leave, 0, 0),
    Expect::Exact(Kind::Key, KEY_A, KEY_DOWN),
    Expect::Exact(Kind::Key, KEY_A, KEY_UP),
];

/// Window B's part: the focus on creation, losing it to the click on A, and
/// the pointer entering it once the button is released over it.
const B_EXPECTED: [Expect; 3] = [
    Expect::Exact(Kind::Focus, 0, 1),
    Expect::Exact(Kind::Focus, 0, 0),
    Expect::Exact(Kind::Enter, 0, 0),
];

/// End the run with `code`.
#[cfg_attr(not(feature = "qemu"), allow(unused_variables))]
fn finish(code: u32) -> ! {
    #[cfg(feature = "qemu")]
    {
        libuser::semihosting::exit(code)
    }
    // The program needs QEMU's monitor to drive it; on a board it stops.
    #[cfg(not(feature = "qemu"))]
    {
        libuser::halt()
    }
}

// Without `qemu`, semihosting printing compiles away and `reason` is unused.
#[cfg_attr(not(feature = "qemu"), allow(unused_variables))]
fn fail(reason: core::fmt::Arguments<'_>) -> ! {
    libuser::semihosting::println!("window-events: ❌ {reason}");
    finish(1)
}

fn main(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    let compositor = Client::new(Bindings::from_imports(init.imports()));
    // The component's own EventCount, which nothing advances: a timer.
    let timer = EventCountKey::from_key(RawKey::from_wire(init.event_count));
    let started_ns = libuser::clock::now_ns();
    while compositor.pixel_order().is_err() {
        let _slept = timer.sleep(POLL_NS);
    }

    // The buffers are posted before the windows exist, so the focus each
    // new window gets arrives.
    let Some(area) = compositor.channel().filter(|area| area.event != 0) else {
        fail(format_args!("no event channel"));
    };
    // SAFETY: Kickstart mapped the channel's control page and data page
    // read/write for the program's lifetime.
    let mut receiver = unsafe {
        Receiver::new(
            EventChannel::at(area.control),
            data_area(area.control),
            EventCountKey::from_key(RawKey::from_wire(area.event)),
        )
    };

    let own = KeyTableKey::from_key(RawKey::from_wire(init.table));
    let table_slot = u32::try_from(init.first_free_slot).unwrap_or(u32::MAX);
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
            .unwrap_or_else(|error| fail(format_args!("table Retype: {:?}", error.code()))),
    );
    let window = |first_slot: u32, (width, height): (u32, u32), (x, y): (i32, i32)| {
        let id = compositor
            .create_window(&table, width, height, first_slot)
            .unwrap_or_else(|error| fail(format_args!("create_window: {error:?}")));
        if let Err(error) = compositor.move_window(id, x, y) {
            fail(format_args!("move_window: {error:?}"));
        }
        id
    };
    let a = window(0, A_SIZE, A_PLACE);
    let b = window(1, B_SIZE, B_PLACE);
    libuser::semihosting::println!("window-events: windows {a} (A) and {b} (B) ready");

    let mut a_next = 0;
    let mut b_next = 0;
    loop {
        let Some(event) = receiver.next_event() else {
            if libuser::clock::now_ns().saturating_sub(started_ns) > TIMEOUT_NS {
                fail(format_args!(
                    "timed out: A got {a_next} of {} records, B {b_next} of {}",
                    A_EXPECTED.len(),
                    B_EXPECTED.len()
                ));
            }
            let _slept = timer.sleep(POLL_NS);
            continue;
        };
        libuser::semihosting::println!(
            "window-events: window {}: {:?} code {:#010x} value {}",
            event.subject,
            event.kind,
            event.code,
            event.value
        );
        if event.kind == Kind::Dropped {
            fail(format_args!("the compositor dropped events"));
        }
        let (expected, next): (&[Expect], &mut usize) = if event.subject == a {
            (&A_EXPECTED, &mut a_next)
        } else if event.subject == b {
            if event.kind == Kind::Key {
                fail(format_args!("window B got a key"));
            }
            (&B_EXPECTED, &mut b_next)
        } else {
            fail(format_args!("a record for window {}", event.subject));
        };
        if expected
            .get(*next)
            .is_some_and(|expect| expect.matches(&event))
        {
            *next += 1;
        }
        if a_next == A_EXPECTED.len() && b_next == B_EXPECTED.len() {
            libuser::semihosting::println!(
                "window-events: ✅ focus on creation, click to focus, drag and key reached the windows"
            );
            finish(0);
        }
    }
}

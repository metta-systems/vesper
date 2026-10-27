//! What the engine gets from Vesper: Wood's [`Platform`].
//!
//! - Frames go to the compositor's window ([`Window`]).
//! - Time is the monotonic counter EL0 reads (`libuser::clock`); a sleep is
//!   a timed wait on Wood's own `EventCount`, which nothing advances, so the
//!   other programs run meanwhile.
//! - The one file is the IWAD, served from its bundled resource; there is
//!   nowhere to keep a configuration or a saved game, so writes fail.
//! - Quitting parks the Thread: the rest of the image keeps running.
//! - Keys are the ones the compositor routes to the window ([`Keyboard`]).
//!
//! There is no sound yet.

use {
    crate::{keyboard::Keyboard, window::Window},
    core::cell::Cell,
    libobject::EventCountKey,
    room::engine::platform::{FileHandle, Platform},
};

/// The name the IWAD is opened by (the engine is started with `-iwad`
/// naming it); any directory before it is ignored.
pub const WAD_NAME: &str = "doom1.wad";

/// The handle of the open IWAD.
const WAD_HANDLE: FileHandle = FileHandle(1);

const NANOS_PER_MILLI: u64 = 1_000_000;

/// Vesper, as the engine sees it.
pub struct Wood {
    window: Window,
    keyboard: Keyboard,
    /// Wood's own `EventCount`, which nothing advances: the sleep timer.
    timer: EventCountKey,
    /// The IWAD's bytes, mapped read-only.
    wad: &'static [u8],
    /// When the engine started, in counter nanoseconds.
    started_ns: u64,
    /// Frames presented so far.
    frames: Cell<u32>,
}

impl Wood {
    pub fn new(
        window: Window,
        keyboard: Keyboard,
        timer: EventCountKey,
        wad: &'static [u8],
    ) -> Self {
        Self {
            window,
            keyboard,
            timer,
            wad,
            started_ns: libuser::clock::now_ns(),
            frames: Cell::new(0),
        }
    }
}

/// Whether `path` names the IWAD.
fn is_wad(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case(WAD_NAME))
}

impl Platform for Wood {
    fn draw_frame(&self, frame: &[u32]) {
        let presented = self.window.present(frame);
        let count = self.frames.get().wrapping_add(1);
        self.frames.set(count);
        if count.is_power_of_two() {
            // Changing sums show the picture changing. (A plain rotate and
            // xor cancels out: the engine doubles every row.)
            let sum = frame.iter().fold(0_u32, |sum, &pixel| {
                sum.wrapping_mul(31).wrapping_add(pixel)
            });
            log::info!(
                "frame {count} presented: {}, sum {sum:#010x}",
                presented.is_ok()
            );
        }
    }

    fn sleep_ms(&self, ms: u32) {
        if ms != 0 {
            // A failed wait only makes the sleep shorter.
            let _slept = self.timer.sleep(u64::from(ms) * NANOS_PER_MILLI);
        }
    }

    fn ticks_ms(&self) -> u32 {
        let elapsed = libuser::clock::now_ns().saturating_sub(self.started_ns) / NANOS_PER_MILLI;
        // The engine's clock wraps, as `DG_GetTicksMs` does.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "wrapping millisecond clock"
        )]
        let ticks = elapsed as u32;
        ticks
    }

    fn key(&self) -> Option<(bool, u8)> {
        self.keyboard.next_key(&self.window)
    }

    fn set_window_title(&self, title: &str) {
        self.window.set_title(title);
    }

    fn open(&self, path: &str) -> Option<FileHandle> {
        is_wad(path).then_some(WAD_HANDLE)
    }

    fn len(&self, file: FileHandle) -> u64 {
        if file == WAD_HANDLE {
            self.wad.len() as u64
        } else {
            0
        }
    }

    fn read_at(&self, file: FileHandle, offset: u64, buffer: &mut [u8]) -> usize {
        if file != WAD_HANDLE {
            return 0;
        }
        let rest = usize::try_from(offset)
            .ok()
            .and_then(|offset| self.wad.get(offset..))
            .unwrap_or_default();
        let count = buffer.len().min(rest.len());
        buffer[..count].copy_from_slice(&rest[..count]);
        count
    }

    fn exists(&self, path: &str) -> bool {
        is_wad(path)
    }

    fn error_box(&self, message: &str) {
        log::error!("{message}");
    }

    fn quit(&self, code: i32) -> ! {
        // Under QEMU the engine quitting ends the run, with its exit status.
        #[cfg(feature = "qemu")]
        {
            log::info!("the engine quit with exit status {code}");
            libuser::semihosting::exit(code.cast_unsigned())
        }
        // On hardware the rest of the system keeps running: park the thread.
        #[cfg(not(feature = "qemu"))]
        {
            log::info!("the engine quit with exit status {code}; parking");
            loop {
                let _woken = self.timer.await_ge(u64::MAX, EventCountKey::WAIT_INFINITE);
            }
        }
    }
}

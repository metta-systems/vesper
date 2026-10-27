//! What the compositor's Threads share: the windows, the router, the damage,
//! the surfaces and the clients' event channels, behind one [`Turnstile`].
//!
//! The first Thread reads input and moves the pointer, the calls clients make
//! change windows, and the frame Thread draws what is damaged. Each of them
//! enters the turnstile for as long as it takes to change or copy the state,
//! and none holds a turn while it draws. Events go to the clients' channels
//! under a turn, so each channel has one producer at a time.

use {
    core::cell::UnsafeCell,
    libcompositor_core::{
        cursor,
        damage::Damage,
        events::{EventChannel, Outbox, client_records, data_area, deliver},
        router::{Output, Router},
        windows::{Rect, Window},
    },
    libdevice_driver::{
        DriverInit, INIT_VA, MAX_AREAS, SURFACE_SLOTS, Turn, Turnstile, driver_init,
    },
    libframebuffer::Framebuffer,
    libgraphics::PixelOrder,
    libobject::{EventCountKey, RawKey},
    vesper_input_event::record::{Event, Kind},
};

/// Bytes of a window's title.
pub const MAX_TITLE: usize = 64;

/// A window's surface: memory the compositor owns and has mapped.
#[derive(Clone, Copy, Debug)]
pub struct Surface {
    /// The id of the window the surface belongs to.
    pub window: u32,
    /// The client that asked for the window (its index among the clients, plus one).
    pub client: u32,
    pub width: u32,
    pub height: u32,
    /// Where the compositor maps the surface's first pixel.
    pub base: usize,
    /// The window's title, the first `title_len` bytes.
    pub title: [u8; MAX_TITLE],
    pub title_len: usize,
}

impl Surface {
    /// Pixels from the start of one row to the start of the next.
    #[must_use]
    pub const fn pitch_pixels(&self) -> usize {
        self.width as usize
    }
}

/// The compositor's state.
pub struct State {
    pub router: Router,
    pub damage: Damage,
    /// The surface at each surface window; a slot is handed out once, since a
    /// client may still hold the Frames after its window is gone.
    pub surfaces: [Option<Surface>; SURFACE_SLOTS],
    /// How many surface windows were handed out.
    pub slots_used: usize,
    /// The first slot of the compositor's table that nothing uses yet.
    pub next_table_slot: u32,
    pub order: PixelOrder,
    pub width: u32,
    pub height: u32,
    /// The display, mapped.
    pub framebuffer: Framebuffer,
    /// Where the pointer was when the screen last needed to know.
    pub cursor: (i32, i32),
    /// What each client's event channel lost, by the client's position among
    /// the areas.
    pub outboxes: [Outbox; MAX_AREAS],
}

impl State {
    pub fn new(framebuffer: Framebuffer, next_table_slot: u32) -> Self {
        let (width, height) = (framebuffer.width(), framebuffer.height());
        let router = Router::new(width, height);
        let cursor = router.pointer();
        let mut damage = Damage::new();
        damage.add(Rect {
            x: 0,
            y: 0,
            width,
            height,
        });
        Self {
            router,
            damage,
            surfaces: [None; SURFACE_SLOTS],
            slots_used: 0,
            next_table_slot,
            order: framebuffer.order(),
            width,
            height,
            framebuffer,
            cursor,
            outboxes: [Outbox::new(); MAX_AREAS],
        }
    }

    /// Send each client the events of `out` that go to its windows, on its
    /// event channel; `stamp` marks a `Dropped` record. A client without a
    /// buffer posted loses them (see `libcompositor_core::events`).
    pub fn deliver(&mut self, out: &Output, stamp: u64) {
        for (index, area) in init().areas().iter().enumerate() {
            if area.channel_base == 0 || area.channel_event == 0 {
                continue;
            }
            let Some(outbox) = self.outboxes.get_mut(index) else {
                continue;
            };
            let client = u32::try_from(index).unwrap_or(u32::MAX).saturating_add(1);
            let records = client_records(out.items(), self.router.windows(), client);
            // SAFETY: Kickstart mapped the channel's control page read/write
            // at `channel_base`, and its data page after it, for the
            // compositor's lifetime.
            unsafe {
                let channel = EventChannel::at(area.channel_base);
                deliver(
                    channel,
                    data_area(area.channel_base),
                    EventCountKey::from_key(RawKey::from_wire(area.channel_event)),
                    outbox,
                    records,
                    stamp,
                );
            }
        }
    }

    /// The window `id`'s surface.
    #[must_use]
    pub fn surface_of(&self, id: u32) -> Option<&Surface> {
        let slot = usize::try_from(id.checked_sub(1)?).ok()?;
        self.surfaces
            .get(slot)?
            .as_ref()
            .filter(|surface| surface.window == id)
    }

    /// The screen rectangle of window `id`.
    #[must_use]
    pub fn rect_of(&self, id: u32) -> Option<Rect> {
        self.router.windows().get(id).map(|window| window.rect)
    }

    /// The window on top.
    #[must_use]
    pub fn top(&self) -> Option<u32> {
        self.router
            .windows()
            .bottom_up()
            .last()
            .map(|window| window.id)
    }

    /// Damage the whole of window `id`.
    pub fn damage_window(&mut self, id: u32) {
        if let Some(rect) = self.rect_of(id) {
            self.damage.add(rect);
        }
    }

    /// Damage where the cursor was and where it is now, if it moved. Returns
    /// whether it did.
    pub fn damage_cursor(&mut self) -> bool {
        let now = self.router.pointer();
        if now == self.cursor {
            return false;
        }
        self.damage
            .add(cursor::rect_at(self.cursor.0, self.cursor.1));
        self.damage.add(cursor::rect_at(now.0, now.1));
        self.cursor = now;
        true
    }

    /// Add `window` to the router, on top, and damage it.
    pub fn add_window(&mut self, window: Window) -> bool {
        let added = self.router.windows_mut().add(window);
        if added {
            self.damage_window(window.id);
        }
        added
    }

    /// Take `id` away from the router, damaging where it was.
    pub fn remove_window(&mut self, id: u32) {
        self.damage_window(id);
        self.router.windows_mut().remove(id);
        self.router.forget_window(id);
        if let Some(slot) = usize::try_from(id.wrapping_sub(1)).ok()
            && let Some(surface) = self.surfaces.get_mut(slot)
        {
            *surface = None;
        }
    }

    /// Feed `event` to the router. A frame's end also damages what the frame
    /// changed on the screen: the cursor, and the window a press raised.
    /// Returns whether the screen needs drawing.
    pub fn feed(&mut self, event: &Event, out: &mut Output) -> bool {
        let top = self.top();
        self.router.feed(event, out);
        if event.kind != Kind::Sync {
            return false;
        }
        let mut changed = self.damage_cursor();
        let raised = self.top();
        if raised != top
            && let Some(id) = raised
        {
            self.damage_window(id);
            changed = true;
        }
        changed
    }
}

/// The state in a static, used only by the holder of a turn.
struct Shared(UnsafeCell<Option<State>>);

// SAFETY: the cell is used only by the holder of a `TURNSTILE` turn.
unsafe impl Sync for Shared {}

static STATE: Shared = Shared(UnsafeCell::new(None));
static TURNSTILE: Turnstile = Turnstile::new();

/// The component's init record.
#[must_use]
pub fn init() -> &'static DriverInit {
    // SAFETY: Kickstart maps every component's init record at `INIT_VA`; it is
    // read in this component's own `AddressSpace` only.
    unsafe { driver_init(INIT_VA) }
}

/// The `EventCount` that tells the frame Thread there is damage; its first
/// advance says the exports can be served.
#[must_use]
pub fn wake_key() -> EventCountKey {
    EventCountKey::from_key(RawKey::from_wire(init().event_count))
}

/// Tell the frame Thread there is something to draw.
pub fn wake() {
    wake_key()
        .advance(1)
        .unwrap_or_else(|error| panic!("compositor: wake failed: {:?}", error.code()));
}

/// Install `state` and open the turnstile on `turns`. The exports are served
/// once the caller advances the `EventCount` of [`wake_key`] for the first time.
pub fn open(state: State, turns: RawKey) {
    // SAFETY: nothing else runs yet: the turnstile is closed, so no Thread can
    // be holding a turn.
    unsafe {
        *STATE.0.get() = Some(state);
    }
    TURNSTILE.open(turns);
}

/// Enter the turnstile: wait for the compositor to be ready and for every
/// earlier holder to leave.
#[must_use]
pub fn enter() -> Turn<'static> {
    TURNSTILE.enter(init())
}

/// The state, for the holder of `turn`.
#[allow(
    clippy::mut_from_ref,
    reason = "the exclusive borrow of the turn proves the holder has the cell to itself"
)]
pub fn state<'turn>(_turn: &'turn mut Turn<'_>) -> &'turn mut State {
    // SAFETY: only the holder of a turn gets here, one at a time.
    unsafe { (*STATE.0.get()).as_mut() }.expect("the compositor opened its state")
}

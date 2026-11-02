//! `compositor_v1`: what clients call to get windows and say what changed in
//! them.

use {
    crate::shared::{self, MAX_TITLE, State, Surface},
    compositor_server::compositor_v1::{CompositorV1, Exception, PixelOrder as WireOrder},
    libcompositor_core::{
        router::Output,
        surface::{FRAME_BITS, FRAME_BYTES, frames},
        windows::{Rect, Window},
    },
    libdevice_driver::{SURFACE_SLOT_BYTES, SURFACE_SLOTS, SURFACE_WINDOW_VA, local_key},
    libgraphics::PixelOrder,
    libmeddle_runtime::{DataArea, IoRec as StageRec},
    libobject::{
        FrameAttribute, FrameKey, KeyTableKey, ObjectType, RawKey, Rights, UntypedKey,
        export::init_return_key, thread::ThreadReturnKey,
    },
    vesper_input_event::record::{EVENT_SIZE, Event},
};

/// Where the first window of the compositor goes, and how far each next one
/// is from the one before.
const FIRST_WINDOW: i32 = 24;
const WINDOW_STEP: i32 = 36;

/// Records of a snapshot: as many as one frame of the router sends.
const MAX_SNAPSHOT: usize = libcompositor_core::router::MAX_ROUTED;

/// The compositor, as clients see it.
pub struct Compositor;

/// The id of the client whose key carries `badge`.
fn client_of(badge: u64) -> Option<u32> {
    let (index, _area) = shared::init().area_for_badge(badge)?;
    u32::try_from(index).ok()?.checked_add(1)
}

/// The surface of window `id`, if client `client` owns it.
fn owned(state: &State, client: u32, id: u32) -> Result<Surface, Exception> {
    state
        .surface_of(id)
        .copied()
        .filter(|surface| surface.client == client)
        .ok_or(Exception::NoWindow)
}

impl CompositorV1 for Compositor {
    fn enter(_badge: u64) {
        init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
            shared::init().return_key,
        )));
    }

    fn pixel_order(_badge: u64) -> WireOrder {
        let mut turn = shared::enter();
        match shared::state(&mut turn).order {
            PixelOrder::Rgb => WireOrder::Rgb,
            PixelOrder::Bgr => WireOrder::Bgr,
        }
    }

    fn create_window(
        badge: u64,
        into: &KeyTableKey,
        width: u32,
        height: u32,
        first_slot: u32,
    ) -> Result<u32, Exception> {
        let client = client_of(badge).ok_or(Exception::NotPermitted)?;
        let count = frames(width, height).ok_or(Exception::BadSize)?;
        let init = shared::init();
        let mut turn = shared::enter();
        let state = shared::state(&mut turn);
        if state.slots_used == SURFACE_SLOTS {
            return Err(Exception::TooManyWindows);
        }
        let slot = state.slots_used;
        // The slot and the memory are spent from here on, whatever happens.
        state.slots_used += 1;
        let first_table_slot = state.next_table_slot;
        let own = KeyTableKey::from_key(RawKey::from_wire(init.table));
        UntypedKey::from_key(RawKey::from_wire(init.memory))
            .retype(
                ObjectType::FRAME,
                FRAME_BITS,
                0,
                count,
                &own,
                first_table_slot,
                Rights::all(),
            )
            .map_err(|_error| Exception::OutOfMemory)?;
        state.next_table_slot = first_table_slot + count;

        let address_space = RawKey::from_wire(init.address_space);
        let base = SURFACE_WINDOW_VA + slot as u64 * SURFACE_SLOT_BYTES;
        for index in 0..count {
            let key = local_key(init.table, first_table_slot + index);
            FrameKey::from_key(key)
                .map(
                    address_space,
                    base + u64::from(index) * FRAME_BYTES,
                    Rights(Rights::READ | Rights::WRITE),
                    FrameAttribute::Default,
                )
                .map_err(|_error| Exception::OutOfMemory)?;
            own.copy_derive(
                key,
                into,
                first_slot.saturating_add(index),
                Rights(Rights::READ | Rights::WRITE | Rights::GRANT),
            )
            .map_err(|_error| Exception::InstallFailed { installed: index })?;
        }

        let id = u32::try_from(slot).unwrap_or(0) + 1;
        let offset = FIRST_WINDOW + WINDOW_STEP * i32::try_from(slot).unwrap_or(0);
        state.surfaces[slot] = Some(Surface {
            window: id,
            client,
            width,
            height,
            base: usize::try_from(base).unwrap_or(0),
            title: [0; MAX_TITLE],
            title_len: 0,
        });
        let added = state.add_window(Window {
            id,
            client,
            rect: Rect {
                x: offset,
                y: offset,
                width,
                height,
            },
        });
        if !added {
            state.surfaces[slot] = None;
            return Err(Exception::TooManyWindows);
        }
        // A new window gets the keyboard focus; the window that had it hears
        // it lost it.
        let stamp = libuser::clock::now_ns();
        let mut out = Output::new();
        state.router.focus_window(id, stamp, &mut out);
        state.deliver(&out, stamp);
        shared::wake();
        Ok(id)
    }

    fn commit(
        badge: u64,
        window: u32,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<(), Exception> {
        let client = client_of(badge).ok_or(Exception::NoWindow)?;
        let mut turn = shared::enter();
        let state = shared::state(&mut turn);
        let surface = owned(state, client, window)?;
        let origin = state.rect_of(window).ok_or(Exception::NoWindow)?;
        let changed = Rect {
            x,
            y,
            width,
            height,
        };
        let bounds = Rect {
            x: 0,
            y: 0,
            width: surface.width,
            height: surface.height,
        };
        if let Some(inside) = changed.intersect(&bounds) {
            state.damage.add(Rect {
                x: origin.x.saturating_add(inside.x),
                y: origin.y.saturating_add(inside.y),
                ..inside
            });
            shared::wake();
        }
        Ok(())
    }

    fn set_title(badge: u64, window: u32, title: StageRec) -> Result<(), Exception> {
        let client = client_of(badge).ok_or(Exception::NotPermitted)?;
        let (_, exported) = shared::init()
            .area_for_badge(badge)
            .ok_or(Exception::NotPermitted)?;
        let area = DataArea {
            base: exported.base,
            len: exported.len,
        };
        let length = usize::try_from(title.len).unwrap_or(usize::MAX);
        if !area.contains(&title) || length > MAX_TITLE {
            return Err(Exception::NotPermitted);
        }
        let mut bytes = [0; MAX_TITLE];
        if !area.read_bytes(title.base - area.base, &mut bytes[..length]) {
            return Err(Exception::NotPermitted);
        }
        let mut turn = shared::enter();
        let state = shared::state(&mut turn);
        owned(state, client, window)?;
        if let Some(surface) = usize::try_from(window.wrapping_sub(1))
            .ok()
            .and_then(|slot| state.surfaces.get_mut(slot))
            .and_then(Option::as_mut)
        {
            surface.title = bytes;
            surface.title_len = length;
        }
        Ok(())
    }

    fn move_window(badge: u64, window: u32, x: i32, y: i32) -> Result<(), Exception> {
        let client = client_of(badge).ok_or(Exception::NoWindow)?;
        let mut turn = shared::enter();
        let state = shared::state(&mut turn);
        owned(state, client, window)?;
        state.damage_window(window);
        state.router.windows_mut().move_to(window, x, y);
        state.damage_window(window);
        shared::wake();
        Ok(())
    }

    fn raise(badge: u64, window: u32) -> Result<(), Exception> {
        let client = client_of(badge).ok_or(Exception::NoWindow)?;
        let mut turn = shared::enter();
        let state = shared::state(&mut turn);
        owned(state, client, window)?;
        state.router.windows_mut().raise(window);
        state.damage_window(window);
        shared::wake();
        Ok(())
    }

    fn destroy(badge: u64, window: u32) -> Result<(), Exception> {
        let client = client_of(badge).ok_or(Exception::NoWindow)?;
        let mut turn = shared::enter();
        let state = shared::state(&mut turn);
        owned(state, client, window)?;
        state.remove_window(window);
        shared::wake();
        Ok(())
    }

    fn snapshot(badge: u64, stage: StageRec) -> Result<u32, Exception> {
        let client = client_of(badge).ok_or(Exception::NotPermitted)?;
        let (_, exported) = shared::init()
            .area_for_badge(badge)
            .ok_or(Exception::NotPermitted)?;
        let area = DataArea {
            base: exported.base,
            len: exported.len,
        };
        let mut records = [Event::sync(0, 0); MAX_SNAPSHOT];
        let count = {
            let mut turn = shared::enter();
            shared::state(&mut turn)
                .router
                .snapshot(client, libuser::clock::now_ns(), &mut records)
        };
        let length = count * EVENT_SIZE;
        if !area.contains(&stage) || stage.len < length as u64 {
            return Err(Exception::NotPermitted);
        }
        let mut offset = stage.base - area.base;
        for record in &records[..count] {
            if !area.write_bytes(offset, &record.to_bytes()) {
                return Err(Exception::NotPermitted);
            }
            offset += EVENT_SIZE as u64;
        }
        Ok(u32::try_from(length).unwrap_or(u32::MAX))
    }
}

compositor_server::compositor_v1_export!(Compositor);

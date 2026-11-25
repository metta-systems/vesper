//! `display_layout_v1`: reading and arranging the outputs, through the keys
//! `compositor_v1.display_layout` installs.
//!
//! Any client reads. A set function first checks the layout bit of the key's
//! badge (`libdevice_driver::LAYOUT_BADGE`, set by Kickstart for a client
//! whose manifest entry says `layout = true`), then works on the state under a
//! turn. An output turns itself when `display_v1.describe` listed the turn;
//! otherwise it stands upright and the compositor turns its pixels while
//! drawing (M5). Every change goes through `State::apply_layout`, which moves
//! the desktop origin (D6) and draws everything again.

use {
    crate::shared::{self, Display, Memory, State},
    compositor_server::display_layout_v1::{DisplayLayoutV1, Exception, Rotation},
    display::display_v1::Rotation as OutputRotation,
    display_client::display_v1::{Bindings as DisplayBindings, Client as DisplayClient},
    libcompositor_core::{layout::turned_size, windows::Rect},
    libdevice_driver::LAYOUT_BADGE,
    libobject::{RawKey, export::init_return_key, thread::ThreadReturnKey},
};

/// The compositor's outputs, as layout clients see them.
pub struct Arranger;

/// Quarter turns of a rotation.
const fn quarter_turns(rotation: Rotation) -> u8 {
    match rotation {
        Rotation::R0 => 0,
        Rotation::R90 => 1,
        Rotation::R180 => 2,
        Rotation::R270 => 3,
    }
}

/// The rotation of `turns` quarter turns.
const fn rotation(turns: u8) -> Rotation {
    match turns % 4 {
        1 => Rotation::R90,
        2 => Rotation::R180,
        3 => Rotation::R270,
        _ => Rotation::R0,
    }
}

/// `display_v1`'s rotation of `turns` quarter turns.
const fn output_rotation(turns: u8) -> OutputRotation {
    match turns % 4 {
        1 => OutputRotation::R90,
        2 => OutputRotation::R180,
        3 => OutputRotation::R270,
        _ => OutputRotation::R0,
    }
}

/// Whether the key `badge` came through may arrange the outputs: it belongs
/// to a client, and Kickstart set its layout bit.
fn may_arrange(badge: u64) -> bool {
    badge & LAYOUT_BADGE != 0 && shared::init().area_for_badge(badge).is_some()
}

/// The index in the output table of the output with id `id`.
fn index_of(state: &State, id: u32) -> Result<usize, Exception> {
    state
        .displays
        .iter()
        .position(|display| {
            display.is_some_and(|display| u32::try_from(display.instance).ok() == Some(id))
        })
        .ok_or(Exception::NoOutput)
}

/// Output `index` and its viewport.
fn output(state: &State, index: usize) -> Result<(Display, Rect), Exception> {
    let display = state
        .displays
        .get(index)
        .copied()
        .flatten()
        .ok_or(Exception::NoOutput)?;
    let viewport = state
        .router
        .layout()
        .viewport(index)
        .ok_or(Exception::NoOutput)?;
    Ok((display, viewport))
}

impl DisplayLayoutV1 for Arranger {
    fn enter(_badge: u64) {
        init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
            shared::init().return_key,
        )));
    }

    fn generation(_badge: u64) -> u32 {
        let mut turn = shared::enter();
        shared::state(&mut turn).layout_generation
    }

    fn count(_badge: u64) -> u32 {
        let mut turn = shared::enter();
        let count = shared::state(&mut turn).displays.iter().flatten().count();
        u32::try_from(count).unwrap_or(0)
    }

    fn output(_badge: u64, index: u32) -> Result<(u32, i32, i32, u32, u32), Exception> {
        let index = usize::try_from(index).map_err(|_overflow| Exception::NoOutput)?;
        let mut turn = shared::enter();
        let (display, viewport) = output(shared::state(&mut turn), index)?;
        Ok((
            u32::try_from(display.instance).unwrap_or(u32::MAX),
            viewport.x,
            viewport.y,
            viewport.width,
            viewport.height,
        ))
    }

    fn output_details(_badge: u64, index: u32) -> Result<(Rotation, u32, u32), Exception> {
        let index = usize::try_from(index).map_err(|_overflow| Exception::NoOutput)?;
        let mut turn = shared::enter();
        let (display, _viewport) = output(shared::state(&mut turn), index)?;
        Ok((
            rotation(display.turns()),
            display.width_mm,
            display.height_mm,
        ))
    }

    fn set_position(badge: u64, id: u32, x: i32, y: i32) -> Result<(), Exception> {
        if !may_arrange(badge) {
            return Err(Exception::NotPermitted);
        }
        let mut turn = shared::enter();
        let state = shared::state(&mut turn);
        let index = index_of(state, id)?;
        let (_display, viewport) = output(state, index)?;
        let moved = Rect { x, y, ..viewport };
        let mut layout = *state.router.layout();
        if layout.overlaps(index, &moved) {
            return Err(Exception::Overlaps);
        }
        layout.set(index, Some(moved));
        state.apply_layout(layout);
        drop(turn);
        shared::wake();
        Ok(())
    }

    fn set_rotation(badge: u64, id: u32, rotation: Rotation) -> Result<(), Exception> {
        if !may_arrange(badge) {
            return Err(Exception::NotPermitted);
        }
        let turns = quarter_turns(rotation);
        let mut turn = shared::enter();
        let state = shared::state(&mut turn);
        let index = index_of(state, id)?;
        let (mut display, viewport) = output(state, index)?;
        // The viewport keeps its top left corner and takes the turned size.
        let (width, height) = turned_size(display.upright, turns);
        let turned = Rect {
            width,
            height,
            ..viewport
        };
        let mut layout = *state.router.layout();
        if layout.overlaps(index, &turned) {
            return Err(Exception::Overlaps);
        }
        // The output turns itself if it can; otherwise it stands upright and
        // the compositor turns the pixels.
        let hardware_turns = if display.rotations & (1 << turns) != 0 {
            turns
        } else {
            0
        };
        if hardware_turns != display.hardware_turns {
            // The call holds the turn: drawing waits the few commands a turn
            // takes, and nothing draws for the old orientation meanwhile.
            let output = DisplayClient::new(DisplayBindings::from_imports_nth(
                shared::init().imports(),
                display.instance,
            ));
            output
                .rotate(output_rotation(hardware_turns))
                .map_err(|_error| Exception::Failed)?;
            let (native_width, native_height, pitch, _format) =
                output.get_mode().map_err(|_error| Exception::Failed)?;
            display.native = (native_width, native_height);
            display.hardware_turns = hardware_turns;
            if let Memory::Flush { base, len, .. } = display.memory {
                display.memory = Memory::Flush {
                    base,
                    len,
                    pitch: pitch as usize,
                };
            }
        }
        display.software_turns = (turns + 4 - hardware_turns) % 4;
        display.width = width;
        display.height = height;
        state.displays[index] = Some(display);
        layout.set(index, Some(turned));
        state.apply_layout(layout);
        drop(turn);
        shared::wake();
        Ok(())
    }

    fn window_output(badge: u64, window: u32) -> Result<u32, Exception> {
        let (client, _area) = shared::init()
            .area_for_badge(badge)
            .ok_or(Exception::NoOutput)?;
        let client = u32::try_from(client).map_err(|_overflow| Exception::NoOutput)? + 1;
        let mut turn = shared::enter();
        let state = shared::state(&mut turn);
        if state
            .surface_of(window)
            .is_none_or(|surface| surface.client != client)
        {
            return Err(Exception::NoOutput);
        }
        let rect = state.rect_of(window).ok_or(Exception::NoOutput)?;
        let index = state
            .router
            .layout()
            .output_of(&rect)
            .ok_or(Exception::NoOutput)?;
        let (display, _viewport) = output(state, index)?;
        u32::try_from(display.instance).map_err(|_overflow| Exception::NoOutput)
    }
}

compositor_server::display_layout_v1_export!(Arranger);

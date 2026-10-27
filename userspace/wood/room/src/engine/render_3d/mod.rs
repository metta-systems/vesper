//! `r_*` sources: the 3D renderer (BSP traversal, walls, floors, sprites,
//! textures, lighting).
//!
//! The C renderer globals live in one [`Renderer`] context, held in
//! [`RENDERER`]. A frame is drawn in phases (`R_SetupFrame`, `R_RenderBSP`,
//! `R_DrawPlanes`, `R_DrawMasked`); `D_Display` borrows the world and the
//! renderer for each phase and hands them down, and runs `NetUpdate`
//! between phases with no borrow held. The renderer never reaches for the
//! world itself.

pub mod bsp;
pub mod data;
pub mod draw;
pub mod main;
pub mod plane;
pub mod segs;
pub mod sky;
pub mod things;

use crate::engine::global::Global;

use self::{
    bsp::{BspState, ClipSegsState},
    data::RenderDataState,
    draw::DrawState,
    main::ViewState,
    plane::{MAXOPENINGS, PlaneState},
    segs::WallState,
    sky::SkyState,
    things::{SpriteState, VisSpriteState},
};

/// The renderer context: the C `r_*.c` globals.
///
/// Several parts hold raw pointers into other parts that stay valid across
/// phases (the drawsegs' clip slices of `openings` and of the sprite clip
/// arrays, the light-table rows in the wall, sprite and plane state, the
/// vissprite list links), so the context lives at a fixed address in
/// [`RENDERER`] and each pointer is derived from the reference the phase
/// that stores it was handed.
pub struct Renderer {
    /// View position, projection and light tables (`r_main.c`).
    pub view: ViewState,
    /// BSP traversal state and the frame's drawsegs (`r_bsp.c`).
    pub bsp: BspState,
    /// The solid-column occlusion list (`r_bsp.c` `solidsegs`).
    pub clip_segs: ClipSegsState,
    /// View-buffer geometry and lookup tables (`r_draw.c`).
    pub draw: DrawState,
    /// Visplanes and the floor/ceiling clip arrays (`r_plane.c`).
    pub planes: PlaneState,
    /// Scratch buffer for the drawsegs' sprite clip and masked column
    /// arrays (`r_plane.c` `openings`), handed out through
    /// `planes.lastopening`.
    pub openings: [i16; MAXOPENINGS],
    /// The wall being drawn (`r_segs.c`).
    pub wall: WallState,
    /// Sprite definitions, sprite scales and clip arrays (`r_things.c`).
    pub sprites: SpriteState,
    /// The frame's vissprite pool (`r_things.c`).
    pub vissprites: VisSpriteState,
    /// The level's sky (`r_sky.c`).
    pub sky: SkyState,
    /// Texture, flat, sprite lump and colormap tables (`r_data.c`).
    pub data: RenderDataState,
}

impl Renderer {
    /// The state before `R_Init`.
    pub const fn new() -> Self {
        Self {
            view: ViewState::new(),
            bsp: BspState::new(),
            clip_segs: ClipSegsState::new(),
            draw: DrawState::new(),
            planes: PlaneState::new(),
            openings: [0; MAXOPENINGS],
            wall: WallState::new(),
            sprites: SpriteState::new(),
            vissprites: VisSpriteState::new(),
            sky: SkyState::new(),
            data: RenderDataState::new(),
        }
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

/// The renderer context. Entry points borrow it once (per render phase for a
/// frame) and pass it down; it is never borrowed across `NetUpdate`, whose
/// responders may resize the view.
pub static RENDERER: Global<Renderer> = Global::new(Renderer::new());

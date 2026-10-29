//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

// Host builds compile this binary natively too; it is only an empty `main` there.
#![cfg_attr(target_os = "vesper", no_std, no_main)]

//! `wood`: the Room Doom engine as a Vesper program. It carves its stack and
//! heap from its RAM, asks the compositor for a 640x400 window, and runs the
//! engine with the IWAD from its bundled resource (`doom1_wad`). The engine
//! plays its demos in the window until keys arrive: the keys the compositor
//! routes to the window, which has the focus when it is new or clicked.
//! Its sound effects play through the PWM audio driver when the image has
//! one, mixed on a second Thread (`sound`) on top of the music the MIDI
//! synthesiser component renders (`music`).

#[cfg(target_os = "vesper")]
extern crate alloc;

#[cfg(target_os = "vesper")]
mod heap;
#[cfg(target_os = "vesper")]
mod keyboard;
#[cfg(target_os = "vesper")]
mod logger;
#[cfg(target_os = "vesper")]
mod music;
#[cfg(target_os = "vesper")]
mod platform;
#[cfg(target_os = "vesper")]
mod sound;
#[cfg(target_os = "vesper")]
mod window;

#[cfg(target_os = "vesper")]
mod program {
    use {
        crate::{
            heap::{self, ENGINE_STACK_TOP},
            keyboard::Keyboard,
            logger,
            platform::{WAD_NAME, Wood},
            window::Window,
        },
        alloc::{borrow::ToOwned, boxed::Box, vec},
        compositor_client::compositor_v1::{
            Bindings as CompositorBindings, Client as CompositorClient,
        },
        libdevice_driver::driver_init,
        libobject::{EventCountKey, RawKey},
        room::engine::{doom::main::doomgeneric_Tick, platform::doomgeneric_Create},
    };

    libuser::entry!(main);

    /// Nanoseconds between two tries while the compositor cannot answer yet.
    const RETRY_NS: u64 = 200_000_000;

    fn main(init_va: u64) -> ! {
        // SAFETY: Kickstart passes the init page's address in x0.
        let init = unsafe { driver_init(init_va) };
        logger::install();
        let first_slot = u32::try_from(init.first_free_slot).unwrap_or(u32::MAX);
        let table_slot = heap::carve(init, first_slot)
            .unwrap_or_else(|error| panic!("wood: carving memory failed: {:?}", error.code()));
        let wad = init
            .resource(0)
            .unwrap_or_else(|| panic!("wood: no {WAD_NAME} resource"));
        log::info!("{WAD_NAME}: {} bytes", wad.len());

        let compositor = CompositorClient::new(CompositorBindings::from_imports(init.imports()));
        // The component's own EventCount: awaiting a count it never reaches
        // is a timer (the sound hand-over advances it, see `sound`).
        let timer = EventCountKey::from_key(RawKey::from_wire(init.event_count));
        let order = loop {
            match compositor.pixel_order() {
                Ok(order) => break order,
                Err(_) => {
                    let _slept = timer.sleep(RETRY_NS);
                }
            }
        };
        let keyboard = Keyboard::open(&compositor);
        let window = Window::create(init, compositor, order, table_slot);

        let wood: &'static Wood = Box::leak(Box::new(Wood::new(window, keyboard, timer, wad)));
        // SAFETY: the engine's stack Frame is mapped (`heap::carve`) and
        // nothing else uses it; this Thread never comes back to its own stack.
        unsafe {
            core::arch::asm!(
                "mov sp, {top}",
                "mov x29, xzr",
                "br {entry}",
                top = in(reg) ENGINE_STACK_TOP,
                entry = in(reg) run as extern "C" fn(&'static Wood) -> !,
                in("x0") wood,
                options(noreturn),
            );
        }
    }

    /// Start the engine on `wood` and tick it for good, on the engine's
    /// stack.
    extern "C" fn run(wood: &'static Wood) -> ! {
        let arguments = vec!["wood".to_owned(), "-iwad".to_owned(), WAD_NAME.to_owned()];
        // SAFETY: called once, at startup.
        unsafe {
            doomgeneric_Create(wood, arguments);
        }
        log::info!("the engine started");
        loop {
            doomgeneric_Tick();
        }
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}

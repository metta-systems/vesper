//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

// Host tests build this binary natively too; it is only an empty `main` there.
#![cfg_attr(target_os = "vesper", no_std, no_main)]
// libqemu's semihosting `println!` (the lamp's progress under QEMU) expands to it.
#![cfg_attr(all(target_os = "vesper", feature = "qemu"), feature(format_args_nl))]

//! `draw-demo`: the shell's `draw` without the shell. It asks the compositor for
//! a window as soon as it starts, paints the shell's scene in it and leaves the
//! lava lamp animating while the window has the keyboard focus (paused, asleep
//! on the compositor's events, while another window has it), for an image whose
//! serial channel belongs to someone else (the compositor test image).

#[cfg(target_os = "vesper")]
#[path = "display.rs"]
mod display;

#[cfg(target_os = "vesper")]
mod program {
    use {
        compositor_client::compositor_v1::{
            Bindings as CompositorBindings, Client as CompositorClient,
        },
        libdevice_driver::driver_init,
        libobject::{CapError, EventCountKey, RawKey},
    };

    libuser::entry!(main);

    /// Milliseconds between two tries while the compositor cannot make the
    /// window yet.
    const RETRY_MS: u64 = 200;

    fn main(init_va: u64) -> ! {
        // SAFETY: Kickstart passes the init page's address in x0.
        let init = unsafe { driver_init(init_va) };
        let compositor = CompositorClient::new(CompositorBindings::from_imports(init.imports()));
        // The component's own EventCount, which nothing advances: a timer.
        let timer = EventCountKey::from_key(RawKey::from_wire(init.event_count));
        let sleep = || match timer.await_ge(u64::MAX, RETRY_MS * 1_000_000) {
            Ok(_) | Err(CapError::TimedOut { .. }) => {}
            Err(error) => panic!("draw-demo: sleeping failed: {:?}", error.code()),
        };
        while crate::display::draw(init, &compositor).is_err() {
            sleep();
        }
        // The lava lamp runs on the extra Thread `draw` released.
        loop {
            sleep();
        }
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}

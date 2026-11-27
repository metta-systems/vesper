# UI compositor completed

Short markers of what is done. Each line says what exists and where it is exercised.
Open items stay in [ui-compositor-implementation-plan.md](ui-compositor-implementation-plan.md);
the design is in [ui-compositor-design.md](ui-compositor-design.md). No compositor
decision is open.

## Completed decisions

Each is recorded in [ui-compositor-design.md](ui-compositor-design.md).

- [x] Q1 (2026-10-19): the compositor carves the surface Frames and installs them in
  the client's lent table; the client just draws.
- [x] Q2 (2026-10-18, rule 2026-10-19): granularity by size; a surface is its bytes
  rounded up to 2 MiB, made of 2 MiB Frames.
- [x] Q3 (2026-10-18): one event io-channel per client, the window id in each record.
- [x] Q4 (2026-10-18): the compositor routes input itself; an `inputd`-style router is
  future work.
- [x] Q5 (2026-10-18): client-side decorations; the compositor runs the Alt-drag grab.
- [x] Q6 (2026-10-18): ARGB with premultiplied blending from version 1; hit-testing by
  window rectangles.
- [x] Q7 (2026-10-18, rule 2026-10-19): a fixed 33 ms timer that runs only while damage
  exists; the adaptive rate is deferred.
- [x] Q8 (2026-10-18): `uses ... via device ...` and `uses ... via component ...`.
- [x] Q9 (2026-10-18): the component lives in `userspace/gui/compositor`.
- [x] Q10 (2026-10-18): the input decisions the routing needed (input D1 to D4, D13)
  were answered first.
- [x] Q11 (2026-10-18): a client channel's overflow drops the newest delivery, emits
  `Dropped` and resyncs from state.
- [x] Q12 (2026-10-27): a new window gets the focus; a click still moves it.

## C0: documents and interface

- [x] The three `ui-compositor-*` documents, and `compositor_v1.if` (`userspace/gui/compositor/interfaces`), a `local` interface with a `placement` window.

## C1: pure routing library

- [x] `userspace/libs/compositor-core`: the window list with stacking and hit-testing, and the input router (pointer clamped to the screen, focus and raise on the first button, an implicit grab until the last button is released, `Enter`, `Leave` and `Focus` records, window-local pointer positions, keys to the focused window, release of a removed device's keys, resync from a snapshot); host-tested by `just _test-compositor-host`.
- [x] The compositor is the only client of the USB HID component's `input_v1`: it posts the buffers, folds the records into the router, recovers devices and dropped records with `descriptor` and `snapshot`, and traces what each routed window receives; `just _test-compositor` under QEMU, `just _boot-compositor-test` the Raspberry Pi 3 image.

## C2: compositor owns the screen

- [x] `compositor-core` has the damage set, clipped premultiplied-alpha blitting, the cursor sprite and the surface size rule (`just _test-compositor-host`).
- [x] `userspace/libs/framebuffer` maps the framebuffer write-combining, and only the compositor does; the shell draws only in a compositor window.
- [x] The compositor draws a background, its clients' windows and the cursor in a cacheable back buffer and copies the damaged rectangles out from a frame Thread, with a 33 ms timer that runs only while there is damage (Q7); `just _test-compositor` checks that the background reaches the framebuffer.

## C3: surfaces and the first client

- [x] `compositor_v1` has `create_window`, `commit`, `set_title`, `move_window`, `raise`, `destroy` and `pixel_order`; surfaces are 2 MiB Frames the compositor carves and installs in the client's lent table (Q1, Q2), and Kickstart gives importers a surface window (`DriverInit::map_window`). The shell's `draw` makes a window and draws the lamp in it; `just _test-shell` checks the lamp through it.

## C4: per-client event channel and routing

- [x] One event io-channel per client (Q3): `compositor_v1` declares `channel 1;`, so Kickstart gives every importer a control page, a data page and an EventCount; the compositor delivers each routed frame's records for the client's windows (window id in `subject`, each window's records closed by a `Sync`) under its turn, drops a delivery whole when no buffer fits and starts the next with `Dropped` (Q11), and `compositor_v1.snapshot` rebuilds the client's focus, held keys and pointer; a new window gets the focus (Q12). `userspace/libs/compositor-core` (`events`, `Router::focus_window`, `Router::snapshot`) is host-tested by `just _test-compositor-host`; `just _test-window-events` checks focus on creation, click to focus, a drag across windows and a key at the client `window-events` (QEMU monitor input on the USB mouse and keyboard, not an injected loopback producer), which exits with the result; `just _test-wood` checks that Wood receives keys.

## C5: real input

- [x] The production image carries the USB stack, and the compositor takes the pointer and the keyboard from it.
